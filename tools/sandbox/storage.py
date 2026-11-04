"""The sandbox's storage on the host: shared caches and each checkout's target/, venvs, the
empty file bound over every ignored file, the stand-ins bound over the absent root names and the
record of the last dirty audit."""

import os
import shutil
import sys
from dataclasses import dataclass
from pathlib import Path

from layout import STAND_INS, Checkout, SandboxError, path_hash, refuse_links, refuse_unsafe


@dataclass(frozen=True)
class Storage:
    """The host directories mounted into the container."""

    cache: Path
    cargo: Path
    uv: Path
    target: Path
    venv: Path
    # The viewer image's docs venv, mounted at .venv-docs by its runs only.
    docs_venv: Path
    # The viewer image's built module, mounted at viewer/pkg by its runs only.
    viewer_pkg: Path
    # An empty read-only file, bound over every file git ignores so a run cannot read it.
    empty: Path
    # One read-only file per name of `layout.STAND_INS`, bound over that root name when it is absent.
    stand_ins: Path
    # The paths the last dirty audit named, with what each was then; never mounted.
    record: Path
    # The root names the run phase holds, written before they are made; never mounted.
    held: Path

    @property
    def advisory(self) -> Path:
        """The advisory database the audit tools fetch, shared like the cargo registry."""
        return self.cache / "advisory"


def cache_root(env: dict[str, str]) -> Path:
    """${XDG_CACHE_HOME:-$HOME/.cache}/oxedi-sandbox; a relative XDG_CACHE_HOME is ignored, as the
    XDG specification says."""
    xdg = env.get("XDG_CACHE_HOME", "")
    base = Path(xdg) if os.path.isabs(xdg) else Path(env.get("HOME", str(Path.home()))) / ".cache"
    return base / "oxedi-sandbox"


def own_dir(cache: Path, root: Path) -> Path:
    """The storage of the checkout at `root`."""
    return cache / path_hash(root)


def record_for(cache: Path, root: Path) -> Path:
    """Where the last dirty audit of the checkout at `root` is recorded."""
    return own_dir(cache, root) / "audit.json"


def held_for(cache: Path, root: Path) -> Path:
    """Where the names the run phase holds in the checkout at `root` are listed while it runs."""
    return own_dir(cache, root) / "held.json"


def storage_for(cache: Path, checkout: Checkout) -> Storage:
    refuse_unsafe(cache, "storage")
    own = own_dir(cache, checkout.root)
    return Storage(
        cache, cache / "cargo", cache / "uv", own / "target", own / "venv", own / "venv-docs",
        own / "viewer-pkg", own / "empty", own / "stand-ins", record_for(cache, checkout.root),
        held_for(cache, checkout.root),
    )


def reviews(cache: Path) -> Path:
    """Where sandbox-pr keeps one directory per change and tested commit: its clone and reports."""
    return cache / "prs"


def forget(cache: Path, review: Path) -> None:
    """Removes a review directory and the storage of the clone it holds, so nothing of an earlier
    run of the same change (a target/ above all) reaches the next one."""
    clean(cache / path_hash(review / "checkout"))
    clean(review)


def make_mount_point(root: Path, mount_point: str) -> None:
    """Makes a mount point of the checkout, with its parents. A link anywhere on its path is refused:
    the host would follow it and make the directory outside the checkout."""
    refuse_links(root, mount_point, f"the mount point {mount_point} must be a real directory of the checkout")
    try:
        (root / mount_point).mkdir(parents=True, exist_ok=True)
    except OSError as error:
        raise SandboxError(f"cannot make the mount point {mount_point} in {root}: {error}") from error


def prepare(storage: Storage, checkout: Checkout, mount_points: tuple[str, ...]) -> None:
    """Creates the storage and the checkout's `mount_points` as the user, so docker never creates
    them as root."""
    for directory in (storage.cargo, storage.uv, storage.target, storage.venv, storage.docs_venv, storage.viewer_pkg):
        directory.mkdir(mode=0o700, parents=True, exist_ok=True)
    for mount_point in mount_points:
        make_mount_point(checkout.root, mount_point)
    # Made anew each time, so they hold what the run must read whatever happened to them on the host.
    for made in (storage.empty, storage.stand_ins):
        if os.path.lexists(made):
            remove(made)
    storage.empty.touch(mode=0o444)
    storage.stand_ins.mkdir(mode=0o700)
    for name, body in STAND_INS.items():
        (storage.stand_ins / name).write_text(body)
        (storage.stand_ins / name).chmod(0o444)


def is_tree(path: Path) -> bool:
    """A real directory: a link to one is not a tree to descend."""
    return not path.is_symlink() and path.is_dir()


def remove(path: Path) -> None:
    """Removes a file, a link or a whole tree, with an explicit stack, so a tree of any depth
    goes. A directory gets its owner's permissions back before it is listed, so one a run made
    unreadable still goes; links are removed, never followed."""
    if not is_tree(path):
        path.unlink()
        return
    pending, emptied = [path], []
    while pending:
        directory = pending.pop()
        directory.chmod(0o700)
        emptied.append(directory)
        for entry in directory.iterdir():
            if is_tree(entry):
                pending.append(entry)
            else:
                entry.unlink()
    # Every directory was listed after its parent, so the reverse order removes children first.
    for directory in reversed(emptied):
        directory.rmdir()


def empty_dir(directory: Path) -> None:
    """Removes what a directory holds and keeps the directory (a mount point)."""
    try:
        directory.chmod(0o700)
        for entry in directory.iterdir():
            remove(entry)
    except OSError as error:
        raise SandboxError(
            f"cannot empty {directory} before the fetch: {error}; remove that directory by hand"
        ) from error


def clean(path: Path) -> bool:
    """Removes `path` and everything under it, with shutil; links inside are removed, never
    followed. False when there was nothing to remove."""
    if not os.path.lexists(path):
        return False

    def make_writable(_function, failed: str, _error) -> None:
        """Gives the failed path's parent inside `path` its owner's permissions back (a run may
        have removed them), then removes the failed path with `remove`, which does the same for
        every directory under it. Nothing outside `path` is changed."""
        if failed != str(path):
            os.chmod(os.path.dirname(failed), 0o700)
        if os.path.lexists(failed):
            remove(Path(failed))

    handler = {"onexc": make_writable} if sys.version_info >= (3, 12) else {"onerror": make_writable}
    try:
        shutil.rmtree(path, **handler)
    except OSError as error:
        raise SandboxError(f"cannot remove {path}: {error}; remove it by hand") from error
    return True
