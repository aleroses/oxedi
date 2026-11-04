#!/usr/bin/env python3
"""Runs a command of this checkout in a throwaway, hardened container of the sandbox image.

The container runs as the user, with a read-only root, no capabilities, no new privileges, the
default seccomp profile and limits on processes, memory and CPUs. It sees the checkout at /work,
with the files the host executes later (Makefile, make/, tools/, ...) bound read-only on top, the
maintainers' notes and every path git ignores hidden, and its own storage under
${XDG_CACHE_HOME:-~/.cache}/oxedi-sandbox: target/ and .venv/ per checkout, the cargo registry
shared. No host variable and no part of the
host home other than that storage reaches it. Two phases, each in its own containers:
- fetch: with network, the checkout read-only, no project code run: `cargo fetch --locked` and
  wheels only for the venv, written to the shared caches; it never falls back to them offline;
- run: the command, with no network and the cargo registry read-only.
After the run, an audit lists every file it changed, every new path it left and every path it
cannot read, and removes the Python caches and the built extension it may have left in the
checkout. A dirty audit is recorded in the checkout's storage, and the next start refuses while a
path it named is still as the run left it.

Run it isolated (`python3 -I`), as make does, so no file in the checkout or variable of the shell
can shadow a module it imports.

usage: sandbox.py --image TAG --kind {base,viewer} --checkout DIR
       sandbox.py --clean {checkout,all} --checkout DIR
`--kind viewer` names a viewer image (make sandbox-image-viewer): its runs also get the docs venv
at .venv-docs, made by the fetch from the hashed docs/requirements.txt, and the run is refused when
the checkout pins another Playwright, wasm-bindgen or binaryen than the image was built with.
The command comes from SANDBOX_CMD. SANDBOX_CPUS (default: the daemon's CPUs minus one),
SANDBOX_MEMORY (GB; default 60 % of the daemon's memory) and SANDBOX_TIMEOUT (seconds; default
3600) set the limits.
Exit status: the command's; 3 when the audit found a change, a new path or a path it cannot read
(whatever the command's status); 124 when it ran past the timeout; 130 on Ctrl-C (SIGINT) and
143 on SIGTERM; 2 when the sandbox refuses to start.
"""

import argparse
import os
import shutil
import signal
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import TypeVar

# Isolated mode leaves the script's own directory out of sys.path; tools/ is read-only in a run.
sys.path.insert(0, str(Path(__file__).resolve().parent))

import viewer_image  # noqa: E402
from arguments import (  # noqa: E402
    ImageKind, Limits, Phase, Runtime, docker_args, fetch_commands, own_dirs, own_sources, run_command,
)
from audit import DIRTY_STATUS, Findings, Snapshot, audit, record, refuse_leftovers, report, snapshot  # noqa: E402
from layout import Checkout, SandboxError, container_name, path_hash, refuse_links, survey  # noqa: E402
from lifecycle import (  # noqa: E402
    DaemonInfo, daemon_info, explain, image_labels, limits_from, refuse_if_running, run_container, running,
)
from storage import Storage, cache_root, clean, empty_dir, prepare, storage_for  # noqa: E402

FETCH_BUDGET_S = 600
# The Playwright version the viewer image's browser was installed for.
PLAYWRIGHT_LABEL = "oxedi.sandbox.playwright"
# The signals that stop the runner the way Ctrl-C does: the container is killed and the checkout
# audited before it exits 128 + the signal's number.
STOP_SIGNALS = (signal.SIGINT, signal.SIGTERM)


class Stopped(BaseException):
    """A stop signal arrived; raised from its handler so every `finally` and the audit run."""

    def __init__(self, signum: int):
        super().__init__(signal.Signals(signum).name)
        self.signum = signum


def stop(signum: int, _frame) -> None:
    raise Stopped(signum)



@dataclass(frozen=True)
class Plan:
    """Everything a phase's `docker run` is built from."""

    image: str
    kind: ImageKind
    checkout: Checkout
    storage: Storage
    runtime: Runtime
    limits: Limits

    def argv(self, phase: Phase, command: list[str]) -> list[str]:
        options = docker_args(
            phase, self.checkout, os.getuid(), os.getgid(), self.runtime, self.limits, self.storage, self.kind,
        )
        return ["docker", "run", *options, self.image, *command]


def describe(plan: Plan, labels: dict[str, str]) -> str:
    rust = labels.get("oxedi.sandbox.rust", "unlabelled")
    limits = plan.limits
    own = ", ".join(p.name for p in (plan.storage.target, *own_sources(plan.storage, plan.kind).values()))
    return " · ".join([
        f"sandbox: runtime {plan.runtime.value}",
        f"image {plan.image} ({plan.kind.value}, Rust {rust})",
        f"{limits.cpus} CPUs, {limits.memory_gb} GB, {limits.timeout_s} s",
        f"storage {plan.storage.target.parent} ({own}), shared {plan.storage.cache} (cargo, uv)",
        f"read-only: {', '.join(plan.checkout.read_only) or 'none'}",
        f"masked: {', '.join(plan.checkout.masked) or 'none'}",
        f"hidden: {', '.join(plan.checkout.hidden) or 'none'}",
    ])


def fetch_phase(plan: Plan) -> int:
    """Runs the fetch commands, each in its own container, within one budget; stops at the first
    failure."""
    started = time.monotonic()
    # The run phase writes the venvs: a planted interpreter or .pth must never run with the network.
    empty_dir(plan.storage.venv)
    empty_dir(plan.storage.docs_venv)
    for command in fetch_commands(plan.kind):
        remaining = FETCH_BUDGET_S - (time.monotonic() - started)
        outcome = run_container(plan.argv(Phase.FETCH, command), plan.checkout.name, max(1.0, remaining))
        if outcome.status != 0:
            budget = f"the fetch ran past its {FETCH_BUDGET_S} s budget"
            failure = (f"fetch failed: {' '.join(command)} exited {outcome.status}; "
                       "read its output above (no network, or a package or group the checkout does not declare)")
            print(f"sandbox: {explain(outcome, plan.limits, budget, failure)}", file=sys.stderr)
            return outcome.status
    print(f"sandbox: fetch took {time.monotonic() - started:.1f} s", file=sys.stderr)
    return 0


def run_phase(plan: Plan, command: str) -> int:
    started = time.monotonic()
    outcome = run_container(plan.argv(Phase.RUN, run_command(command)), plan.checkout.name, plan.limits.timeout_s)
    print(f"sandbox: run took {time.monotonic() - started:.1f} s", file=sys.stderr)
    if outcome.status != 0:
        budget = f"the command ran past SANDBOX_TIMEOUT={plan.limits.timeout_s} s"
        print(f"sandbox: {explain(outcome, plan.limits, budget, f'command exited {outcome.status}')}", file=sys.stderr)
    return outcome.status


def phases(plan: Plan, command: str) -> int:
    status = fetch_phase(plan)
    return status if status != 0 else run_phase(plan, command)


def conclude(before: Snapshot, plan: Plan) -> Findings:
    """Audits the checkout, prints the findings and records them (or clears the record)."""
    findings = audit(before, plan.checkout)
    report(findings)
    record(findings, plan.checkout.root, plan.storage.record)
    return findings


Result = TypeVar("Result")


def audited(plan: Plan, work: Callable[[], Result]) -> tuple[Result, Findings]:
    """Runs `work` (the containers), then audits the checkout, also after a stop signal or any
    other error. When that audit fails too, its error is printed and the first one is raised."""
    before = snapshot(plan.checkout.root)
    try:
        result = work()
    except BaseException:
        try:
            conclude(before, plan)
        except BaseException as failed:
            print(f"sandbox: the audit failed as well, so review the checkout with git status and git diff: {failed}",
                  file=sys.stderr)
        raise
    return result, conclude(before, plan)


def checked_daemon() -> DaemonInfo:
    """The Docker daemon's details; a missing or rootless daemon is refused."""
    info = daemon_info()
    if info.rootless:
        raise SandboxError(
            "refused: the Docker daemon is rootless (name=rootless in docker info); "
            "use a rootful Docker Engine or Docker Desktop"
        )
    return info


def checkout_pin(root: Path, relative: str, parse: Callable[[str], str]) -> str:
    """One pin of the checkout's file `relative`, read as data on the host with viewer_image.py's
    parser `parse`; a link anywhere on that path is refused, since its target, not the checkout's
    file, would be read."""
    refuse_links(root, relative, f"{relative} must be a real file of the checkout")
    try:
        return parse(viewer_image.read(root, relative))
    except SystemExit as error:
        # viewer_image.py ends its command line with one SystemExit line; here it is a refusal.
        raise SandboxError(str(error.code).removeprefix(viewer_image.PREFIX).strip()) from None


@dataclass(frozen=True)
class ImagePin:
    """A tool the viewer image holds at the version one of the checkout's files pins."""

    tool: str
    label: str
    source: str
    parse: Callable[[str], str]


# What the viewer image was built from, each read from the checkout and compared with its label.
IMAGE_PINS = (
    ImagePin("playwright", PLAYWRIGHT_LABEL, viewer_image.DOCS_REQUIREMENTS, viewer_image.playwright_pin),
    ImagePin("wasm-bindgen", "oxedi.sandbox.wasm-bindgen", viewer_image.INSTALL_TOOLS,
             lambda text: viewer_image.wasm_pins(text).bindgen_version),
    ImagePin("binaryen", "oxedi.sandbox.binaryen", viewer_image.INSTALL_TOOLS,
             lambda text: viewer_image.wasm_pins(text).binaryen_version),
)


def refuse_stale_image(root: Path, labels: dict[str, str]) -> None:
    """The viewer image holds the browser one Playwright version installs and the wasm tools
    install_tools.sh pins: a checkout that pins another version of any of them is refused here,
    before any container, instead of failing in the run (Playwright finds no browser of its own
    revision, make wasm asks for a wasm-bindgen it cannot install offline) or passing with the
    image's older wasm-opt."""
    for pin in IMAGE_PINS:
        pinned = checkout_pin(root, pin.source, pin.parse)
        installed = labels.get(pin.label, f"no {pin.label} label")
        if pinned != installed:
            raise SandboxError(
                f"this checkout pins {pin.tool} {pinned} but the viewer image has {installed}; "
                "run make sandbox-image-viewer"
            )


def ready(image: str, kind: ImageKind, checkout_dir: Path, env: dict[str, str], info: DaemonInfo) -> Plan:
    """The plan for a checkout, once nothing refuses it: the image exists (a viewer image built for
    the checkout's Playwright and wasm tools), no container of the checkout runs and no path of its
    last dirty audit is left. Its storage is made and the plan printed."""
    runtime = Runtime.RUNSC if Runtime.RUNSC.value in info.runtimes else Runtime.RUNC
    checkout = survey(checkout_dir)
    plan = Plan(image, kind, checkout, storage_for(cache_root(env), checkout), runtime, limits_from(env, info))
    labels = image_labels(image)
    if kind is ImageKind.VIEWER:
        refuse_stale_image(checkout.root, labels)
    refuse_if_running(checkout.name)
    refuse_leftovers(checkout.root, plan.storage.record)
    prepare(plan.storage, checkout, ("target", *own_dirs(kind)))
    print(describe(plan, labels), file=sys.stderr)
    return plan


def sandbox(image: str, kind: ImageKind, checkout_dir: Path, env: dict[str, str]) -> int:
    command = env.get("SANDBOX_CMD", "").strip()
    if not command:
        raise SandboxError('no command: set CMD, for example make sandbox CMD="make gates"')
    plan = ready(image, kind, checkout_dir, env, checked_daemon())
    status, findings = audited(plan, lambda: phases(plan, command))
    return DIRTY_STATUS if findings.dirty else status


def clean_storage(everything: bool, checkout_dir: Path, env: dict[str, str]) -> int:
    """Removes this checkout's storage, or all of it, unless a sandbox container that uses it runs."""
    cache = cache_root(env)
    doomed = cache if everything else cache / path_hash(checkout_dir)
    pattern = "^oxedi-sandbox-" if everything else f"^{container_name(checkout_dir)}$"
    busy = running(pattern) if shutil.which("docker") else []
    if busy:
        raise SandboxError(f"{', '.join(busy)} is running and uses {doomed}; wait for it or stop it with docker kill")
    done = "removed" if clean(doomed) else "nothing to remove at"
    print(f"sandbox: {done} {doomed}", file=sys.stderr)
    return 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="sandbox.py", description=__doc__.splitlines()[0])
    action = parser.add_mutually_exclusive_group(required=True)
    action.add_argument("--image",
                        help="run SANDBOX_CMD in this image, oxedi-sandbox:<hash> or oxedi-sandbox-viewer:<hash>")
    action.add_argument("--clean", choices=("checkout", "all"),
                        help="remove this checkout's storage, or all of the sandbox storage")
    parser.add_argument("--kind", choices=[k.value for k in ImageKind],
                        help="what the --image is: the base image or the viewer one (required with --image)")
    parser.add_argument("--checkout", required=True, type=Path, help="the checkout to mount at /work")
    args = parser.parse_args(argv)
    if args.image is not None and args.kind is None:
        parser.error("--image needs --kind base or --kind viewer")
    for signum in STOP_SIGNALS:
        signal.signal(signum, stop)
    try:
        if args.clean is not None:
            return clean_storage(args.clean == "all", args.checkout.resolve(), dict(os.environ))
        return sandbox(args.image, ImageKind(args.kind), args.checkout.resolve(), dict(os.environ))
    except SandboxError as error:
        print(f"sandbox: {error}", file=sys.stderr)
        return 2
    except Stopped as stopped:
        print(f"sandbox: interrupted by {stopped}; the container, if it had started, was killed", file=sys.stderr)
        return 128 + stopped.signum


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
