"""One sandbox run per checkout at a time: an exclusive lock on `run.lock` in the checkout's storage.

The lock is `flock`, taken without waiting, so a second run is refused at once, and a runner
killed by any signal releases it with its process: the kernel drops the lock when the last
descriptor of the open file closes. The descriptor is not inherited by the containers' `docker`
clients, so one that outlives a killed runner does not keep the lock. While held, the file
names the runner's pid and start time, which the refusal of a second run prints. Other
checkouts, other worktrees of the same repository included, have their own storage and lock.
"""

import fcntl
import json
import os
from collections.abc import Callable
from contextlib import suppress
from datetime import datetime
from pathlib import Path

from layout import SandboxError

LOCK_NAME = "run.lock"
# A lock taken on a file another process removed at that moment guards nothing: the next start
# makes a new file. That many tries are made before the start is refused.
ATTEMPTS = 3
# What a refused run says about the lock it met, and what to do: a run of a checkout can move to
# another worktree; a review of a change and commit has one lock whatever worktree asks for it.
WHOSE_RUN = "another sandbox run of this checkout"
RUN_ADVICE = "wait for it to end, or use another worktree"


def lock_path(storage: Path) -> Path:
    return storage / LOCK_NAME


def holder(path: Path) -> str:
    """`pid <n>, since <time>` as the holder of the lock at `path` wrote it."""
    try:
        record = json.loads(path.read_text())
        return f"pid {int(record['pid'])}, since {record['since']}"
    except (OSError, ValueError, KeyError, TypeError) as error:
        return f"its pid and start time unreadable in {path}: {error}"


class Busy(SandboxError):
    """The lock is held by another run, whose pid and start time `holder` names."""

    def __init__(self, holder_text: str, whose: str = WHOSE_RUN, advice: str = RUN_ADVICE):
        super().__init__(f"{whose} is in progress ({holder_text}); {advice}")
        self.holder = holder_text


def same_file(descriptor: int, path: Path) -> bool:
    """The descriptor still refers to the file at `path`, not to one removed since it was opened."""
    try:
        return os.path.samestat(os.fstat(descriptor), os.stat(path))
    except FileNotFoundError:
        return False


def acquire(path: Path, create: bool, busy: Callable[[str], Busy]) -> int | None:
    """The descriptor of `path` locked by this process; None when `create` is false and the file
    does not exist. Raises `Busy` when another process holds the lock."""
    flags = os.O_RDWR | (os.O_CREAT if create else 0)
    for _ in range(ATTEMPTS):
        try:
            descriptor = os.open(path, flags, 0o600)
        except FileNotFoundError:
            if create:
                raise
            return None
        try:
            fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            os.close(descriptor)
            raise busy(holder(path)) from None
        if same_file(descriptor, path):
            return descriptor
        os.close(descriptor)
    raise SandboxError(f"{path} was removed each time the lock was taken; "
                       "retry once no make sandbox-clean is running")


def write_holder(descriptor: int) -> None:
    """Writes this process's pid and start time over the file's earlier holder."""
    since = datetime.now().astimezone().isoformat(timespec="seconds")
    body = (json.dumps({"pid": os.getpid(), "since": since}) + "\n").encode()
    os.pwrite(descriptor, body, 0)
    os.ftruncate(descriptor, len(body))


class RunLock:
    """Holds the run lock of the storage directory `storage` while the `with` block runs.

    With `create` (a run), the directory and the file are made when missing. Without it (a clean),
    a missing file means no run holds the lock, and nothing is made. `whose` and `advice` shape
    the refusal a second holder gets."""

    def __init__(self, storage: Path, create: bool = True, whose: str = WHOSE_RUN, advice: str = RUN_ADVICE):
        self.storage = storage
        self.create = create
        self.whose = whose
        self.advice = advice
        self.descriptor: int | None = None

    def __enter__(self) -> "RunLock":
        if self.create:
            self.storage.mkdir(mode=0o700, parents=True, exist_ok=True)
        path = lock_path(self.storage)
        try:
            self.descriptor = acquire(path, self.create, lambda text: Busy(text, self.whose, self.advice))
            if self.descriptor is not None:
                write_holder(self.descriptor)
        except OSError as error:
            self.release()
            raise SandboxError(f"cannot take the run lock {path}: {error}") from error
        return self

    def release(self) -> None:
        if self.descriptor is not None:
            # Emptied while still locked, so a run that meets the lock between a release and the
            # next holder's write never names the finished run.
            with suppress(OSError):
                os.ftruncate(self.descriptor, 0)
            os.close(self.descriptor)
            self.descriptor = None

    def __exit__(self, *_exc: object) -> None:
        self.release()
