#!/usr/bin/env python3
"""Tests someone else's change in the sandbox: a pull request fetched from GitHub, or a ref of the
host checkout, without running any of it on the host.

The flow: a throwaway clone made with git isolated from the host's configuration; a risk report
made from the diff alone (the paths that let a change run code or steer a tool, the packages it
adds or changes, and the cargo sources it adds); a confirmation when it adds a cargo source;
cargo-deny over its Cargo.lock in an audit container; a confirmation when something is flagged or
cargo-deny failed; the fetch; pip-audit over the wheels the fetch installed, and a second
confirmation when it failed; then the target in the run phase, with no network, and the audit of
the clone. A text and a JSON report go next to the clone, in
${XDG_CACHE_HOME:-~/.cache}/oxedi-sandbox/prs/<id>-<sha12>/. Nothing is published.

The rules come from the checkout that runs this script, never from the change: this script and
the modules beside it, the sensitive-path list and the cargo-deny policy (deny.toml, bound
read-only into the audit container). Every git command of this process runs without the host's
global and system configuration, hooks, attributes file or file transport.

usage: sandbox_pr.py --image TAG --kind {base,viewer} --checkout DIR
The change comes from SANDBOX_PR (a pull request number; SANDBOX_HEAD=1 tests its head instead of
its merge ref) or SANDBOX_REF (a ref of the host checkout). SANDBOX_TARGET is the command (default
`make gates && make py-test`); `--kind viewer` runs it in the viewer image, with the docs venv
fetched and audited too. SANDBOX_CONFIRM=yes answers every confirmation. SANDBOX_SOURCE, for
tests, names another git checkout to clone from. The limits are those of sandbox.py.
Exit status: as sandbox.py (the command's, 2 refused, 3 dirty audit, 124 timeout, 130 and 143 on a
signal), the fetch's when the fetch failed, and 4 when a confirmation was needed and not given.
"""

import argparse
import os
import re
import signal
import subprocess
import sys
from collections.abc import MutableMapping
from dataclasses import dataclass
from pathlib import Path

# Isolated mode leaves the script's own directory out of sys.path; tools/ is read-only in a run.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from arguments import VENVS, ImageKind, Phase, deny_command, pip_audit_command  # noqa: E402
from audit import DIRTY_STATUS  # noqa: E402
from deps import dependency_diffs, new_cargo_sources  # noqa: E402
from layout import SandboxError, container_name  # noqa: E402
from lifecycle import refuse_if_running, run_container  # noqa: E402
from report import (  # noqa: E402
    Confirmed, Progress, Ran, Report, Tool, VenvAudit, deny_lines, hint_lines, pip_audit_lines, risk_lines,
    write_reports,
)
from risk import Risk, image_hint, touched  # noqa: E402
from sandbox import (  # noqa: E402
    STOP_SIGNALS, Plan, Stopped, audited, checked_daemon, fetch_phase, ready, run_phase, stop,
)
from source import GIT_ENV, GIT_OPTIONS, Change, Request, clone, isolated_git  # noqa: E402
from storage import cache_root, clean, forget, reviews  # noqa: E402

NOT_CONFIRMED_STATUS = 4
DEFAULT_TARGET = "make gates && make py-test"
AUDIT_BUDGET_S = 600
# --attr-source, which lets the diff read no attributes of the change, came with git 2.40.
GIT_MINIMUM = (2, 40)
GIT_VERSION = re.compile(r"git version (\d+)\.(\d+)")
YES = ("y", "yes")


@dataclass(frozen=True)
class Settings:
    """What the variables ask for, checked before any git command."""

    request: Request
    target: str
    confirm: bool

    @staticmethod
    def parse(env: dict[str, str]) -> "Settings":
        confirm = env.get("SANDBOX_CONFIRM", "")
        if confirm not in ("", "yes"):
            raise SandboxError(f'CONFIRM must be yes or unset, got "{confirm}"')
        request = Request.parse(env)
        return Settings(request, env.get("SANDBOX_TARGET", "").strip() or DEFAULT_TARGET, confirm == "yes")


def git_isolation(environ: MutableMapping[str, str]) -> None:
    """Drops the host's GIT_ variables from `environ` and sets source.py's isolation there, its
    `-c` options as GIT_CONFIG_COUNT pairs, so every git this process starts (the survey and the
    audit of the clone too) runs without the host's configuration, hooks or attributes file."""
    for name in [name for name in environ if name.startswith("GIT_")]:
        del environ[name]
    pairs = [option.partition("=") for flag, option in zip(GIT_OPTIONS[::2], GIT_OPTIONS[1::2]) if flag == "-c"]
    environ.update(GIT_ENV, GIT_CONFIG_COUNT=str(len(pairs)))
    for index, (key, _, value) in enumerate(pairs):
        environ.update({f"GIT_CONFIG_KEY_{index}": key, f"GIT_CONFIG_VALUE_{index}": value})


def require_git() -> None:
    try:
        result = subprocess.run(["git", "version"], capture_output=True, text=True, check=False)
    except FileNotFoundError as error:
        raise SandboxError("git was not found on PATH; sandbox-pr needs git 2.40 or later") from error
    found = result.stdout.strip() or result.stderr.strip()
    match = GIT_VERSION.match(found)
    if match is None:
        raise SandboxError(f'cannot read the git version from "{found}"; sandbox-pr needs git 2.40 or later')
    if (int(match[1]), int(match[2])) < GIT_MINIMUM:
        raise SandboxError(f"needs git 2.40 or later (for --attr-source), found {found}")


def top_level(path: Path) -> str:
    """The top directory of the git checkout that holds `path`; empty when none does."""
    result = isolated_git(path, "rev-parse", "--show-toplevel")
    return os.fsdecode(result.stdout).strip() if result.returncode == 0 else ""


def source_checkout(host: Path, env: dict[str, str]) -> Path:
    """The checkout the clone is made from: the host's, or SANDBOX_SOURCE when a test names another
    one, which must be the top of a git checkout."""
    named = env.get("SANDBOX_SOURCE", "")
    if not named:
        return host
    path = Path(named).resolve()
    if not (path.is_dir() and top_level(path) == str(path)):
        raise SandboxError(f'SOURCE must be the top directory of a git checkout, got "{named}"')
    return path


def fresh_clone(cache: Path, source: Path, request: Request) -> tuple[Change, Path]:
    """Clones the change into `prs/.<id>-incoming`, then, once the tested commit is known, removes
    the old `prs/<id>-<sha12>/` with its storage and moves the clone there as `checkout/`."""
    root = reviews(cache)
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    root = root.resolve()
    incoming = root / f".{request.label}-incoming"
    clean(incoming)
    change = clone(source, request, incoming)
    review = root / f"{change.label}-{change.tested[:12]}"
    refuse_if_running(container_name(review / "checkout"))
    forget(cache, review)
    review.mkdir(mode=0o700)
    incoming.rename(review / "checkout")
    return change, review / "checkout"


def assess(change: Change, root: Path) -> Risk:
    paths = touched(change, root)
    names = [t.path for t in paths]
    cargo, python = dependency_diffs(change, root, names)
    return Risk(tuple(paths), cargo, python, new_cargo_sources(change, root, names))


def audit_tool(plan: Plan, command: list[str]) -> Tool:
    outcome = run_container(plan.argv(Phase.AUDIT, command), plan.checkout.name, AUDIT_BUDGET_S, capture=True)
    output = outcome.output if not outcome.timed_out else f"ran past its {AUDIT_BUDGET_S} s budget and was killed"
    return Tool(outcome.status, output)


def pip_audits(plan: Plan) -> tuple[VenvAudit, ...]:
    """pip-audit over each venv of the image, each in its own audit container."""
    audits = []
    for venv in VENVS[plan.kind]:
        tool = audit_tool(plan, pip_audit_command(venv))
        audits.append(VenvAudit(venv, tool.status, tool.output))
    return tuple(audits)


def confirm(needed: bool, why: str, settings: Settings) -> Confirmed:
    """Asks on the terminal when a confirmation is needed and CONFIRM=yes did not give it; without
    a terminal the answer is no."""
    if not needed:
        return Confirmed.NOT_NEEDED
    if settings.confirm:
        return Confirmed.ENV
    if not sys.stdin.isatty():
        print(f"sandbox-pr: {why}; there is no terminal to ask, so it stops with exit {NOT_CONFIRMED_STATUS}: "
              "read the report, then set CONFIRM=yes to go on", file=sys.stderr)
        return Confirmed.REFUSED
    print(f"sandbox-pr: {why}. Go on? [y/N] ", end="", file=sys.stderr, flush=True)
    answer = sys.stdin.readline().strip()
    if answer.lower() in YES:
        return Confirmed.TTY
    print(f'sandbox-pr: not confirmed (answer "{answer}"), so it stops with exit {NOT_CONFIRMED_STATUS}',
          file=sys.stderr)
    return Confirmed.REFUSED


def first_gate(risk: Risk, deny: Tool) -> tuple[bool, str]:
    """Whether the fetch needs a confirmation, and why."""
    found = (("the change touches what the report flags", risk.flagged), ("cargo-deny failed", deny.status != 0))
    reasons = [reason for reason, applies in found if applies]
    return bool(reasons), " and ".join(reasons)


def source_gate(risk: Risk, settings: Settings) -> Confirmed:
    """Asks before cargo-deny, which resolves the change's manifests, reaches a source the change
    adds; each source on its own line first."""
    for url in risk.new_sources:
        print(f"sandbox-pr: the change adds a cargo source: {url}", file=sys.stderr)
    return confirm(bool(risk.new_sources), "cargo-deny would fetch from a source the change adds", settings)


def stages(plan: Plan, risk: Risk, settings: Settings, risk_text: list[str]) -> Progress:
    """The containers in order, each confirmation before what it guards: the new-source
    confirmation, cargo-deny, the first confirmation, the fetch, pip-audit, the second
    confirmation, the run. Each part of the report is printed before the confirmation that rests
    on it."""
    print("\n".join(risk_text), flush=True)
    answers = (source_gate(risk, settings),)
    if Confirmed.REFUSED in answers:
        return Progress(NOT_CONFIRMED_STATUS, None, (), answers, False)
    plan.storage.advisory.mkdir(mode=0o700, parents=True, exist_ok=True)
    deny = audit_tool(plan, deny_command())
    print("\n".join(deny_lines(deny)), flush=True)
    answers += (confirm(*first_gate(risk, deny), settings),)
    if Confirmed.REFUSED in answers:
        return Progress(NOT_CONFIRMED_STATUS, deny, (), answers, False)
    fetched = fetch_phase(plan)
    if fetched != 0:
        return Progress(fetched, deny, (), answers, False)
    pip = pip_audits(plan)
    print("\n".join(pip_audit_lines(pip)), flush=True)
    answers += (confirm(any(audit.status != 0 for audit in pip), "pip-audit failed", settings),)
    if Confirmed.REFUSED in answers:
        return Progress(NOT_CONFIRMED_STATUS, deny, pip, answers, False)
    return Progress(run_phase(plan, settings.target), deny, pip, answers, True)


def review_change(image: str, kind: ImageKind, host: Path, env: dict[str, str]) -> int:
    settings = Settings.parse(env)
    require_git()
    source = source_checkout(host, env)
    info = checked_daemon()
    change, root = fresh_clone(cache_root(env), source, settings.request)
    plan = ready(image, kind, root, env, info)
    risk = assess(change, root)
    hint = image_hint(list(risk.touched), kind)
    # The terminal's risk report ends with the hint, so it shows before the first confirmation.
    risk_text = risk_lines(settings.request, change, risk) + hint_lines(hint)
    outcome, findings = audited(plan, lambda: stages(plan, risk, settings, risk_text))
    run = Ran(settings.target, outcome.status, tuple(findings.lines())) if outcome.ran else None
    write_reports(root.parent, Report(settings.request, change, risk, outcome, run, image, plan.runtime.value, hint))
    return DIRTY_STATUS if findings.dirty else outcome.status


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="sandbox_pr.py", description=__doc__.splitlines()[0])
    parser.add_argument("--image", required=True,
                        help="the sandbox image, oxedi-sandbox:<hash> or oxedi-sandbox-viewer:<hash>")
    parser.add_argument("--kind", required=True, choices=[k.value for k in ImageKind],
                        help="what the --image is: the base image or the viewer one")
    parser.add_argument("--checkout", required=True, type=Path, help="the host checkout whose rules apply")
    args = parser.parse_args(argv)
    for signum in STOP_SIGNALS:
        signal.signal(signum, stop)
    git_isolation(os.environ)
    try:
        return review_change(args.image, ImageKind(args.kind), args.checkout.resolve(), dict(os.environ))
    except SandboxError as error:
        print(f"sandbox-pr: {error}", file=sys.stderr)
        return 2
    except Stopped as stopped:
        print(f"sandbox-pr: interrupted by {stopped}; the container, if it had started, was killed", file=sys.stderr)
        return 128 + stopped.signum


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
