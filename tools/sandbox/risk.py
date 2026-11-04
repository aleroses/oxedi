"""What a change touches, judged without running any of it: the paths whose kind lets a change run
code or steer a tool, and whether the change, with the packages deps.py finds, needs a
confirmation.

Every text is read through `isolated_git` and parsed as data; nothing of the clone is imported or
executed. The diff reads attributes from no tree, so the change's own `.gitattributes` cannot hide
a binary file.
"""

import dataclasses
import os
import tomllib
from collections.abc import Callable, Iterable
from dataclasses import dataclass
from enum import Enum
from fnmatch import fnmatch
from pathlib import Path, PurePosixPath

from arguments import ImageKind
from deps import CargoSources, PackageDiff, blob
from layout import HOST_EXECUTED, covered
from source import Change, git_bytes, git_text


class Kind(Enum):
    BUILD_SCRIPT = "a build script: cargo compiles and runs it before the crate"
    MANIFEST = "a manifest, lock or toolchain file: it chooses what is downloaded, built and run"
    HOST_EXECUTED = (
        "a file the host executes or trusts after a run: the checkout's copy, this change's, is bound "
        "read-only and runs inside the sandbox (only the sandbox runner, this path list and deny.toml "
        "are always the host's)"
    )
    SCRIPT = "a script the build, the tests or the tools run"
    TEST_HOOK = "a test configuration that pytest loads before any test"
    PROC_MACRO = "a procedural macro crate: the compiler runs it on every crate that uses it"
    RUN_DOCS = "documentation whose code blocks make docs-test runs"
    BINARY = "a binary file, which the diff cannot show"
    SYMLINK = "a symbolic link, which can point outside the checkout"
    SUBMODULE = "a submodule, whose commit comes from another repository"
    GIT_ATTRIBUTES = "a git attributes file: it picks the filters and diff and merge drivers git runs on the files"

    @property
    def why(self) -> str:
        return self.value


MANIFEST_NAMES = frozenset({"Cargo.toml", "Cargo.lock", "pyproject.toml", "uv.toml", ".python-version"})
MANIFEST_PATTERNS = ("requirements*.txt", "rust-toolchain*")
SCRIPT_PATTERNS = ("*.sh", "*.py", "*.mjs", "*.js")
# The crates' own sources, the Python package's modules included: library code, not scripts.
LIBRARY_SOURCES = ("crates/*/src/*", "crates/*/python/*")
TEST_HOOK_NAMES = frozenset({"conftest.py", "pytest.ini", "setup.cfg", "tox.ini"})
SYMLINK_MODE, SUBMODULE_MODE = "120000", "160000"


@dataclass(frozen=True)
class Touched:
    """A path the diff names, its kinds (empty when nothing about it is flagged) and git's status
    letter (A, M, D, T)."""

    path: str
    kinds: tuple[Kind, ...]
    status: str


@dataclass(frozen=True)
class Entry:
    """One diff entry: `--raw` gives the modes and the status, `--numstat` whether git sees the
    file as binary."""

    old_mode: str
    new_mode: str
    status: str
    binary: bool = False


def matches(text: str, patterns: Iterable[str]) -> bool:
    return any(fnmatch(text, pattern) for pattern in patterns)


def is_script(path: str) -> bool:
    return matches(PurePosixPath(path).name, SCRIPT_PATTERNS) and not matches(path, LIBRARY_SOURCES)


def is_run_docs(path: str) -> bool:
    return path == "README.md" or (path.startswith("docs/") and path.endswith(".md"))


def is_host_executed(path: str) -> bool:
    return covered(path, set(HOST_EXECUTED)) or ".cargo" in PurePosixPath(path).parts


# The kinds a path has by its name and place, in Kind's order. HOST_EXECUTED is left to a path
# that is neither a build script nor a manifest, which say more.
NAME_RULES: tuple[tuple[Kind, Callable[[str], bool]], ...] = (
    (Kind.BUILD_SCRIPT, lambda path: PurePosixPath(path).name == "build.rs"),
    (Kind.MANIFEST, lambda path: PurePosixPath(path).name in MANIFEST_NAMES
     or matches(PurePosixPath(path).name, MANIFEST_PATTERNS)),
    (Kind.SCRIPT, is_script),
    (Kind.TEST_HOOK, lambda path: PurePosixPath(path).name in TEST_HOOK_NAMES),
    (Kind.RUN_DOCS, is_run_docs),
    (Kind.GIT_ATTRIBUTES, lambda path: PurePosixPath(path).name == ".gitattributes"),
)


def name_kinds(path: str) -> set[Kind]:
    kinds = {kind for kind, applies in NAME_RULES if applies(path)}
    if not kinds & {Kind.BUILD_SCRIPT, Kind.MANIFEST} and is_host_executed(path):
        kinds.add(Kind.HOST_EXECUTED)
    return kinds


def entry_kinds(entry: Entry) -> set[Kind]:
    """The kinds an entry has by its content: binary, or a symbolic link or a submodule on either
    side of the diff."""
    modes = {entry.old_mode, entry.new_mode}
    found = {Kind.SYMLINK: SYMLINK_MODE in modes, Kind.SUBMODULE: SUBMODULE_MODE in modes, Kind.BINARY: entry.binary}
    return {kind for kind, present in found.items() if present}


def is_proc_macro(manifest: bytes) -> bool:
    """A Cargo.toml whose `[lib]` sets `proc-macro = true`. One that does not parse is already a
    manifest finding, and cargo refuses it."""
    try:
        lib = tomllib.loads(manifest.decode()).get("lib", {})
    except (UnicodeDecodeError, tomllib.TOMLDecodeError):
        return False
    return isinstance(lib, dict) and any(lib.get(key) is True for key in ("proc-macro", "proc_macro"))


def entries(records: list[str]) -> dict[str, Entry]:
    """The entries of `git diff --raw --numstat -z`: per path a raw record (`:<old mode> <new mode>
    <old sha> <new sha> <status>`, then the path), then per path a numstat record
    (`<added>\\t<deleted>\\t<path>`, `-` for both when binary)."""
    found: dict[str, Entry] = {}
    remaining = iter(records)
    for record in remaining:
        if record.startswith(":"):
            old_mode, new_mode, _, _, status = record[1:].split(" ")
            found[next(remaining)] = Entry(old_mode, new_mode, status)
        else:
            added, deleted, path = record.split("\t", 2)
            found[path] = dataclasses.replace(found[path], binary=(added, deleted) == ("-", "-"))
    return found


def touched(change: Change, root: Path) -> list[Touched]:
    """Every path the change's diff (base...head) names, sorted, with its kinds."""
    no_attributes = git_text(root, "hash-object", "-t", "tree", os.devnull)
    output = git_bytes(root, f"--attr-source={no_attributes}", "diff", "--no-renames", "--raw", "--numstat", "-z",
                       "--end-of-options", f"{change.base}...{change.head}")
    records = [os.fsdecode(record) for record in output.split(b"\0") if record]
    found = []
    for path, entry in sorted(entries(records).items()):
        kinds = name_kinds(path) | entry_kinds(entry)
        if PurePosixPath(path).name == "Cargo.toml" and entry.status != "D" \
                and is_proc_macro(blob(root, change.head, path)):
            kinds.add(Kind.PROC_MACRO)
        found.append(Touched(path, tuple(kind for kind in Kind if kind in kinds), entry.status))
    return found


def flagged(touched: list[Touched], cargo: PackageDiff, python: PackageDiff) -> bool:
    """Whether a change needs a confirmation: a path of any kind, a package added or changed, or a
    dependency file that could not be read."""
    return any(t.kinds for t in touched) or any(d.added or d.changed or d.errors for d in (cargo, python))


@dataclass(frozen=True)
class Risk:
    """What the diff alone says about a change."""

    touched: tuple[Touched, ...]
    cargo: PackageDiff
    python: PackageDiff
    # What the change's Cargo.toml and Cargo.lock files add as sources, and which cannot be read.
    new_sources: CargoSources

    @property
    def flagged(self) -> bool:
        return flagged(list(self.touched), self.cargo, self.python)


# The paths whose tests need the viewer image: the page and the WebAssembly binding it runs.
VIEWER_PATHS = ("viewer/", "crates/oxedi_wasm/")
VIEWER_HINT = "this change touches the viewer; rerun with IMAGE=viewer to run its tests"


def image_hint(touched: list[Touched], kind: ImageKind) -> str:
    """What to rerun with when the base image cannot run the tests of what the change touches;
    empty when nothing needs another image."""
    needs_viewer = any(t.path.startswith(VIEWER_PATHS) for t in touched)
    return VIEWER_HINT if kind is ImageKind.BASE and needs_viewer else ""
