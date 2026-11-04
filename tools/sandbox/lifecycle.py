"""The Docker daemon and one container's life: its checks, the limits, the run, the kill and the
line that explains how it ended."""

import json
import os
import shutil
import subprocess
import sys
import threading
from dataclasses import dataclass

from arguments import Limits
from layout import SandboxError

DEFAULT_TIMEOUT_S = 3600
KILL_GRACE_S = 10
STDERR_TAIL_BYTES = 4096



@dataclass(frozen=True)
class DaemonInfo:
    rootless: bool
    runtimes: tuple[str, ...]
    cpus: int
    memory_bytes: int


def docker(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["docker", *args], capture_output=True, text=True, check=False)


def daemon_info() -> DaemonInfo:
    if shutil.which("docker") is None:
        raise SandboxError("docker was not found on PATH; install Docker Engine (Linux) or Docker Desktop")
    result = docker("info", "--format", "{{json .}}")
    if result.returncode != 0:
        raise SandboxError(f"docker info failed; is the Docker daemon running? {result.stderr.strip()}")
    try:
        info = json.loads(result.stdout)
        return DaemonInfo(
            rootless=any("name=rootless" in option for option in info.get("SecurityOptions") or []),
            runtimes=tuple(info.get("Runtimes") or {}),
            cpus=int(info["NCPU"]),
            memory_bytes=int(info["MemTotal"]),
        )
    except (ValueError, KeyError, TypeError) as error:
        unreadable = f"docker info printed what the sandbox cannot read ({error!r}): {result.stdout[:200]}"
        raise SandboxError(unreadable) from error


def positive_knob(env: dict[str, str], name: str, unit: str, default: int) -> int:
    text = env.get(name, "")
    if not text:
        return default
    if not (text.isascii() and text.isdigit()) or int(text) < 1:
        raise SandboxError(f'{name} must be a positive whole number of {unit}, got "{text}"')
    return int(text)


def limits_from(env: dict[str, str], info: DaemonInfo) -> Limits:
    return Limits(
        cpus=positive_knob(env, "SANDBOX_CPUS", "CPUs", max(1, info.cpus - 1)),
        memory_gb=positive_knob(env, "SANDBOX_MEMORY", "GB", max(1, info.memory_bytes * 6 // (10 << 30))),
        timeout_s=positive_knob(env, "SANDBOX_TIMEOUT", "seconds", DEFAULT_TIMEOUT_S),
    )


def image_labels(image: str) -> dict[str, str]:
    result = docker("image", "inspect", "--format", "{{json .Config.Labels}}", image)
    if result.returncode != 0:
        raise SandboxError(f"image {image} not found; build it with make sandbox-image")
    try:
        labels = json.loads(result.stdout)
    except ValueError as error:
        unreadable = f"docker image inspect printed labels the sandbox cannot read: {result.stdout[:200]}"
        raise SandboxError(unreadable) from error
    return labels if isinstance(labels, dict) else {}


def running(pattern: str) -> list[str]:
    """The running containers whose name matches the anchored regular expression `pattern`."""
    result = docker("ps", "--filter", f"name={pattern}", "--format", "{{.Names}}")
    if result.returncode != 0:
        raise SandboxError(f"docker ps failed, so whether {pattern} runs is unknown: {result.stderr.strip()}")
    return result.stdout.split()


def refuse_if_running(name: str) -> None:
    if name in running(f"^{name}$"):
        raise SandboxError(
            f"{name} is already running for this checkout; wait for it or stop it with docker kill {name}"
        )


def kill(name: str) -> None:
    """Kills the container; one that already ended (--rm removed it) is not an error."""
    result = docker("kill", name)
    if result.returncode != 0 and "No such container" not in result.stderr and "is not running" not in result.stderr:
        print(f"sandbox: docker kill {name} failed; stop it by hand: {result.stderr.strip()}", file=sys.stderr)


class Tee(threading.Thread):
    """Reads a container's stream as it comes: copies it to our stderr when `echo`, keeps all of it
    when `keep`, and always keeps its last bytes, which name the cause when docker itself fails."""

    def __init__(self, stream, echo: bool, keep: bool):
        super().__init__(daemon=True)
        self.stream = stream
        self.echo = echo
        self.keep = keep
        self.tail = b""
        self.whole = bytearray()

    def run(self) -> None:
        for chunk in iter(lambda: os.read(self.stream.fileno(), 65536), b""):
            if self.echo:
                sys.stderr.buffer.write(chunk)
                sys.stderr.buffer.flush()
            if self.keep:
                self.whole += chunk
            self.tail = (self.tail + chunk)[-STDERR_TAIL_BYTES:]

    def last_line(self) -> str:
        lines = self.tail.decode(errors="replace").strip().splitlines()
        return lines[-1] if lines else "docker printed no reason"

    def text(self) -> str:
        return self.whole.decode(errors="replace")


@dataclass(frozen=True)
class Outcome:
    status: int
    cause: str
    timed_out: bool
    # What the container printed, stdout then stderr, when the run captured it; empty otherwise.
    output: str = ""


def run_container(argv: list[str], name: str, timeout_s: float, capture: bool = False) -> Outcome:
    """Runs `docker run ...` and waits at most `timeout_s`. Unless it ended by itself (timeout,
    Ctrl-C, any error), the container is killed before this returns or raises. With `capture`,
    stdout and stderr are kept for the outcome instead of being copied to ours."""
    process = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stderr=subprocess.PIPE,
                               stdout=subprocess.PIPE if capture else None)
    readers = [Tee(process.stderr, echo=not capture, keep=capture)]
    if capture:
        readers.insert(0, Tee(process.stdout, echo=False, keep=True))
    for reader in readers:
        reader.start()
    ended = False
    try:
        status = process.wait(timeout=timeout_s)
        ended = True
    except subprocess.TimeoutExpired:
        return Outcome(124, "", True)
    finally:
        if not ended:
            kill(name)
            try:
                process.wait(timeout=KILL_GRACE_S)
            except subprocess.TimeoutExpired:
                process.kill()
        for reader in readers:
            reader.join(timeout=KILL_GRACE_S)
    output = "".join(reader.text() for reader in readers) if capture else ""
    return Outcome(status, readers[-1].last_line(), False, output)


def explain(outcome: Outcome, limits: Limits, budget: str, failure: str) -> str:
    """The line for a container that did not exit 0. `budget` names the time limit and `failure`
    says what failed when neither docker nor the memory limit is the cause."""
    if outcome.timed_out:
        return f"{budget}; the container was killed"
    if outcome.status == 125:
        return f"docker could not start the container, or the command itself exited 125: {outcome.cause}"
    if outcome.status == 137:
        return (f"the container was killed (exit 137), most likely by its {limits.memory_gb} GB memory limit; "
                "raise it with SANDBOX_MEMORY=<GB>")
    return failure
