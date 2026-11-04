"""What `make sandbox-image` reads: the Rust version of the image, from rust-toolchain.toml.

That file is the one place the version is written; the image takes it as a build argument, so a
bump of the file is a new image tag and never a second copy of the pin. A channel that is not an
exact `X.Y.Z` (`stable`, `1.99`, a date) is refused with one stderr line that starts with
`sandbox-image:` and exit status 2.

    python3 -I rust_version.py --root DIR
"""

import argparse
import re
import sys
import tomllib
from pathlib import Path

PREFIX = "sandbox-image:"
TOOLCHAIN = "rust-toolchain.toml"
EXACT = re.compile(r"[0-9]+\.[0-9]+\.[0-9]+")


class Refused(Exception):
    """A reason the toolchain file does not name a version; printed as one line, exit status 2."""


def channel(root: Path) -> str:
    """The exact version `root/rust-toolchain.toml` names."""
    path = root / TOOLCHAIN
    try:
        toolchain = tomllib.loads(path.read_text(encoding="utf-8")).get("toolchain", {})
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise Refused(f"{PREFIX} cannot read {TOOLCHAIN} under {root}: {error}") from error
    if not isinstance(toolchain, dict):
        raise Refused(f"{PREFIX} {TOOLCHAIN} has no [toolchain] table; write [toolchain] with channel = \"X.Y.Z\"")
    found = toolchain.get("channel", "")
    if not isinstance(found, str) or not EXACT.fullmatch(found):
        raise Refused(
            f"{PREFIX} {TOOLCHAIN} names no exact channel X.Y.Z (found '{found}'); "
            f"set channel to a version such as 1.99.0"
        )
    return found


def main(argv: list[str]) -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, required=True)
    try:
        print(channel(parser.parse_args(argv).root))
    except Refused as refusal:
        print(refusal, file=sys.stderr)
        sys.exit(2)


if __name__ == "__main__":
    main(sys.argv[1:])
