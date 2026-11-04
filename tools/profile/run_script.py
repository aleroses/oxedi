#!/usr/bin/env python3
"""Runs a Python script as `python SCRIPT ARGS...` would and records how it ended.

usage: run_script.py STATUS_FILE SCRIPT [ARGS...]
STATUS_FILE receives the exit status when the script ends: 0, the code of a SystemExit, or 1 for
an uncaught exception (whose traceback goes to stderr). py-spy exits 0 whatever its child does,
so `py_spy_record.sh` runs the script through this to learn whether it succeeded.
"""

import os
import runpy
import sys
import traceback


def exit_status(error: SystemExit) -> int:
    code = error.code
    if code is None:
        return 0
    if isinstance(code, int):
        return code
    print(code, file=sys.stderr)
    return 1


def main(argv: list[str]) -> int:
    if len(argv) < 2:
        print("usage: run_script.py STATUS_FILE SCRIPT [ARGS...]", file=sys.stderr)
        return 2
    status_file, script, args = argv[0], argv[1], argv[2:]
    sys.argv = [script, *args]
    sys.path[0] = os.path.dirname(os.path.abspath(script))
    status = 1
    try:
        runpy.run_path(script, run_name="__main__")
        status = 0
    except SystemExit as error:
        status = exit_status(error)
    except BaseException:  # noqa: BLE001 - any failure of the script is reported as status 1
        traceback.print_exc()
    finally:
        with open(status_file, "w", encoding="utf-8") as handle:
            handle.write(f"{status}\n")
    return status


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
