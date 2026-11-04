"""The packages a change adds, changes or removes, judged without running any of it: the Cargo.lock
packages and the Python packages the dependency files declare.

Every text is read with `git cat-file blob <sha>:<path>` through `isolated_git` and parsed as data;
nothing of the clone is imported or executed. A dependency file that does not parse is a finding.
"""

import dataclasses
import os
import re
import tomllib
from collections.abc import Callable, Iterable, Iterator
from dataclasses import dataclass
from fnmatch import fnmatch
from pathlib import Path, PurePosixPath

from source import Change, git_bytes, git_text, redacted


# The source Cargo.lock gives a workspace member or a path crate, which it lists without one.
NO_SOURCE = "local"
# A PEP 508 name, then nothing or what may follow a name: extras, a version, markers or `@ url`.
REQUIREMENT = re.compile(r"\s*([A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?)\s*((?:[\[(<>=!~;@].*)?)")
# The Cargo.lock sources of crates.io, the registry cargo-deny reaches anyway, matched whole so a
# look-alike such as `...crates.io-index.example` is a new source.
CRATES_IO = frozenset({"registry+https://github.com/rust-lang/crates.io-index", "sparse+https://index.crates.io/"})
# The `[tool.uv]` keys that name where packages are downloaded from.
UV_INDEX_KEYS = ("index", "extra-index-url", "index-url", "find-links")

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
    """A TOML text parsed, without a leading UTF-8 byte order mark, as cargo and uv drop it."""
    try:
        return tomllib.loads(text.removeprefix("\ufeff"))
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


def strings_under(value: object, key: str) -> Iterator[str]:
    """Every string `key` holds in a TOML value, at any depth."""
    if isinstance(value, dict):
        for name, item in value.items():
            if name == key and isinstance(item, str):
                yield item
            else:
                yield from strings_under(item, key)
    elif isinstance(value, list):
        for item in value:
            yield from strings_under(item, key)


def manifest_sources(data: dict) -> set[str]:
    """The URLs a Cargo.toml names: every `git` and `registry-index`, in a dependency table of any
    kind or target, in `[patch.*]`, `[replace]` or `[source.*]`, and the `registry` of a `[source.*]`
    table (a dependency's `registry` is a name, resolved by `config_sources`). Both keys are taken at
    any depth on purpose, even under `[package.metadata]`: reporting one URL too many is the safe side."""
    sources = data.get("source", {})
    tables = sources.values() if isinstance(sources, dict) else ()
    return ({url for key in ("git", "registry-index") for url in strings_under(data, key)}
            | {url for source in tables for url in strings_under(source, "registry")})


def config_sources(data: dict) -> set[str]:
    """The URLs a `.cargo/config` or `.cargo/config.toml` names: what a manifest would, and the
    `index` of `[registry]` and of every `[registries.*]`, where a dependency's `registry` name leads."""
    indexes = (data.get(key) for key in ("registry", "registries"))
    return manifest_sources(data) | {url for found in indexes for url in strings_under(found, "index")}


def lock_url(source: str) -> str:
    """The URL of a Cargo.lock source without its kind (`git+`, `registry+`, `sparse+`), query or fragment."""
    url = source.partition("+")[2] or source
    return re.split(r"[?#]", url, maxsplit=1)[0]


def lock_sources(data: dict) -> set[str]:
    """The URLs of the Cargo.lock packages that do not come from crates.io."""
    packages = data.get("package", [])
    sources = strings_under(packages if isinstance(packages, list) else [], "source")
    return {lock_url(source) for source in sources if source not in CRATES_IO}


SOURCE_READERS = {"Cargo.toml": manifest_sources, "Cargo.lock": lock_sources,
                  ".cargo/config": config_sources, ".cargo/config.toml": config_sources}


def source_reader(path: str) -> Callable[[dict], set[str]] | None:
    """How the sources of `path` are read, by its name or, for a cargo config, its last two parts."""
    parts = PurePosixPath(path).parts
    return SOURCE_READERS.get("/".join(parts[-2:])) or SOURCE_READERS.get(parts[-1])


@dataclass(frozen=True)
class CargoSources:
    """The URLs the change's Cargo files (`source_reader`) add, sorted and redacted, and why one
    could not be read after the change: one that cannot be read may name any source, so it is
    asked about like a new one."""

    urls: tuple[str, ...] = ()
    unreadable: tuple[str, ...] = ()


def source_urls(files: dict[str, str], side: str) -> tuple[set[str], tuple[str, ...]]:
    """The URLs the Cargo files among `files` (path -> text) name, and why one could not be parsed."""
    found, errors = set(), []
    for path, text in sorted(files.items()):
        if read := source_reader(path):
            try:
                found |= read(toml(text, f"{path} {side}"))
            except ValueError as error:
                errors.append(str(error))
    return found, tuple(errors)


def read_cargo_sources(old: dict[str, str], new: dict[str, str]) -> CargoSources:
    """What `cargo_sources` finds, with the files that could not be parsed after the change. One
    that cannot be parsed before the change only makes more URLs look new."""
    before, _ = source_urls(old, "before the change")
    after, unreadable = source_urls(new, "after the change")
    return CargoSources(tuple(sorted({redacted(url) for url in after - before})), unreadable)


def cargo_sources(old: dict[str, str], new: dict[str, str]) -> tuple[str, ...]:
    """The URLs the Cargo files name after the change and not before (path -> text on each side),
    sorted, credentials redacted: where cargo-deny would download from for the first time."""
    return read_cargo_sources(old, new).urls


def requirement(line: str) -> tuple[str, str]:
    """A requirement's normalized name and the rest of it (extras, version, markers or URL); a line
    that starts with no name (a bare URL or path) is its own name."""
    match = REQUIREMENT.fullmatch(line)
    if match is None:
        return line.strip(), ""
    return normalized(match[1]), match[2].strip()


def normalized(name: str) -> str:
    """A package name as Python packaging compares it: lower case, `-`, `_` and `.` runs as `-`."""
    return re.sub(r"[-_.]+", "-", name).lower()


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


def rendered(value: object) -> str:
    """A TOML value on one line: a table as its `k=v` pairs sorted by key, an array as its items
    joined by ` | `."""
    if isinstance(value, dict):
        return ", ".join(f"{key}={rendered(value[key])}" for key in sorted(value))
    if isinstance(value, list):
        return " | ".join(rendered(item) for item in value)
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def uv_declared(path: str, text: str, where: str) -> dict[str, str]:
    """Where a pyproject.toml tells uv to take packages from: per `[tool.uv.sources]` key its table,
    and per `[tool.uv]` index key, named `uv:<key>`, its value, each on one line with every URL's
    credentials redacted. A requirements file says this in its option lines instead."""
    if PurePosixPath(path).name != "pyproject.toml":
        return {}
    tools = table(toml(text, where).get("tool", {}), f"{where} [tool]")
    uv = table(tools.get("uv", {}), f"{where} [tool.uv]")
    sources = table(uv.get("sources", {}), f"{where} [tool.uv.sources]")
    found = {normalized(name): rendered(spec) for name, spec in sources.items()}
    found |= {f"uv:{key}": rendered(uv[key]) for key in UV_INDEX_KEYS if key in uv}
    return {name: redacted(spec) for name, spec in found.items()}


def compare_specs(path: str, old: dict[str, str], new: dict[str, str]) -> PackageDiff:
    return PackageDiff(
        added=tuple(sorted((n, "", new[n], path) for n in new.keys() - old.keys())),
        removed=tuple(sorted((n, old[n], "", path) for n in old.keys() - new.keys())),
        changed=tuple(sorted((n, old[n], new[n], path) for n in old.keys() & new.keys() if old[n] != new[n])),
    )


def file_diff(path: str, old: str, new: str) -> PackageDiff:
    """The packages a file declares and where uv takes them from, compared apart, so a source
    for a declared package shows as its own entry."""
    try:
        sides = [(read(path, old, f"{path} before the change"), read(path, new, f"{path} after the change"))
                 for read in (declared, uv_declared)]
    except ValueError as error:
        return PackageDiff(errors=(str(error),))
    return total(compare_specs(path, before, after) for before, after in sides)


def declared_python_diff(old_files: dict[str, str], new_files: dict[str, str]) -> PackageDiff:
    """The Python packages the dependency files declare before and after (path -> text; a file
    absent on one side declares nothing there): `pyproject.toml` from `[project] dependencies`,
    `[project.optional-dependencies]`, `[dependency-groups]` and `[build-system] requires`, with
    uv's sources and indexes from `[tool.uv]`; `requirements*.txt` line by line."""
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


def new_cargo_sources(change: Change, root: Path, paths: list[str]) -> CargoSources:
    """The sources the Cargo files among the touched `paths` add, and the ones that cannot be read
    after the change, not UTF-8 or not TOML."""
    cargo = [path for path in paths if source_reader(path)]
    if not cargo:
        return CargoSources()
    old, new = both_ends(change, root, cargo)
    found = read_cargo_sources(old.files, new.files)
    return CargoSources(found.urls, new.errors + found.unreadable)
