"""The packages a change adds, changes or removes, judged without running any of it: the Cargo.lock
packages and the Python packages the dependency files declare.

Every text is read with `git cat-file blob <sha>:<path>` through `isolated_git` and parsed as data;
nothing of the clone is imported or executed. A dependency file that does not parse is a finding.
"""

import dataclasses
import os
import re
import tomllib
from collections.abc import Iterable, Iterator
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path, PurePosixPath

from source import Change, git_bytes, git_text, redacted


# The source Cargo.lock gives a workspace member or a path crate, which it lists without one.
NO_SOURCE = "local"
# A PEP 508 name, then nothing or what may follow a name: extras, a version, markers or `@ url`.
REQUIREMENT = re.compile(r"\s*([A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?)\s*((?:[\[(<>=!~;@].*)?)")

# (name, old version or spec, new version or spec, source): an added package has an empty old
# side, a removed one an empty new side. The source is the registry or git URL for Cargo.lock
# (`local` for a workspace or path crate) and the declaring file for Python.
Package = tuple[str, str, str, str]


@dataclass(frozen=True)
class PackageDiff:
    """The packages a change adds, removes and changes, each sorted, and why a dependency file
    could not be read."""

    added: tuple[Package, ...] = ()
    removed: tuple[Package, ...] = ()
    changed: tuple[Package, ...] = ()
    errors: tuple[str, ...] = ()


def total(diffs: Iterable[PackageDiff]) -> PackageDiff:
    """The diffs joined into one, each field sorted."""
    fields = [field.name for field in dataclasses.fields(PackageDiff)]
    joined: dict[str, list] = {name: [] for name in fields}
    for diff in diffs:
        for name in fields:
            joined[name].extend(getattr(diff, name))
    return PackageDiff(**{name: tuple(sorted(items)) for name, items in joined.items()})


def blob(root: Path, sha: str, path: str) -> bytes:
    """The bytes of `path` at `sha`, as stored: no filter or text conversion applies."""
    return git_bytes(root, "cat-file", "blob", f"{sha}:{path}")


def table(value: object, where: str) -> dict:
    if not isinstance(value, dict):
        raise ValueError(f"{where} is not a table")
    return value


def strings(value: object, where: str) -> list[str]:
    if not (isinstance(value, list) and all(isinstance(item, str) for item in value)):
        raise ValueError(f"{where} is not a list of strings")
    return value


def toml(text: str, where: str) -> dict:
    try:
        return tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise ValueError(f"{where} is not valid TOML: {error}") from error


def lock_packages(text: str, where: str) -> dict[str, set[tuple[str, str]]]:
    """The packages of a Cargo.lock: name -> its (version, source) pairs. Cargo may lock one
    version of a name from two sources, so both are kept."""
    packages = toml(text, where).get("package", [])
    if not isinstance(packages, list):
        raise ValueError(f"{where} package is not an array of tables")
    found: dict[str, set[tuple[str, str]]] = {}
    for package in packages:
        fields = table(package, f"{where} [[package]]")
        name, version, source = fields.get("name"), fields.get("version"), fields.get("source", NO_SOURCE)
        if not all(isinstance(value, str) for value in (name, version, source)):
            raise ValueError(f"{where} holds a [[package]] without a name, a version and a source as text: {package!r}")
        found.setdefault(name, set()).add((version, source))
    return found


def compare_versions(name: str, old: set[tuple[str, str]], new: set[tuple[str, str]]) -> PackageDiff:
    """One package's (version, source) pairs before and after: one pair swapped for another is a
    change (a new version, or the same version from another source); otherwise each pair is added
    or removed."""
    gone, came = sorted(old - new), sorted(new - old)
    if len(gone) == len(came) == 1:
        (old_version, _), (new_version, source) = gone[0], came[0]
        return PackageDiff(changed=((name, old_version, new_version, source),))
    return PackageDiff(added=tuple((name, "", v, source) for v, source in came),
                       removed=tuple((name, v, "", source) for v, source in gone))


def cargo_lock_diff(old: str, new: str, path: str = "Cargo.lock") -> PackageDiff:
    """The packages a Cargo.lock adds, removes and changes (an empty text holds none). When either
    side does not parse, the diff holds only that finding, which names `path`."""
    try:
        before = lock_packages(old, f"{path} before the change")
        after = lock_packages(new, f"{path} after the change")
    except ValueError as error:
        return PackageDiff(errors=(str(error),))
    names = before.keys() | after.keys()
    return total(compare_versions(name, before.get(name, set()), after.get(name, set())) for name in names)


def requirement(line: str) -> tuple[str, str]:
    """A requirement's normalized name and the rest of it (extras, version, markers or URL); a line
    that starts with no name (a bare URL or path) is its own name."""
    match = REQUIREMENT.fullmatch(line)
    if match is None:
        return line.strip(), ""
    return re.sub(r"[-_.]+", "-", match[1]).lower(), match[2].strip()


def requirements_lines(text: str) -> Iterator[str]:
    """The lines of a requirements file, continuations joined, comments and per-requirement
    options (`--hash`) left out. An option line (`-e <url>`, `-r <file>`, `-c <file>`,
    `--index-url <url>`) names where packages come from, so it is kept whole."""
    for line in text.replace("\\\n", " ").splitlines():
        line = re.split(r"(?:^|\s)#", line, maxsplit=1)[0].strip()
        if not line.startswith("-"):
            line = re.split(r"\s-", line, maxsplit=1)[0]
        if line:
            yield " ".join(line.split())


def group(groups: dict, name: str, where: str, seen: tuple[str, ...] = ()) -> Iterator[str]:
    """The requirements of a dependency group, every `include-group` resolved."""
    if name in seen or name not in groups:
        raise ValueError(f"{where} [dependency-groups] includes {name!r}, which is undefined or includes itself")
    for item in strings_and_tables(groups[name], f"{where} [dependency-groups] {name}"):
        if isinstance(item, str):
            yield item
        else:
            yield from group(groups, included(item, where, name), where, seen + (name,))


def strings_and_tables(value: object, where: str) -> list[str | dict]:
    if not (isinstance(value, list) and all(isinstance(item, str | dict) for item in value)):
        raise ValueError(f"{where} is not a list of requirements and include-group tables")
    return value


def included(item: dict, where: str, name: str) -> str:
    """The group an `{include-group = "x"}` table names."""
    if item.keys() != {"include-group"} or not isinstance(item["include-group"], str):
        raise ValueError(
            f"{where} [dependency-groups] {name} holds {item!r}, neither a requirement nor an include-group"
        )
    return item["include-group"]


def pyproject_lines(text: str, where: str) -> Iterator[str]:
    data = toml(text, where)
    project = table(data.get("project", {}), f"{where} [project]")
    yield from strings(project.get("dependencies", []), f"{where} [project] dependencies")
    extras = table(project.get("optional-dependencies", {}), f"{where} [project.optional-dependencies]")
    for extra, listed in extras.items():
        yield from strings(listed, f"{where} [project.optional-dependencies] {extra}")
    groups = table(data.get("dependency-groups", {}), f"{where} [dependency-groups]")
    for name in groups:
        yield from group(groups, name, where)
    build = table(data.get("build-system", {}), f"{where} [build-system]")
    yield from strings(build.get("requires", []), f"{where} [build-system] requires")


def declared(path: str, text: str, where: str) -> dict[str, str]:
    """The packages a dependency file declares: name -> its specs, sorted and joined by ` | `."""
    is_pyproject = PurePosixPath(path).name == "pyproject.toml"
    lines = pyproject_lines(text, where) if is_pyproject else requirements_lines(text)
    specs: dict[str, set[str]] = {}
    for line in lines:
        name, spec = requirement(redacted(line))
        specs.setdefault(name, set()).add(spec)
    return {name: " | ".join(sorted(found)) for name, found in specs.items()}


def compare_specs(path: str, old: dict[str, str], new: dict[str, str]) -> PackageDiff:
    return PackageDiff(
        added=tuple(sorted((n, "", new[n], path) for n in new.keys() - old.keys())),
        removed=tuple(sorted((n, old[n], "", path) for n in old.keys() - new.keys())),
        changed=tuple(sorted((n, old[n], new[n], path) for n in old.keys() & new.keys() if old[n] != new[n])),
    )


def file_diff(path: str, old: str, new: str) -> PackageDiff:
    try:
        before = declared(path, old, f"{path} before the change")
        after = declared(path, new, f"{path} after the change")
    except ValueError as error:
        return PackageDiff(errors=(str(error),))
    return compare_specs(path, before, after)


def declared_python_diff(old_files: dict[str, str], new_files: dict[str, str]) -> PackageDiff:
    """The Python packages the dependency files declare before and after (path -> text; a file
    absent on one side declares nothing there): `pyproject.toml` from `[project] dependencies`,
    `[project.optional-dependencies]`, `[dependency-groups]` and `[build-system] requires`,
    `requirements*.txt` line by line."""
    paths = old_files.keys() | new_files.keys()
    return total(file_diff(path, old_files.get(path, ""), new_files.get(path, "")) for path in paths)


def is_python_dependencies(path: str) -> bool:
    name = PurePosixPath(path).name
    return name == "pyproject.toml" or fnmatch(name, "requirements*.txt")


@dataclass(frozen=True)
class Texts:
    """The dependency files at one end of the diff: path -> text, and the ones that are not UTF-8."""

    files: dict[str, str]
    errors: tuple[str, ...]


def texts(root: Path, sha: str, paths: list[str], side: str) -> Texts:
    """The texts of `paths` at `sha`; a path absent there is left out."""
    listed = git_bytes(root, "ls-tree", "-r", "--name-only", "-z", sha, "--", *paths).split(b"\0")
    present = {os.fsdecode(path) for path in listed if path}
    files, errors = {}, []
    for path in sorted(present):
        try:
            files[path] = blob(root, sha, path).decode()
        except UnicodeDecodeError as error:
            errors.append(f"{path} {side} is not UTF-8 text: {error}")
    return Texts(files, tuple(errors))


def both_ends(change: Change, root: Path, paths: list[str]) -> tuple[Texts, Texts]:
    """The texts of `paths` where the diff starts (the merge base of base and head) and at the
    head."""
    start = git_text(root, "merge-base", "--end-of-options", change.base, change.head)
    return texts(root, start, paths, "before the change"), texts(root, change.head, paths, "after the change")


def dependency_diffs(change: Change, root: Path, paths: list[str]) -> tuple[PackageDiff, PackageDiff]:
    """The Cargo.lock and the Python dependency diffs of the dependency files among the touched
    `paths`."""
    locks = [path for path in paths if PurePosixPath(path).name == "Cargo.lock"]
    python = [path for path in paths if is_python_dependencies(path)]
    cargo, declared_diff = PackageDiff(), PackageDiff()
    if locks:
        old, new = both_ends(change, root, locks)
        cargo = total([*(cargo_lock_diff(old.files.get(p, ""), new.files.get(p, ""), p) for p in locks),
                       PackageDiff(errors=old.errors + new.errors)])
    if python:
        old, new = both_ends(change, root, python)
        declared_diff = total([declared_python_diff(old.files, new.files), PackageDiff(errors=old.errors + new.errors)])
    return cargo, declared_diff
