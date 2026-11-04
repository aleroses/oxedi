"""Tests of the audit after a run, against a fake `docker` whose run-phase container writes into
the checkout (FAKE_RUN_WRITES)."""

import os
import signal
import subprocess
import unittest

from test_sandbox import GIT_ENV, World, make_repo_copy, mount_at

TRACKED = (".gitignore", "Makefile", "src/lib.rs", "crates/oxedi_py/python/oxedi/__init__.py")


class Audit(World):
    def test_audit_lists_and_exits_3(self):
        checkout = self.checkout("c", *TRACKED)
        writes = (
            "echo edit >> src/lib.rs && mkdir src/__pycache__ && echo pyc > src/__pycache__/x.cpython-313.pyc"
            " && echo so > crates/oxedi_py/python/oxedi/_core.abi3.so"
            f" && ln -s '{self.home / '.ssh'}' keys"
        )
        result = self.sandbox(checkout, FAKE_RUN_WRITES=writes)
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("sandbox: tracked file src/lib.rs changed while the sandbox ran\n", result.stderr)
        self.assertIn(
            f"sandbox: the run left a new path keys, a symbolic link to {self.home / '.ssh'} outside the checkout\n",
            result.stderr,
        )
        self.assertIn(
            "sandbox: removed the caches and the built extension the run may have left: "
            "crates/oxedi_py/python/oxedi/_core.abi3.so, src/__pycache__\n",
            result.stderr,
        )
        self.assertIn(
            "sandbox: exit 3: the run changed the checkout (changed files: 1, new paths: 1, unreadable: 0, "
            "not removed: 0); review them with git status and git diff before running anything from this checkout; "
            "the next sandbox run refuses to start until each is removed, reverted or added with git add\n",
            result.stderr,
        )
        self.assertFalse((checkout / "src" / "__pycache__").exists())
        self.assertFalse((checkout / "crates/oxedi_py/python/oxedi/_core.abi3.so").exists())
        self.assertTrue((checkout / "keys").is_symlink())
        self.assertTrue((self.home / ".ssh").is_dir())

    def test_audit_clean_run(self):
        checkout = self.checkout("c", *TRACKED)
        writes = "mkdir -p target/debug .venv/lib && echo x > target/debug/x && echo x > .venv/lib/x"
        result = self.sandbox(checkout, FAKE_RUN_WRITES=writes, FAKE_RUN_EXIT="5")
        self.assertEqual(result.returncode, 5, result.stderr)
        self.assertNotIn("sandbox: the run", result.stderr)
        self.assertNotIn("sandbox: removed", result.stderr)
        self.assertNotIn("exit 3", result.stderr)

    def test_new_paths_outside_the_outputs_exit_3(self):
        checkout = self.checkout("c", *TRACKED)
        writes = "mkdir .claude .cargo && echo x > .claude/settings.json && echo x > .cargo/config.toml"
        result = self.sandbox(checkout, FAKE_RUN_WRITES=writes)
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("sandbox: the run left a new path .cargo/config.toml\n", result.stderr)
        self.assertIn("sandbox: the run left a new path .claude/settings.json\n", result.stderr)
        self.assertNotIn("tracked file", result.stderr)

    def test_a_rewritten_gitignore_is_a_tracked_change(self):
        checkout = self.checkout("c", *TRACKED, ".venv-docs/bin/python")
        writes = "grep -v venv-docs .gitignore > g && cat g > .gitignore && rm g"
        result = self.sandbox(checkout, FAKE_RUN_WRITES=writes, FAKE_RUN_EXIT="1")
        self.assertEqual(mount_at(self.run_phase(), "/work/.venv-docs")["type"], "tmpfs")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("sandbox: tracked file .gitignore changed while the sandbox ran\n", result.stderr)
        # What the rewrite unignored shows as new already, before any later survey could unmask it.
        self.assertIn("sandbox: the run left a new path .venv-docs/bin/python\n", result.stderr)

    def test_a_tracked_file_turned_into_a_fifo_does_not_block(self):
        checkout = self.checkout("c", *TRACKED)
        result = self.sandbox(checkout, FAKE_RUN_WRITES="rm src/lib.rs && mkfifo src/lib.rs")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("sandbox: tracked file src/lib.rs changed while the sandbox ran\n", result.stderr)

    def test_an_output_that_cannot_be_removed_exits_3(self):
        checkout = self.checkout("c", *TRACKED)
        result = self.sandbox(checkout, FAKE_RUN_WRITES="mkdir src/__pycache__ && chmod 555 src")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("sandbox: cannot remove src/__pycache__ (Permission denied); remove it by hand\n", result.stderr)
        self.assertIn("not removed: 1", result.stderr)

    def test_an_unreadable_tracked_file_exits_3(self):
        checkout = self.checkout("c", *TRACKED)
        writes = "chmod 000 src/lib.rs && mkdir src/__pycache__ && echo pyc > src/__pycache__/x.pyc"
        result = self.sandbox(checkout, FAKE_RUN_WRITES=writes)
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn(
            "sandbox: unreadable: src/lib.rs (Permission denied); the audit cannot tell what the run did there\n",
            result.stderr,
        )
        self.assertNotIn("tracked file", result.stderr)
        self.assertIn("removed the caches and the built extension the run may have left: src/__pycache__",
                      result.stderr)
        self.assertFalse((checkout / "src" / "__pycache__").exists())

    def test_an_unreadable_new_directory_exits_3(self):
        checkout = self.checkout("c", *TRACKED)
        writes = "mkdir evil && echo x > evil/build.rs && mkdir evil/__pycache__ && chmod 000 evil"
        result = self.sandbox(checkout, FAKE_RUN_WRITES=writes)
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertNotIn("Traceback", result.stderr)
        self.assertIn(
            "sandbox: unreadable: git status: warning: could not open directory 'evil/': Permission denied; "
            "the audit cannot tell what the run did there\n",
            result.stderr,
        )
        self.assertIn(
            "sandbox: unreadable: evil (Permission denied); the audit cannot tell what the run did there\n",
            result.stderr,
        )
        self.assertIn("unreadable: 2,", result.stderr)

    def test_an_edited_untracked_file_exits_3(self):
        checkout = self.checkout("c", *TRACKED)
        (checkout / "draft.py").write_text("print('draft')\n")
        result = self.sandbox(checkout, FAKE_RUN_WRITES="echo evil >> draft.py")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("sandbox: untracked file draft.py changed while the sandbox ran\n", result.stderr)
        self.assertNotIn("the run left a new path", result.stderr)

    def test_an_untouched_untracked_file_is_no_finding(self):
        checkout = self.checkout("c", *TRACKED)
        (checkout / "draft.py").write_text("print('draft')\n")
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("sandbox: the run", result.stderr)

    def test_audit_after_a_stop_signal(self):
        for signum in (signal.SIGINT, signal.SIGTERM):
            with self.subTest(signal=signum.name):
                checkout = self.checkout(f"c-{signum.name}", *TRACKED)
                (self.fake / "run.pid").unlink(missing_ok=True)
                process = self.start(checkout, FAKE_RUN_WRITES="echo edit >> src/lib.rs", FAKE_RUN_SLEEP="30")
                self.wait_for_run()
                process.send_signal(signum)
                _, err = process.communicate(timeout=20)
                self.assertEqual(process.returncode, 128 + signum)
                self.assertIn("sandbox: tracked file src/lib.rs changed while the sandbox ran\n", err)

    def planted(self, name: str) -> str:
        """A module that leaves a marker on the host when it is imported."""
        return f"printf '%s\\n' 'open(\"{self.tmp / name}\", \"w\").close()' > hashlib.py"

    def test_a_path_host_tools_look_up_is_named_first(self):
        checkout = self.checkout("c", *TRACKED)
        # The run cannot create a root GNUmakefile (it is held read-only), so an .envrc stands for it.
        writes = "echo x > .envrc && echo x > hashlib.py && mkdir -p crates/x && echo x > crates/x/conftest.py"
        result = self.sandbox(checkout, FAKE_RUN_WRITES=writes)
        self.assertEqual(result.returncode, 3, result.stderr)
        warning = (
            "sandbox: do not run make or any tool in this checkout until you remove .envrc, crates/x/conftest.py, "
            "hashlib.py: host tools load them by name (make reads GNUmakefile and makefile before Makefile)"
        )
        self.assertIn(warning + "\n", result.stderr)
        after_run = result.stderr[result.stderr.index("sandbox: run took"):].splitlines()
        self.assertEqual(after_run[1], warning)
        self.assertIn("sandbox: the run left a new path .envrc\n", result.stderr)

    def test_a_dirty_audit_blocks_the_next_start_until_the_paths_go(self):
        checkout = self.checkout("c", *TRACKED)
        writes = "echo edit >> src/lib.rs && echo x > .envrc && " + self.planted("marker")
        self.assertEqual(self.sandbox(checkout, FAKE_RUN_WRITES=writes).returncode, 3)
        runs = len(self.runs())
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(
            result.stderr,
            "sandbox: refused: the last run left .envrc, hashlib.py, src/lib.rs, still as it left them; "
            "review them with git status and git diff, then remove or revert each one, or add it with git add to "
            "keep it (make sandbox-clean forgets the record, with the rest of this checkout's storage)\n",
        )
        self.assertEqual(len(self.runs()), runs)
        (checkout / ".envrc").unlink()
        (checkout / "hashlib.py").write_text("# reviewed by hand\n")
        subprocess.run(["git", "-C", str(checkout), "add", "src/lib.rs"], env=dict(os.environ, **GIT_ENV), check=True)
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(list((self.home / ".cache" / "oxedi-sandbox").glob("*/audit.json")), [])
        self.assertFalse((self.tmp / "marker").exists())

    def test_an_unreadable_record_refuses(self):
        checkout = self.checkout("c", *TRACKED)
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        [own] = [p for p in (self.home / ".cache" / "oxedi-sandbox").iterdir() if p.name not in ("cargo", "uv")]
        (own / "audit.json").write_text("[1]")
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(
            result.stderr,
            f"sandbox: the audit record {own / 'audit.json'} is not a map of paths to fingerprints; remove it once "
            "you have reviewed the checkout with git status and git diff\n",
        )

    def test_a_planted_module_never_runs_on_the_host_through_make(self):
        checkout = make_repo_copy(self.tmp / "repo")
        first = self.make(checkout, "sandbox", "CMD=true", FAKE_RUN_WRITES=self.planted("marker"))
        self.assertEqual(first.returncode, 2, first.stderr)
        self.assertIn("sandbox: do not run make or any tool in this checkout until you remove hashlib.py:", first.stderr)
        self.assertIn("Error 3", first.stderr)
        second = self.make(checkout, "sandbox", "CMD=true")
        self.assertEqual(second.returncode, 2, second.stderr)
        # The host make stops at the guard, before the runner starts.
        self.assertIn("*** make: a sandbox run left hashlib.py unreviewed;", second.stderr)
        self.assertFalse((self.tmp / "marker").exists())

    def test_a_module_in_the_checkout_never_shadows_the_runners(self):
        checkout = make_repo_copy(self.tmp / "repo")
        for name in ("hashlib", "pathlib", "json"):
            (checkout / f"{name}.py").write_text(f'open("{self.tmp / name}", "w").close()\n')
        result = self.make(checkout, "sandbox", "CMD=true", PYTHONPATH=str(checkout))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual([n for n in ("hashlib", "pathlib", "json") if (self.tmp / n).exists()], [])


if __name__ == "__main__":
    unittest.main()
