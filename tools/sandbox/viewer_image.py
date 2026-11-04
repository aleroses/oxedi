"""What `make sandbox-image-viewer` reads: the build arguments and the tag of the viewer image.

The wasm tools are pinned by crates/oxedi_wasm/scripts/install_tools.sh and Playwright by
docs/requirements.txt; the image takes both from there, so a bump changes the build arguments and
the tag, never a second copy of the pin. Every failure is one `SystemExit` line that starts with
`sandbox-image-viewer:`.

    python3 -I viewer_image.py build-args --root DIR
    python3 -I viewer_image.py tag --root DIR --base TAG
    python3 -I viewer_image.py tree-digest --dir DIR

`tree-digest` also runs inside the image build (the Dockerfile copies this file), so the digest the
build checks and the one it prints on a mismatch come from the same function.
"""

import argparse
import hashlib
import os
import re
import stat
import sys
from dataclasses import dataclass
from pathlib import Path

PREFIX = "sandbox-image-viewer:"
INSTALL_TOOLS = "crates/oxedi_wasm/scripts/install_tools.sh"
DOCS_REQUIREMENTS = "docs/requirements.txt"
PLAYWRIGHT_LOCK = "tools/sandbox/playwright.txt"
DOCKERFILE = "tools/sandbox/Dockerfile.viewer"
# This file: the Dockerfile copies it into the build and runs its tree-digest.
VIEWER_IMAGE = "tools/sandbox/viewer_image.py"
IMAGE_REPOSITORY = "oxedi-sandbox-viewer"
# Read, in this order, by the tag; each one is an input of the build.
TAG_INPUTS = (DOCKERFILE, PLAYWRIGHT_LOCK, INSTALL_TOOLS, VIEWER_IMAGE)
# Playwright's own markers, written after an install; they are not part of the browser.
TREE_MARKERS = frozenset({"INSTALLATION_COMPLETE", "DEPENDENCIES_VALIDATED"})
PLAYWRIGHT_LINE = re.compile(r"^playwright==(\S+?)(?:\s*[;\\].*)?$", re.MULTILINE)


@dataclass(frozen=True)
class WasmPins:
    bindgen_version: str
    bindgen_sha256: str
    binaryen_version: str
    binaryen_sha256: str


def fail(message: str) -> SystemExit:
    return SystemExit(f"{PREFIX} {message}")


def shell_value(text: str, name: str) -> str:
    """The quoted value of the `NAME="value"` line of a shell script."""
    found = re.search(rf'^{name}="([^"\n]+)"$', text, re.MULTILINE)
    if found is None:
        raise fail(f'{INSTALL_TOOLS} has no {name}="<value>" line')
    return found.group(1)


def wasm_pins(install_tools: str) -> WasmPins:
    return WasmPins(
        bindgen_version=shell_value(install_tools, "WASM_BINDGEN_VERSION"),
        bindgen_sha256=shell_value(install_tools, "WASM_BINDGEN_SHA256"),
        binaryen_version=shell_value(install_tools, "BINARYEN_VERSION"),
        binaryen_sha256=shell_value(install_tools, "BINARYEN_SHA256"),
    )


def playwright_pin(requirements: str, source: str = DOCS_REQUIREMENTS) -> str:
    """The version of the `playwright==X` line, without its hashes, markers or comments."""
    found = PLAYWRIGHT_LINE.search(requirements)
    if found is None:
        raise fail(f"{source} pins no playwright==<version>")
    return found.group(1)


def read(root: Path, relative: str) -> str:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError) as error:
        raise fail(f"cannot read {relative} under {root}: {error}") from error


def build_args(root: Path) -> list[str]:
    """`--build-arg NAME=value` pairs, after checking the lock and the docs pin one Playwright."""
    locked = playwright_pin(read(root, PLAYWRIGHT_LOCK), PLAYWRIGHT_LOCK)
    docs = playwright_pin(read(root, DOCS_REQUIREMENTS))
    if locked != docs:
        raise fail(
            f"{PLAYWRIGHT_LOCK} pins playwright {locked} but {DOCS_REQUIREMENTS} pins {docs}; "
            f"regenerate {PLAYWRIGHT_LOCK}"
        )
    pins = wasm_pins(read(root, INSTALL_TOOLS))
    values = {
        "WASM_BINDGEN_VERSION": pins.bindgen_version,
        "WASM_BINDGEN_SHA256": pins.bindgen_sha256,
        "BINARYEN_VERSION": pins.binaryen_version,
        "BINARYEN_SHA256": pins.binaryen_sha256,
        "PLAYWRIGHT_VERSION": docs,
    }
    return [arg for name, value in values.items() for arg in ("--build-arg", f"{name}={value}")]


def tag(root: Path, base_tag: str) -> str:
    digest = hashlib.sha256(base_tag.encode("utf-8"))
    for relative in TAG_INPUTS:
        try:
            digest.update((root / relative).read_bytes())
        except OSError as error:
            raise fail(f"cannot read {relative} under {root}: {error}") from error
    return f"{IMAGE_REPOSITORY}:{digest.hexdigest()[:12]}"


def tree_line(directory: Path, path: Path) -> str:
    """`<sha256 or 'link'>  <mode octal>  <relative path>[ -> <target>]` for a regular file or a
    symbolic link; an empty line for anything else (a directory is named by the paths under it)."""
    relative = path.relative_to(directory).as_posix()
    try:
        info = os.lstat(path)
        mode = f"{stat.S_IMODE(info.st_mode):04o}"
        if stat.S_ISLNK(info.st_mode):
            return f"link  {mode}  {relative} -> {os.readlink(path)}\n"
        if stat.S_ISREG(info.st_mode):
            return f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {mode}  {relative}\n"
    except OSError as error:
        raise fail(f"cannot read {path}: {error}") from error
    return ""


def unlisted(error: OSError) -> None:
    raise fail(f"cannot list {error.filename}: {error.strerror or error}")


def tree_digest(directory: Path) -> str:
    """SHA-256 of the sorted lines (`tree_line`) of every regular file and symbolic link in the tree,
    with its mode, and each link's target; Playwright's marker files are left out. A link is never
    followed."""
    lines = []
    for parent, subdirs, files in os.walk(directory, onerror=unlisted):
        names = (name for name in subdirs + files if name not in TREE_MARKERS)
        lines.extend(tree_line(directory, Path(parent, name)) for name in names)
    return hashlib.sha256("".join(sorted(lines)).encode("utf-8")).hexdigest()


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="viewer_image.py")
    parser.add_argument("command", choices=("build-args", "tag", "tree-digest"))
    parser.add_argument("--root", type=Path)
    parser.add_argument("--base")
    parser.add_argument("--dir", type=Path)
    options = parser.parse_args(argv)
    if options.command == "tree-digest":
        if options.dir is None:
            raise fail("tree-digest needs --dir <the browsers directory>")
        print(tree_digest(options.dir))
        return 0
    if options.root is None:
        raise fail(f"{options.command} needs --root <the repository root>")
    if options.command == "tag":
        if not options.base:
            raise fail("tag needs --base <the base image tag>")
        print(tag(options.root, options.base))
    else:
        print("\n".join(build_args(options.root)))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
