"""The root names the run phase holds: host files for the read-only binds over absent names.

Docker binds the empty file of the storage over each absent root name host tools load
(`layout.ROOT_LOADED`), and would create the missing mount point in the checkout as root. So the
runner creates each one first, as the user, with a fixed body that explains itself, and removes
it once the run is over. The container never sees that body, only the empty file bound over it.

A runner killed before it removes them (SIGKILL, an out-of-memory kill, a power loss) leaves them
behind: a `GNUmakefile` or `makefile` stops every host make with its own line, and the list of
held names, written to the checkout's storage before any is created, lets the next start remove
those still holding their body, or refuse when one was changed.
"""

import json
import os
import stat
import sys
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path

from layout import SandboxError

MAKE_BODY = (
    "$(error sandbox: a killed sandbox run left this placeholder {name}; remove it, and any GNUmakefile, "
    "makefile or conftest.py at the root that holds only such a line, then retry)\n"
)
# What the host finds in each held name until the run is over.
BODIES = {
    "GNUmakefile": MAKE_BODY.format(name="GNUmakefile"),
    "makefile": MAKE_BODY.format(name="makefile"),
    "conftest.py": (
        "# sandbox: a killed sandbox run left this placeholder conftest.py; remove it, and any GNUmakefile, "
        "makefile or conftest.py at the root that holds only such a line, then retry\n"
    ),
}


CLEAR_BY_HAND = ("remove each root GNUmakefile, makefile or conftest.py that holds only a placeholder line, "
                 "then that list")


def holds_body(path: Path, name: str) -> bool:
    """`path` is a regular file holding exactly the placeholder body of `name`. Raises OSError when
    it cannot be read."""
    info = os.lstat(path)
    return stat.S_ISREG(info.st_mode) and path.read_bytes() == BODIES[name].encode()


def make(root: Path, name: str) -> None:
    path = root / name
    try:
        with open(path, "x", encoding="utf-8") as file:
            file.write(BODIES[name])
        path.chmod(0o444)
    except OSError as error:
        raise SandboxError(f"cannot make the placeholder {name} the run binds over in {root}: {error}; "
                           "if it appeared since the sandbox started, review it and retry") from error


def unhold(root: Path, name: str) -> str:
    """Removes the placeholder `name` when it still holds its body; `path (why)` when it cannot,
    else an empty string. One that changed is left for the audit to report."""
    path = root / name
    try:
        if holds_body(path, name):
            path.unlink()
    except FileNotFoundError:
        return ""
    except OSError as error:
        return f"{path} ({error.strerror or error})"
    return ""


@contextmanager
def placeholders(root: Path, held: Path, names: tuple[str, ...]) -> Iterator[None]:
    """Lists `names` in `held`, makes each in the checkout for the run phase to bind over, and
    removes each again once the run is over, then the list."""
    if not names:
        yield
        return
    held.write_text(json.dumps(list(names)) + "\n")
    made = []
    try:
        for name in names:
            make(root, name)
            made.append(name)
        yield
    finally:
        failures = [failure for failure in (unhold(root, name) for name in made) if failure]
        if failures:
            raise SandboxError(f"cannot remove {', '.join(failures)}, made for the run; remove it by hand")
        held.unlink(missing_ok=True)


def listed(held: Path) -> list[str]:
    """The names a run listed in `held`; a list the sandbox cannot read refuses the start."""
    try:
        names = json.loads(held.read_text())
    except (OSError, ValueError) as error:
        raise SandboxError(f"cannot read {held}, the names a killed run held: {error}; {CLEAR_BY_HAND}") from error
    if not (isinstance(names, list) and all(name in BODIES for name in names)):
        raise SandboxError(f"{held} is not a list of held names; {CLEAR_BY_HAND}")
    return names


def changed(root: Path, names: list[str]) -> list[str]:
    """The names that exist but no longer hold their body."""
    found = []
    for name in names:
        try:
            if not holds_body(root / name, name):
                found.append(name)
        except FileNotFoundError:
            continue
        except OSError as error:
            found.append(f"{name} ({error.strerror or error})")
    return found


def clear_leftovers(root: Path, held: Path) -> None:
    """After a killed run: removes each listed name that still holds its body, with one line, or
    refuses when one was changed since."""
    if not os.path.lexists(held):
        return
    names = listed(held)
    edited = changed(root, names)
    if edited:
        raise SandboxError(
            f"refused: a killed sandbox run left placeholders in the checkout and {', '.join(edited)} changed "
            f"since; review it, then remove it, or keep it and remove {held}"
        )
    present = [name for name in names if os.path.lexists(root / name)]
    try:
        for name in present:
            (root / name).unlink()
        held.unlink()
    except OSError as error:
        raise SandboxError(f"cannot remove a placeholder a killed sandbox run left: {error}; "
                           f"{CLEAR_BY_HAND}") from error
    print(f"sandbox: removed {', '.join(present) or 'nothing'} of the placeholders "
          f"({', '.join(names)}) a killed sandbox run left in the checkout", file=sys.stderr)
