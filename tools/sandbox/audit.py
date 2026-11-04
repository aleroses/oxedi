"""The audit after a run: what the run changed or left in the checkout, and the removal of the
caches and the built extension it may leave there.

Before the first container, `snapshot` records the untracked and ignored paths git lists and a
fingerprint of every tracked and untracked file. After the last one, `audit` compares, removes the
outputs and returns the findings. A changed file or a new path outside those outputs makes the
sandbox exit 3, whatever the command's status: the host would otherwise run or commit what the
run wrote. The audit fails closed: so does a path it cannot read, a warning git prints while
listing the checkout, and an output it cannot remove.

A dirty audit is recorded in the checkout's storage, which no run can write: `leftovers` names
the paths still as the run left them, and the next start refuses while there is one, because the
host would otherwise run it (a planted `hashlib.py` or `GNUmakefile`) and the next snapshot would
take it for the baseline.
"""

import hashlib
import json
import os
import stat
import sys
from collections.abc import Iterator
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path, PurePosixPath

from layout import (
    OUTPUT_FILES, OUTPUT_NAMES, OUTPUT_TOP, Checkout, SandboxError, git_records, git_run, is_output, refuse_unsafe,
)
from storage import remove

DIRTY_STATUS = 3
# Untracked files one by one; an ignored directory as one entry, not its contents; submodules not
# entered, so git never reads a configuration the run could have written in one.
STATUS = ("status", "--porcelain=v1", "--untracked-files=all", "--ignored=matching", "--ignore-submodules=all")
# Names host tools look up in a directory without being told: make reads GNUmakefile and makefile
# before Makefile, Python imports sitecustomize and usercustomize, pytest a conftest.py, direnv an
# .envrc, cargo a .cargo/ and rustup a rust-toolchain of the directory or any parent. A *.py at
# the root shadows a module for a Python started there.
LOOKED_UP = ("GNUmakefile", "makefile", "sitecustomize.py", "usercustomize.py", "conftest.py", ".envrc", ".cargo",
             "rust-toolchain*")
UNREADABLE = "unreadable"


@dataclass(frozen=True)
class Snapshot:
    """The checkout as git sees it (paths relative, `/`-separated): a fingerprint per tracked and
    per untracked file, the untracked and ignored paths, the paths that could not be read (with
    the reason) and what git warned about while listing them."""

    tracked: dict[str, str]
    untracked: dict[str, str]
    others: frozenset[str]
    unreadable: dict[str, str]
    warnings: tuple[str, ...]


@dataclass(frozen=True)
class Findings:
    """What a run changed (tracked and untracked files), left (new paths, described), what the
    audit could not read, what it removed and why it could not remove the rest; then the new
    paths host tools look up by name, and every path the findings name (relative, as git lists
    them), which a dirty audit records."""

    changed: tuple[str, ...]
    changed_untracked: tuple[str, ...]
    new: tuple[str, ...]
    unreadable: tuple[str, ...]
    removed: tuple[str, ...]
    failures: tuple[str, ...]
    looked_up: tuple[str, ...]
    paths: tuple[str, ...]

    @property
    def dirty(self) -> bool:
        return any((self.changed, self.changed_untracked, self.new, self.unreadable, self.failures))

    def lines(self) -> list[str]:
        """One line per finding, in this order, after the warning about the paths host tools
        look up by name. The audit cannot tell who changed a file, the run or the host."""
        warning = [
            f"do not run make or any tool in this checkout until you remove {', '.join(self.looked_up)}: "
            "host tools load them by name (make reads GNUmakefile and makefile before Makefile)"
        ] if self.looked_up else []
        templates = (
            (self.changed, "tracked file {} changed while the sandbox ran"),
            (self.changed_untracked, "untracked file {} changed while the sandbox ran"),
            (self.new, "the run left a new path {}"),
            (self.unreadable, "unreadable: {}; the audit cannot tell what the run did there"),
            (self.failures, "cannot remove {}; remove it by hand"),
        )
        return warning + [template.format(item) for items, template in templates for item in items]


def digest(path: Path) -> str:
    with open(path, "rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def fingerprint(root: Path, path: str) -> str:
    """What a path is now: missing, a file's executable bits and SHA-256, a link's target or
    another type. A path reached through a linked directory is not read. Raises OSError when the
    path cannot be read."""
    full = root / path
    if os.path.realpath(full.parent) != str(full.parent):
        return "behind a symbolic link"
    try:
        info = os.lstat(full)
    except (FileNotFoundError, NotADirectoryError):
        return "missing"
    if stat.S_ISREG(info.st_mode):
        return f"file {info.st_mode & 0o111:o} {digest(full)}"
    if stat.S_ISLNK(info.st_mode):
        return f"link {os.readlink(full)}"
    return f"type {stat.S_IFMT(info.st_mode):o}"


def fingerprints(root: Path, paths: list[str], unreadable: dict[str, str]) -> dict[str, str]:
    """The fingerprint of each path; one that cannot be read goes to `unreadable` with the reason."""
    found = {}
    for path in paths:
        try:
            found[path] = fingerprint(root, path)
        except OSError as error:
            unreadable[path] = error.strerror or str(error)
    return found


def status(root: Path) -> tuple[list[str], frozenset[str], tuple[str, ...]]:
    """The untracked (`??`) paths, those and the ignored (`!!`) paths of `git status`, and what git
    printed on stderr (a directory it could not open, for one)."""
    records, warnings = git_run(root, *STATUS)
    untracked, found = [], set()
    entries = iter(records)
    for record in entries:
        code, path = record[:2], record[3:]
        if code[0] in "RC":
            next(entries, None)  # a rename or copy carries its old path as the next record
        if code in ("??", "!!"):
            found.add(path.rstrip("/"))
        if code == "??":
            untracked.append(path)
    return untracked, frozenset(found), tuple(warnings)


def snapshot(root: Path) -> Snapshot:
    untracked, others, warnings = status(root)
    unreadable: dict[str, str] = {}
    tracked = fingerprints(root, git_records(root, "ls-files"), unreadable)
    return Snapshot(tracked, fingerprints(root, untracked, unreadable), others, unreadable, warnings)


def changes(before: dict[str, str], after: dict[str, str], unreadable: dict[str, str]) -> tuple[str, ...]:
    """The paths whose fingerprint differs; an unreadable one is reported as such instead."""
    readable = (before.keys() | after.keys()) - unreadable.keys()
    return tuple(sorted(p for p in readable if before.get(p) != after.get(p)))


def describe_new(root: Path, path: str) -> str:
    """The path, and where it points when it is a symbolic link out of the checkout."""
    full = root / path
    if full.is_symlink() and not Path(os.path.realpath(full)).is_relative_to(root):
        return f"{path}, a symbolic link to {os.readlink(full)} outside the checkout"
    return path


def is_scrubbed(path: str) -> bool:
    return PurePosixPath(path).name in OUTPUT_NAMES or fnmatch(path, OUTPUT_FILES)


def is_looked_up(path: str) -> bool:
    parts = PurePosixPath(path).parts
    named = any(fnmatch(part, name) for part in parts for name in LOOKED_UP)
    return named or (len(parts) == 1 and path.endswith(".py"))


def where(root: Path, error: OSError) -> str:
    """The path an error names, relative to the checkout when it is inside it."""
    failed = Path(error.filename or root)
    return failed.relative_to(root).as_posix() if failed.is_relative_to(root) else str(failed)


def reason(root: Path, error: OSError) -> str:
    """`path (why)`, the path relative to the checkout when it is inside it."""
    return f"{where(root, error)} ({error.strerror or error})"


@dataclass(frozen=True)
class Scrubbed:
    """What the scrub removed, the directories it could not list and the outputs it could not
    remove (`path (why)`), and the paths of those two."""

    removed: tuple[str, ...]
    unlisted: tuple[str, ...]
    failures: tuple[str, ...]
    blocked: tuple[str, ...]


def outputs(root: Path, skipped: set[str], unlisted: list[OSError]) -> Iterator[str]:
    """Every Python cache and built extension under `root`, found without walking into a skipped
    path (what the run could not write, or its own storage) or into an output. The error of a
    directory that cannot be listed goes to `unlisted`, and the walk goes on."""
    for directory, subdirs, files in os.walk(root, onerror=unlisted.append):
        base = Path(directory).relative_to(root)
        doomed = [(base / name).as_posix() for name in subdirs + files if is_scrubbed((base / name).as_posix())]
        blocked = skipped.union(doomed)
        subdirs[:] = [name for name in subdirs if (base / name).as_posix() not in blocked]
        yield from doomed


def scrub(root: Path, skipped: set[str]) -> Scrubbed:
    """Removes the outputs; one that cannot be removed is recorded, and the scrub goes on."""
    removed, unlisted, failures, failed = [], [], [], []
    for path in outputs(root, skipped, unlisted):
        try:
            remove(root / path)
            removed.append(path)
        except OSError as error:
            failures.append(reason(root, error))
            failed.append(path)
    blocked = [where(root, error) for error in unlisted] + failed
    return Scrubbed(tuple(sorted(removed)), tuple(reason(root, e) for e in unlisted), tuple(failures), tuple(blocked))


def audit(before: Snapshot, checkout: Checkout) -> Findings:
    """Compares the checkout with `before` and removes the outputs the run may have left."""
    root = checkout.root
    after = snapshot(root)
    left = sorted(p for p in after.others - before.others if not is_output(p))
    # The run could not write what is read-only or masked; .git is read-only too.
    scrubbed = scrub(root, set(OUTPUT_TOP) | {".git"} | set(checkout.read_only) | set(checkout.masked))
    unreadable = [f"{path} ({why})" for path, why in sorted(after.unreadable.items())]
    # A new untracked file is a new path; only those that were there before can have changed.
    kept = {path: print_ for path, print_ in after.untracked.items() if path in before.untracked}
    unreadable += [f"git status: {warning}" for warning in after.warnings] + list(scrubbed.unlisted)
    changed = changes(before.tracked, after.tracked, after.unreadable)
    changed_untracked = changes(before.untracked, kept, after.unreadable)
    named = {*changed, *changed_untracked, *left, *after.unreadable, *scrubbed.blocked}
    return Findings(
        changed, changed_untracked, tuple(describe_new(root, p) for p in left), tuple(unreadable),
        scrubbed.removed, scrubbed.failures, tuple(filter(is_looked_up, left)), tuple(sorted(named)),
    )


def report(findings: Findings) -> None:
    lines = findings.lines()
    if findings.removed:
        lines.append(f"removed the caches and the built extension the run may have left: {', '.join(findings.removed)}")
    if findings.dirty:
        changed = len(findings.changed) + len(findings.changed_untracked)
        lines.append(
            f"exit {DIRTY_STATUS}: the run changed the checkout (changed files: {changed}, new paths: "
            f"{len(findings.new)}, unreadable: {len(findings.unreadable)}, not removed: {len(findings.failures)}); "
            "review them with git status and git diff before running anything from this checkout; "
            "the next sandbox run refuses to start until each is removed, reverted or added with git add"
        )
    for line in lines:
        print(f"sandbox: {line}", file=sys.stderr)


def current(root: Path, paths: list[str]) -> dict[str, str]:
    """The fingerprint of each path, `unreadable` for one that cannot be read."""
    unreadable: dict[str, str] = {}
    found = fingerprints(root, paths, unreadable)
    return {path: found.get(path, UNREADABLE) for path in paths}


def record(findings: Findings, root: Path, path: Path) -> None:
    """Writes what each path a dirty audit named is now, or removes the record after a clean one."""
    if not findings.dirty:
        path.unlink(missing_ok=True)
        return
    path.write_text(json.dumps(current(root, list(findings.paths)), indent=1, sort_keys=True) + "\n")


def recorded(path: Path) -> dict[str, str]:
    """The record of the last dirty audit; none is an empty one. A record the sandbox cannot read
    refuses the start."""
    refuse_unsafe(path, "audit record")
    if not os.path.lexists(path):
        return {}
    try:
        paths = json.loads(path.read_text())
    except (OSError, ValueError) as error:
        raise SandboxError(f"cannot read the audit record {path}: {error}; remove it once you have reviewed "
                           "the checkout with git status and git diff") from error
    if not (isinstance(paths, dict) and all(isinstance(v, str) for v in paths.values())):
        raise SandboxError(f"the audit record {path} is not a map of paths to fingerprints; remove it once you "
                           "have reviewed the checkout with git status and git diff")
    return paths


def unaccepted(root: Path, paths: list[str]) -> set[str]:
    """What git lists among `paths` as untracked, ignored or changed in the working tree: what
    nobody has added with git add."""
    records, _ = git_run(root, *STATUS, "--", *(f":(literal){path}" for path in paths))
    found, entries = set(), iter(records)
    for record in entries:
        if record[0] in "RC":
            next(entries, None)  # a rename or copy carries its old path as the next record
        # The second column is the working tree's: `?` untracked, `!` ignored, a letter a change.
        if record[1] != " ":
            found.add(record[3:].rstrip("/"))
    return found


def related(path: str, listed: set[str]) -> bool:
    """`path` is listed, or holds or sits inside a listed path."""
    return any(entry == path or entry.startswith(f"{path}/") or path.startswith(f"{entry}/") for entry in listed)


def leftovers(root: Path, record_path: Path) -> list[str]:
    """The paths of the last dirty audit still as the run left them and not added with git add.
    The record goes once there is none."""
    paths = recorded(record_path)
    same = [p for p, now in current(root, list(paths)).items() if now == paths[p]]
    listed = unaccepted(root, same) if same else set()
    left = sorted(p for p in same if related(p, listed))
    if not left:
        record_path.unlink(missing_ok=True)
    return left


def refuse_leftovers(root: Path, record_path: Path) -> None:
    left = leftovers(root, record_path)
    if left:
        raise SandboxError(
            f"refused: the last run left {', '.join(left)}, still as it left them; review them with git status "
            "and git diff, then remove or revert each one, or add it with git add to keep it "
            "(make sandbox-clean forgets the record, with the rest of this checkout's storage)"
        )
