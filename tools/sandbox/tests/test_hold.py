"""Tests of hold.py: the placeholders the run phase holds at the root, what they say when a killed
run leaves them, and how the next start clears or refuses them."""

import os
import shutil
import tempfile
import unittest
from pathlib import Path

from test_sandbox import World, make_repo_copy

import hold
import layout
import storage

NAMES = ("GNUmakefile", "makefile", "conftest.py")


class Placeholders(unittest.TestCase):
    def setUp(self):
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        self.held = self.root / ".held.json"

    def entries(self) -> list[str]:
        return sorted(os.listdir(self.root))

    def test_each_name_holds_its_body_during_the_run_only(self):
        with hold.placeholders(self.root, self.held, NAMES):
            for name in NAMES:
                self.assertEqual((self.root / name).read_text(), hold.BODIES[name])
                self.assertEqual((self.root / name).stat().st_mode & 0o777, 0o444)
            self.assertTrue(self.held.exists())
        self.assertEqual(self.entries(), [])

    def test_the_make_body_names_the_cause(self):
        self.assertEqual(
            hold.BODIES["GNUmakefile"],
            "$(error sandbox: a killed sandbox run left this placeholder GNUmakefile; remove it, and any GNUmakefile, "
            "makefile or conftest.py at the root that holds only such a line, then retry)\n")

    def test_a_name_that_appeared_is_refused(self):
        (self.root / "makefile").write_text("all:\n")
        with self.assertRaises(layout.SandboxError) as caught, hold.placeholders(self.root, self.held, ("makefile",)):
            pass
        self.assertTrue(str(caught.exception).startswith(
            f"cannot make the placeholder makefile the run binds over in {self.root}: "), caught.exception)
        self.assertEqual((self.root / "makefile").read_text(), "all:\n")
        self.assertFalse(self.held.exists())

    def test_a_changed_placeholder_is_left_for_the_audit(self):
        with hold.placeholders(self.root, self.held, ("GNUmakefile",)):
            (self.root / "GNUmakefile").chmod(0o644)
            (self.root / "GNUmakefile").write_text("all:\n")
        self.assertEqual(self.entries(), ["GNUmakefile"])

    @unittest.skipIf(os.geteuid() == 0, "root removes files from a read-only directory")
    def test_what_cannot_be_removed_is_one_line(self):
        self.addCleanup(self.root.chmod, 0o700)
        with self.assertRaises(layout.SandboxError) as caught, hold.placeholders(self.root, self.held, ("GNUmakefile",)):
            self.root.chmod(0o500)
        self.assertEqual(
            str(caught.exception),
            f"cannot remove {self.root / 'GNUmakefile'} (Permission denied), made for the run; remove it by hand")

    def killed(self) -> None:
        """What a runner killed during the run leaves: the list and the placeholders."""
        # Kept referenced: a collected generator would run its cleanup, which a killed runner never does.
        self.left = hold.placeholders(self.root, self.held, NAMES)
        self.left.__enter__()

    def test_a_killed_runs_placeholders_are_cleared(self):
        self.killed()
        (self.root / "makefile").unlink()
        hold.clear_leftovers(self.root, self.held)
        self.assertEqual(self.entries(), [])

    def test_a_changed_leftover_is_refused(self):
        self.killed()
        (self.root / "conftest.py").chmod(0o644)
        (self.root / "conftest.py").write_text("import os\n")
        with self.assertRaises(layout.SandboxError) as caught:
            hold.clear_leftovers(self.root, self.held)
        self.assertEqual(
            str(caught.exception),
            "refused: a killed sandbox run left placeholders in the checkout and conftest.py changed since; "
            f"review it, then remove it, or keep it and remove {self.held}")
        self.assertEqual(self.entries(), [".held.json", *sorted(NAMES)])

    def test_an_unreadable_list_is_refused(self):
        self.held.write_text('["Makefile"]')
        with self.assertRaises(layout.SandboxError) as caught:
            hold.clear_leftovers(self.root, self.held)
        self.assertEqual(
            str(caught.exception),
            f"{self.held} is not a list of held names; remove each root GNUmakefile, makefile or conftest.py that "
            "holds only a placeholder line, then that list")


class KilledRun(World):
    def kill_during_run(self, checkout: Path) -> Path:
        """Leaves what a runner killed during the run leaves; returns the list of held names."""
        held = storage.held_for(storage.cache_root({"HOME": str(self.home)}), checkout.resolve())
        held.parent.mkdir(parents=True, exist_ok=True)
        # Kept referenced: a collected generator would run its cleanup, which a killed runner never does.
        self.left = hold.placeholders(checkout, held, NAMES)
        self.left.__enter__()
        return held

    def test_the_next_start_clears_them(self):
        checkout = self.checkout()
        held = self.kill_during_run(checkout)
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            result.stderr.splitlines()[0],
            "sandbox: removed GNUmakefile, makefile, conftest.py of the placeholders (GNUmakefile, makefile, "
            "conftest.py) a killed sandbox run left in the checkout")
        self.assertIn("held in the run: GNUmakefile, makefile, conftest.py", result.stderr)
        self.assertFalse(held.exists())
        self.assertEqual([n for n in NAMES if os.path.lexists(checkout / n)], [])

    def test_a_changed_one_refuses_the_start(self):
        checkout = self.checkout()
        self.kill_during_run(checkout)
        (checkout / "GNUmakefile").chmod(0o644)
        (checkout / "GNUmakefile").write_text("all:\n")
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertTrue(result.stderr.startswith(
            "sandbox: refused: a killed sandbox run left placeholders in the checkout and GNUmakefile changed since"),
            result.stderr)
        self.assertFalse(self.runs())

    def test_host_make_says_what_left_them(self):
        checkout = make_repo_copy(self.tmp / "repo")
        self.kill_during_run(checkout)
        for goal in ((), ("help",), ("sandbox-clean",)):
            with self.subTest(goal=goal):
                result = self.make(checkout, *goal)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(
                    result.stderr,
                    "GNUmakefile:1: *** sandbox: a killed sandbox run left this placeholder GNUmakefile; remove it, "
                    "and any GNUmakefile, makefile or conftest.py at the root that holds only such a line, then "
                    "retry.  Stop.\n")


if __name__ == "__main__":
    unittest.main()
