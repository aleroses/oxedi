"""Parses a sample file in a loop, as a workload for `make profile-py`.

usage: profile_parse.py [FILE] [ROUNDS]
FILE defaults to the largest anonymized sample of the core's tests; ROUNDS to 200.
"""

import sys
from pathlib import Path

import oxedi

SAMPLE = Path(__file__).resolve().parents[2] / "oxedi_core/tests/samples/edi835_test_united.rmt"


def main(argv: list[str]) -> int:
    path = Path(argv[0]) if argv else SAMPLE
    try:
        rounds = int(argv[1]) if len(argv) > 1 else 200
    except ValueError:
        print(f"profile_parse: ROUNDS must be an integer, got {argv[1]!r}", file=sys.stderr)
        return 2
    rows = 0
    for _ in range(rounds):
        result = oxedi.parse_file(path)
        rows += sum(len(result.tables[name]) for name in result.tables.keys())
    print(f"profile_parse: {path.name} x {rounds}, {rows} rows")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
