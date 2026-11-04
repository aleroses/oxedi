"""Tests of guard.py: host make refuses while a sandbox run's leftovers are unreviewed, directly and
through make/common.mk."""

import json
import subprocess
import sys
from pathlib import Path

from test_sandbox import World, make_repo_copy

import audit
import storage

GUARD = Path(__file__).resolve().parent.parent / "guard.py"
LINE = ("make: a sandbox run left GNUmakefile, hashlib.py unreviewed; remove or revert them, or git add them once "
        "reviewed (make sandbox-clean forgets the record)")


class Guard(World):
    def leave(self, checkout: Path, *paths: str) -> Path:
        """Writes each of `paths` into the checkout, untracked, and records them as a dirty audit
        would; returns the record."""
        for path in paths:
            (checkout / path).write_text("left by the run\n")
        record = storage.record_for(storage.cache_root({"HOME": str(self.home)}), checkout.resolve())
        record.parent.mkdir(parents=True, exist_ok=True)
        record.write_text(json.dumps(audit.current(checkout, list(paths))))
        return record

    def guard(self, checkout: Path) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, "-I", str(GUARD), "--checkout", str(checkout)],
            env=self.env(), capture_output=True, text=True, check=False, timeout=60,
        )

    def test_no_record_passes(self):
        result = self.guard(self.checkout())
        self.assertEqual((result.returncode, result.stdout, result.stderr), (0, "", ""))

    def test_reviewed_record_passes(self):
        checkout = self.checkout()
        record = self.leave(checkout, "GNUmakefile", "hashlib.py")
        (checkout / "GNUmakefile").unlink()
        (checkout / "hashlib.py").write_text("# reviewed by hand\n")
        result = self.guard(checkout)
        self.assertEqual((result.returncode, result.stderr), (0, ""))
        self.assertFalse(record.exists())

    def test_unreviewed_record_refuses(self):
        checkout = self.checkout()
        record = self.leave(checkout, "GNUmakefile", "hashlib.py")
        result = self.guard(checkout)
        self.assertEqual((result.returncode, result.stdout, result.stderr), (2, "", LINE + "\n"))
        self.assertTrue(record.exists())

    def test_an_unreadable_record_refuses(self):
        checkout = self.checkout()
        record = self.leave(checkout, "GNUmakefile")
        record.write_text("[1]")
        result = self.guard(checkout)
        self.assertEqual(
            (result.returncode, result.stderr),
            (2, f"make: the audit record {record} is not a map of paths to fingerprints; remove it once you have "
                "reviewed the checkout with git status and git diff\n"))

    def test_host_make_refuses_before_any_recipe(self):
        checkout = make_repo_copy(self.tmp / "repo")
        self.leave(checkout, "hashlib.py")
        result = self.make(checkout, "-n", "fmt-check")
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertRegex(
            result.stderr,
            r"^make/common\.mk:\d+: \*\*\* make: a sandbox run left hashlib\.py unreviewed; remove or revert them, "
            r"or git add them once reviewed \(make sandbox-clean forgets the record\)\.  Stop\.\n$")
        self.assertEqual(result.stdout, "")

    def test_clean_goal_skips(self):
        checkout = make_repo_copy(self.tmp / "repo")
        record = self.leave(checkout, "hashlib.py")
        for goal in ((), ("help",)):
            with self.subTest(goal=goal):
                result = self.make(checkout, *goal)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn("sandbox-clean", result.stdout)
        result = self.make(checkout, "sandbox-clean")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(record.exists())
        self.assertEqual(self.make(checkout, "-n", "fmt-check").returncode, 0)

    def test_submake_skips(self):
        checkout = make_repo_copy(self.tmp / "repo")
        self.leave(checkout, "hashlib.py")
        result = self.make(checkout, "-n", "fmt-check", MAKELEVEL="1")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("unreviewed", result.stderr)
