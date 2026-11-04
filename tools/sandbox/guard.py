"""Refuses host make while a sandbox run's leftovers are unreviewed.

A dirty audit is recorded in the checkout's storage (audit.py says how). While a path it names is
still as the run left it and not added with git add, the host would run what the run planted, so
the top-level make of the checkout stops with one line instead (make/common.mk runs this file).
With no record, it checks that one file is absent and calls no git.

    python3 -I guard.py --checkout DIR

Exit status: 0 when nothing is left; 2 with one line on stderr otherwise.
"""

import argparse
import os
import sys
from pathlib import Path

# Isolated mode leaves the script's own directory out of sys.path; tools/ is read-only in a run.
sys.path.insert(0, str(Path(__file__).resolve().parent))

from audit import leftovers  # noqa: E402
from layout import SandboxError  # noqa: E402
from storage import cache_root, record_for  # noqa: E402

PREFIX = "make:"


def refusal(root: Path, env: dict[str, str]) -> str:
    """The line that stops make in `root`, or an empty one when nothing a run left is unreviewed."""
    record = record_for(cache_root(env), root)
    if not os.path.lexists(record):
        return ""
    try:
        left = leftovers(root, record)
    except SandboxError as error:
        return f"{PREFIX} {error}"
    except OSError as error:
        return f"{PREFIX} cannot check the sandbox audit record {record} against {root}: {error}"
    if not left:
        return ""
    return (f"{PREFIX} a sandbox run left {', '.join(left)} unreviewed; remove or revert them, or git add them "
            "once reviewed (make sandbox-clean forgets the record)")


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(prog="guard.py", description=__doc__.splitlines()[0])
    parser.add_argument("--checkout", required=True, type=Path, help="the checkout make runs in")
    line = refusal(parser.parse_args(argv).checkout.resolve(), dict(os.environ))
    if line:
        print(line, file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
