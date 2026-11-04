#!/usr/bin/env python3
"""Checks that no development dependency reaches what oxedi publishes.

crates: for each published crate, the normal dependencies `cargo tree` reports must all be in
        the crate's allowed list below.
wheel:  every `Requires-Dist` of a built wheel must be gated by an `extra` marker naming one of
        the user extras, so a plain `pip install oxedi` installs none of them.

usage: deps.py crates
       deps.py wheel [WHEEL ...]     (default: target/wheels/*.whl)
Exit status 0 when everything is allowed, 1 on a finding, 2 when a check cannot run.
"""

import re
import subprocess
import sys
import zipfile
from email.parser import BytesParser
from pathlib import Path

# The normal dependencies of each published crate, workspace members included. Adding a
# dependency to a crate's [dependencies] means adding it here, in the same change, after review.
ALLOWED_CRATES = {
    "oxedi_core": ["hmac", "serde", "serde_json", "sha2"],
    "oxedi_py": [
        "arrow-array",
        "arrow-buffer",
        "arrow-data",
        "arrow-schema",
        "bytes",
        "oxedi_core",
        "pyo3",
        "pyo3-stub-gen",
        "self_cell",
    ],
    "oxedi_duckdb": ["libduckdb-sys", "oxedi_core"],
    "oxedi_wasm": ["oxedi_core", "serde", "serde_json", "wasm-bindgen"],
}

# The extras a user can ask for (`pip install "oxedi[pandas]"`); the only markers a
# Requires-Dist may carry.
ALLOWED_EXTRAS = ["edi-835-parser", "pandas", "polars", "pyx12"]

EXTRA_MARKER = re.compile(r"""\bextra\s*==\s*['"]([^'"]+)['"]""")


class CannotCheck(Exception):
    """The check could not run, which is different from finding a leak."""


def tree_dependencies(tree: str) -> list[str]:
    """The names listed by `cargo tree --depth 1 --prefix none`, without the root (first line)."""
    names = []
    for line in tree.splitlines()[1:]:
        if line.strip():
            names.append(line.split()[0])
    return names


def crate_findings(crate: str, dependencies: list[str], allowed: list[str]) -> list[str]:
    """One message per dependency of `crate` that is not allowed."""
    return [
        f"{crate}: normal dependency `{dep}` is not in the allowed list ({', '.join(allowed)}); "
        f"a development tool belongs in [dev-dependencies], and a real new dependency needs "
        f"review and an entry in ALLOWED_CRATES of tools/deps/deps.py"
        for dep in sorted(set(dependencies))
        if dep not in allowed and dep != crate
    ]


def stale_entries(crate: str, dependencies: list[str], allowed: list[str]) -> list[str]:
    """One message per allowed entry that `crate` no longer depends on."""
    return [
        f"{crate}: allowed dependency `{dep}` is no longer a normal dependency; remove it from "
        f"ALLOWED_CRATES in tools/deps/deps.py"
        for dep in allowed
        if dep not in dependencies
    ]


def cargo_tree(crate: str) -> str:
    command = ["cargo", "tree", "-e", "normal", "--depth", "1", "--target", "all", "--all-features", "--prefix", "none", "--locked", "-p", crate]
    try:
        done = subprocess.run(command, capture_output=True, text=True, check=False)
    except OSError as e:
        raise CannotCheck(f"{crate}: cannot run `{' '.join(command)}`: {e}") from e
    if done.returncode != 0:
        raise CannotCheck(f"{crate}: `{' '.join(command)}` failed: {done.stderr.strip()}")
    return done.stdout


def wheel_findings(wheel: str, requires_dist: list[str], allowed_extras: list[str]) -> list[str]:
    """One message per `Requires-Dist` of `wheel` that is unconditional or gated by another extra."""
    allowed = ", ".join(allowed_extras)
    findings = []
    for requirement in requires_dist:
        _, _, marker = requirement.partition(";")
        extras = EXTRA_MARKER.findall(marker)
        if not extras:
            findings.append(
                f"{wheel}: Requires-Dist `{requirement}` has no `extra ==` marker, so every "
                f"install of the wheel would pull it in; allowed extras: {allowed}"
            )
            continue
        findings.extend(
            f"{wheel}: Requires-Dist `{requirement}` belongs to extra `{extra}`, which is not a "
            f"user extra; allowed extras: {allowed}"
            for extra in extras
            if extra not in allowed_extras
        )
    return findings


def wheel_requirements(wheel: Path) -> list[str]:
    try:
        with zipfile.ZipFile(wheel) as archive:
            names = [n for n in archive.namelist() if n.endswith(".dist-info/METADATA")]
            if len(names) != 1:
                raise CannotCheck(f"{wheel}: expected one .dist-info/METADATA, found {len(names)}")
            headers = BytesParser().parsebytes(archive.read(names[0]), headersonly=True)
    except (OSError, zipfile.BadZipFile) as e:
        raise CannotCheck(f"{wheel}: cannot read the wheel: {e}") from e
    return headers.get_all("Requires-Dist") or []


def check_crates() -> list[str]:
    findings = []
    for crate, allowed in ALLOWED_CRATES.items():
        dependencies = tree_dependencies(cargo_tree(crate))
        findings += crate_findings(crate, dependencies, allowed)
        findings += stale_entries(crate, dependencies, allowed)
    return findings


def check_wheels(paths: list[str]) -> tuple[list[str], list[str]]:
    """The findings and the names of the wheels checked (found in target/wheels when no path is given)."""
    wheels = [Path(p) for p in paths] or sorted(Path("target/wheels").glob("*.whl"))
    if not wheels:
        raise CannotCheck("no wheel given and none in target/wheels; run make dist first")
    findings = []
    for wheel in wheels:
        findings += wheel_findings(wheel.name, wheel_requirements(wheel), ALLOWED_EXTRAS)
    if findings and not paths:
        findings.append(
            "the wheel was read from target/wheels, not built by this check; "
            "if it is stale, run make clean-dist && make dist"
        )
    return findings, [w.name for w in wheels]


def main(argv: list[str]) -> int:
    if not argv or argv[0] not in ("crates", "wheel"):
        print(__doc__, file=sys.stderr)
        return 2
    try:
        if argv[0] == "crates":
            findings, detail = check_crates(), ""
        else:
            findings, names = check_wheels(argv[1:])
            source = "existing wheel" if not argv[1:] else "wheel"
            detail = f" (checked {source}{'s' if len(names) > 1 else ''}: {', '.join(names)})"
    except CannotCheck as e:
        print(f"deps-check: {e}", file=sys.stderr)
        return 2
    for finding in findings:
        print(f"deps-check: {finding}", file=sys.stderr)
    if findings:
        return 1
    print(f"deps-check: {argv[0]} ok{detail}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
