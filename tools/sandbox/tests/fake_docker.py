#!/usr/bin/env python3
"""A stand-in for the `docker` CLI in the sandbox tests.

Every call is appended to $FAKE_DIR/calls.jsonl as {"argv": [...]}; a `run` also records, for
each bind mount, whether its source and its mount point existed and who owned the source at that
moment. Behaviour comes from variables:

FAKE_INFO          the JSON `docker info --format '{{json .}}'` prints
FAKE_RUNNING       what `docker ps` prints (the running container names)
FAKE_NO_IMAGE      when set, `docker image inspect` fails until a `docker build` ran
FAKE_PLAYWRIGHT_LABEL  when set, the image's oxedi.sandbox.playwright label (the viewer image's)
FAKE_WASM_BINDGEN_LABEL, FAKE_BINARYEN_LABEL  when set, the viewer image's oxedi.sandbox.wasm-bindgen
                   and oxedi.sandbox.binaryen labels
FAKE_ARCH          the server architecture `docker version` prints; unset, it fails as with no daemon
FAKE_RUN_EXIT      exit status of the run-phase container (`bash -c ...`); default 0
FAKE_RUN_STDERR    what the run-phase container writes to stderr
FAKE_RUN_SLEEP     seconds the run-phase container sleeps before it exits
FAKE_RUN_WRITES    a shell command the run-phase container runs in the checkout (/work's source)
FAKE_FETCH_EXIT    exit status of every fetch-phase container; default 0
FAKE_DENY_EXIT     exit status of the cargo-deny container; default 0
FAKE_DENY_OUTPUT   what the cargo-deny container writes to stderr
FAKE_PIP_AUDIT_EXIT    exit status of the pip-audit container; default 0
FAKE_PIP_AUDIT_OUTPUT  what the pip-audit container writes to stdout
FAKE_RUN_LEAVES    a file name the run-phase container creates in the source bound at /work/target
FAKE_WATCH         a path whose existence every `run` records as "watched"

`docker kill` ends a sleeping run-phase container, as the real one would.
"""

import json
import os
import signal
import subprocess
import sys
import time
from itertools import pairwise
from pathlib import Path

STATE = Path(os.environ["FAKE_DIR"])
DEFAULT_INFO = {
    "SecurityOptions": ["name=apparmor,profile=default", "name=seccomp,profile=builtin", "name=cgroupns"],
    "Runtimes": {"io.containerd.runc.v2": {"path": "runc"}, "runc": {"path": "runc"}},
    "NCPU": 16,
    "MemTotal": 22914514944,
}
LABELS = {"oxedi.sandbox.rust": "1.99.0", "oxedi.sandbox.python": "3.11.15 3.13.13"}
VIEWER_LABELS = {
    "FAKE_PLAYWRIGHT_LABEL": "oxedi.sandbox.playwright",
    "FAKE_WASM_BINDGEN_LABEL": "oxedi.sandbox.wasm-bindgen",
    "FAKE_BINARYEN_LABEL": "oxedi.sandbox.binaryen",
}


def record(entry: dict) -> None:
    with open(STATE / "calls.jsonl", "a", encoding="utf-8") as log:
        log.write(json.dumps(entry) + "\n")


def bind_mounts(argv: list[str]) -> list[dict]:
    found = []
    for flag, spec in pairwise(argv):
        if flag == "--mount":
            fields = dict(part.partition("=")[::2] for part in spec.split(","))
            if fields.get("type") == "bind":
                found.append(fields)
    return found


def work_source(argv: list[str]) -> Path | None:
    """The host directory bound at /work, if any."""
    sources = [Path(m["src"]) for m in bind_mounts(argv) if m["dst"] == "/work"]
    return sources[0] if sources else None


def bind_sources(argv: list[str]) -> dict:
    """Per bind source: whether it existed, its owner, its entries and, for a destination under
    /work, whether the mount point existed in the checkout."""
    work = work_source(argv)
    sources = {}
    for fields in bind_mounts(argv):
        path = Path(fields["src"])
        destination = fields["dst"]
        under_work = work is not None and destination.startswith("/work/")
        sources[fields["src"]] = {
            "exists": path.exists(),
            "uid": path.stat().st_uid if path.exists() else -1,
            "entries": sorted(os.listdir(path)) if path.is_dir() else [],
            "mount_point": os.path.lexists(work / destination[len("/work/"):]) if under_work else None,
        }
    return sources


def image_inspect(argv: list[str]) -> int:
    if os.environ.get("FAKE_NO_IMAGE") and not (STATE / "built").exists():
        print(f"Error: No such image: {argv[-1]}", file=sys.stderr)
        return 1
    template = argv[argv.index("--format") + 1] if "--format" in argv else ""
    if "Labels" in template:
        viewer = {label: os.environ.get(variable, "") for variable, label in VIEWER_LABELS.items()}
        print(json.dumps({**LABELS, **{label: value for label, value in viewer.items() if value}}))
    elif "Size" in template:
        print("1616000000")
    else:
        print("[]")
    return 0


def target_source(argv: list[str]) -> Path:
    return next(Path(m["src"]) for m in bind_mounts(argv) if m["dst"] == "/work/target")


def run(argv: list[str]) -> int:
    watch = os.environ.get("FAKE_WATCH", "")
    record({"argv": argv, "binds": bind_sources(argv), "watched": bool(watch) and os.path.lexists(watch)})
    if "cat" in argv:
        print("make\t4.3\ngcc\t13.2")
        return 0
    if "cargo-deny" in argv:
        sys.stderr.write(os.environ.get("FAKE_DENY_OUTPUT", ""))
        return int(os.environ.get("FAKE_DENY_EXIT", "0"))
    if "pip-audit" in argv:
        sys.stdout.write(os.environ.get("FAKE_PIP_AUDIT_OUTPUT", ""))
        return int(os.environ.get("FAKE_PIP_AUDIT_EXIT", "0"))
    if "bash" not in argv:
        return int(os.environ.get("FAKE_FETCH_EXIT", "0"))
    writes = os.environ.get("FAKE_RUN_WRITES", "")
    if writes:
        subprocess.run(["sh", "-c", writes], cwd=work_source(argv), check=True)
    leaves = os.environ.get("FAKE_RUN_LEAVES", "")
    if leaves:
        (target_source(argv) / leaves).write_text("left by the run\n")
    (STATE / "run.pid").write_text(str(os.getpid()))
    time.sleep(float(os.environ.get("FAKE_RUN_SLEEP", "0")))
    sys.stderr.write(os.environ.get("FAKE_RUN_STDERR", ""))
    return int(os.environ.get("FAKE_RUN_EXIT", "0"))


def kill(argv: list[str]) -> int:
    record({"argv": argv})
    pid_file = STATE / "run.pid"
    if pid_file.exists():
        try:
            os.kill(int(pid_file.read_text()), signal.SIGTERM)
        except ProcessLookupError:
            pass
    return 0


def server_version() -> int:
    arch = os.environ.get("FAKE_ARCH", "")
    if not arch:
        print("Cannot connect to the Docker daemon at unix:///var/run/docker.sock.", file=sys.stderr)
        return 1
    print(arch)
    return 0


def main(argv: list[str]) -> int:
    if argv[:1] == ["run"]:
        return run(argv)
    if argv[:1] == ["kill"]:
        return kill(argv)
    record({"argv": argv})
    if argv[:1] == ["info"]:
        print(os.environ.get("FAKE_INFO", json.dumps(DEFAULT_INFO)))
    elif argv[:2] == ["image", "inspect"]:
        return image_inspect(argv)
    elif argv[:1] == ["build"]:
        (STATE / "built").touch()
    elif argv[:1] == ["ps"]:
        print(os.environ.get("FAKE_RUNNING", ""))
    elif argv[:1] == ["version"]:
        return server_version()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
