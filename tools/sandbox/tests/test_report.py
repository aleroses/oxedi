"""Tests of report.py: which confirmation the reports name. The reports themselves are proven
through the whole flow in test_sandbox_pr.py."""

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import report  # noqa: E402


class Combined(unittest.TestCase):
    def test_refused_wins_then_tty_then_env(self):
        C = report.Confirmed
        self.assertEqual(report.combined([C.TTY, C.REFUSED]), C.REFUSED)
        self.assertEqual(report.combined([C.ENV, C.TTY]), C.TTY)
        self.assertEqual(report.combined([C.NOT_NEEDED, C.ENV]), C.ENV)
        self.assertEqual(report.combined([]), C.NOT_NEEDED)


if __name__ == "__main__":
    unittest.main()
