"""What the sandbox protects in a checkout: the paths bound read-only over it and the ones masked.

Read-only, in both phases: what the host executes or trusts after a run (the fixed list below),
plus every tracked file git names like a build or tool configuration, every tracked `.cargo`
directory and every submodule (the host's make targets include their makefiles), wherever they
are, among them every tracked file a host tool loads by its name alone (pytest a `conftest.py`,
Python a `sitecustomize.py`, direnv an `.envrc`). A name the host loads from the root
(`GNUmakefile`, `makefile`, `conftest.py`) is read-only when present; when absent, the run phase
binds an empty read-only file over it, so the run cannot create it. Hidden, in both phases: the
maintainers' notes and every path git ignores, except the sandbox's own outputs, so a run can
neither read nor change them. An ignored directory is masked by an empty read-only tmpfs; an
ignored file has an empty read-only file bound over it. A directory git ignores as a whole (it
holds no tracked file) is masked whole, caches inside it included.
"""

import hashlib
import os
import subprocess
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path, PurePosixPath

# Names the host loads from the root without being told: make reads GNUmakefile and makefile before
# Makefile, and pytest a conftest.py of the root it starts in. Present, each is bound read-only;
# absent, the run phase binds an empty read-only file over it, so the run cannot create one.
ROOT_LOADED = ("GNUmakefile", "makefile", "conftest.py")
# What the host executes or trusts after a run: bound read-only over the checkout.
HOST_EXECUTED = (
    "Makefile", "make", "tools", ".cargo", ".github", "rust-toolchain.toml", "Cargo.toml",
    "Cargo.lock", "CLAUDE.md", ".claude", "crates/oxedi_py/pyproject.toml", ".git", ".gitmodules", *ROOT_LOADED,
)
# Tracked files with these names, anywhere, steer cargo, rustup, uv, pytest, nox, tox, Python or
# direnv when the host runs them: bound read-only too.
TRACKED_NAMES = frozenset({
    "Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml", "pyproject.toml", "uv.toml",
    ".python-version", "conftest.py", "pytest.ini", "setup.cfg", "tox.ini", "noxfile.py", "sitecustomize.py",
    "usercustomize.py", ".envrc",
})
# The maintainers' local notes: masked by an empty read-only tmpfs.
MASKED = (".doc", ".workflow-kit", ".doc/scratch")
# What a run writes on purpose: its own storage at the top, and the Python caches and the built
# extension anywhere. These ignored paths are not masked.
OUTPUT_TOP = frozenset({"target", ".venv"})
OUTPUT_NAMES = frozenset({"__pycache__", ".pytest_cache", ".mypy_cache"})
OUTPUT_FILES = "crates/oxedi_py/python/oxedi/_core*.so"
# Characters that would break a `--mount` field list or a log line.
UNSAFE_IN_PATH = (",", '"', "\n")


class SandboxError(Exception):
    """A reason the sandbox refuses to start; printed as one line, exit status 2."""


@dataclass(frozen=True)
class Checkout:
    """A checkout, the paths bound read-only over it, the directories masked, the files hidden and
    the absent root names the run phase holds with an empty read-only file (relative,
    `/`-separated)."""

    root: Path
    read_only: tuple[str, ...]
    masked: tuple[str, ...]
    hidden: tuple[str, ...]
    placeholders: tuple[str, ...]

    @property
    def name(self) -> str:
        return container_name(self.root)


def path_hash(path: Path) -> str:
    return hashlib.sha256(str(path).encode()).hexdigest()[:12]


def container_name(root: Path) -> str:
    """The name of the one container a checkout may run at a time."""
    return f"oxedi-sandbox-{path_hash(root)}"


def refuse_unsafe(path: Path | str, what: str) -> None:
    if any(c in str(path) for c in UNSAFE_IN_PATH):
        raise SandboxError(f"refused: the {what} path {str(path)!r} holds a comma, a quote or a line break")


def inside(path: str, outers: set[str]) -> bool:
    return any(path.startswith(f"{outer}/") for outer in outers)


def covered(path: str, outers: set[str]) -> bool:
    return path in outers or inside(path, outers)


def refuse_links(root: Path, path: str, rule: str) -> None:
    """Refuses a symbolic link on any step of `path` under `root`, outermost first (`a`, `a/b`,
    `a/b/c`): the host would follow it to a target outside the checkout. `rule` says what the path
    must be."""
    relative = PurePosixPath(path)
    for step in (*reversed(relative.parents[:-1]), relative):
        if (root / step).is_symlink():
            raise SandboxError(f"refused: {step} is a symbolic link; {rule}")


def outermost(paths: set[str]) -> set[str]:
    """The paths that are not inside another one of them: an outer mount already covers them."""
    return {p for p in paths if not inside(p, paths)}


def one_line(lines: list[str]) -> str:
    """Lines of a tool's stderr as one line, so a refusal stays the one line the runner prints."""
    return "; ".join(line.strip() for line in lines if line.strip())


def git_run(root: Path, *args: str) -> tuple[list[str], list[str]]:
    """The NUL-separated records `git <command> -z <options>` prints in `root`, and the lines of its
    stderr. `-z` follows the command, so a pathspec after `--` may end the arguments."""
    command, *options = args
    result = subprocess.run(["git", "-C", str(root), command, "-z", *options], capture_output=True, check=False)
    stderr = os.fsdecode(result.stderr).strip().splitlines()
    if result.returncode != 0:
        raise SandboxError(f"git {' '.join(args)} failed in {root}; the sandbox needs a git checkout: {one_line(stderr)}")
    records = [os.fsdecode(record) for record in result.stdout.split(b"\0") if record]
    return records, stderr


def git_records(root: Path, *args: str) -> list[str]:
    return git_run(root, *args)[0]


def git_paths(root: Path, *args: str) -> set[str]:
    return {record.rstrip("/") for record in git_records(root, *args)}


def escapes(path: str) -> bool:
    """A path that is absolute or climbs out with `..`."""
    return PurePosixPath(path).is_absolute() or ".." in PurePosixPath(path).parts


def declared_submodules(root: Path) -> set[str]:
    """The submodule paths .gitmodules names; one that leaves the checkout is refused."""
    if not (root / ".gitmodules").is_file():
        return set()
    keys = r"^submodule\..*\.path$"
    command = ["git", "-C", str(root), "config", "-z", "--file", ".gitmodules", "--get-regexp", keys]
    result = subprocess.run(command, capture_output=True, check=False)
    # Exit 1: no submodule.<name>.path key at all.
    if result.returncode not in (0, 1):
        raise SandboxError(f"git cannot read {root / '.gitmodules'}: {one_line(os.fsdecode(result.stderr).splitlines())}")
    paths = {os.fsdecode(record).partition("\n")[2] for record in result.stdout.split(b"\0") if record}
    outside = sorted(filter(escapes, paths))
    if outside:
        raise SandboxError(f"refused: .gitmodules names the submodule path {outside[0]!r}, outside the checkout")
    return paths


def tracked_and_submodules(root: Path) -> tuple[set[str], set[str]]:
    """The tracked paths, and every submodule path: the gitlinks git tracks and the paths
    .gitmodules names (a submodule not yet registered is still protected)."""
    staged = [record.split("\t", 1) for record in git_records(root, "ls-files", "--stage")]
    gitlinks = {path for entry, path in staged if entry.startswith("160000 ")}
    return {path for _, path in staged}, gitlinks | declared_submodules(root)


def configuration(tracked: set[str]) -> set[str]:
    """Tracked files named like a build or tool configuration, and the `.cargo` directories that
    hold tracked files."""
    named = {p for p in tracked if PurePosixPath(p).name in TRACKED_NAMES}
    cargo = {p[: p.index(".cargo") + len(".cargo")] for p in tracked if ".cargo" in PurePosixPath(p).parts}
    return named | cargo


def is_output(path: str) -> bool:
    parts = PurePosixPath(path).parts
    return parts[0] in OUTPUT_TOP or bool(OUTPUT_NAMES.intersection(parts)) or fnmatch(path, OUTPUT_FILES)


def ignored_paths(root: Path) -> set[str]:
    """What git ignores in `root` before the run (a wholly ignored directory as one entry), less
    the sandbox's outputs."""
    listed = git_paths(root, "ls-files", "--others", "--ignored", "--exclude-standard", "--directory")
    return {p for p in listed if not is_output(p)}


def present_real(root: Path, paths: set[str]) -> set[str]:
    """The paths that exist; a symbolic link on the path to one of them, or an unsafe name, is
    refused."""
    present = {p for p in paths if os.path.lexists(root / p)}
    for path in sorted(present):
        refuse_unsafe(path, "checkout")
        refuse_links(root, path, "the read-only and masked paths must be real files of the checkout")
    return present


def survey(root: Path) -> Checkout:
    """The read-only, masked and hidden paths of `root`, as git and the fixed lists name them. A
    symbolic link among them, or on the path to one of them, is refused: its target, not the
    checkout's file, would be bound, and the link itself would stay writable."""
    refuse_unsafe(root, "checkout")
    secret = set(MASKED) | ignored_paths(root)
    tracked, submodules = tracked_and_submodules(root)
    present = present_real(root, set(HOST_EXECUTED) | configuration(tracked) | submodules | secret)
    masked = outermost({p for p in present & secret if (root / p).is_dir()})
    # What a masked directory does not already cover: ignored files are hidden, the rest bound.
    free = {p for p in present if not covered(p, masked)}
    hidden = free & secret
    read_only = outermost(free - hidden)
    # `lexists`: a dangling link, or `makefile` on a file system that folds case, is not absent.
    placeholders = tuple(name for name in ROOT_LOADED if not os.path.lexists(root / name))
    return Checkout(root, tuple(sorted(read_only)), tuple(sorted(masked)), tuple(sorted(hidden)), placeholders)
