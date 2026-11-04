"""Tests of `make sandbox-clean` and `make sandbox-check`: what each removes or runs, against a fake
`docker`."""

import subprocess
import sys
import unittest

from test_sandbox import SCRIPT, World, layout, make_repo_copy


class Clean(World):
    def storage(self, checkout) -> object:
        return self.home / ".cache" / "oxedi-sandbox" / layout.path_hash(checkout)

    def clean(self, checkout, scope: str, **extra: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--clean", scope, "--checkout", str(checkout)],
            env=self.env(**extra), capture_output=True, text=True, check=False, timeout=60,
        )

    def test_clean_removes_this_checkouts_storage_only(self):
        first, second = self.checkout("a"), self.checkout("b")
        for checkout in (first, second):
            self.assertEqual(self.sandbox(checkout).returncode, 0)
        locked = self.storage(first) / "venv" / "lib"
        locked.mkdir()
        (locked / "x").write_text("x")
        locked.chmod(0)
        result = self.clean(first, "checkout")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, f"sandbox: removed {self.storage(first)}\n")
        self.assertFalse(self.storage(first).exists())
        self.assertTrue(self.storage(second).is_dir())
        self.assertTrue((self.home / ".cache" / "oxedi-sandbox" / "cargo").is_dir())
        again = self.clean(first, "checkout")
        self.assertEqual(again.stderr, f"sandbox: nothing to remove at {self.storage(first)}\n")

    def test_clean_forgets_docs_venv(self):
        checkout = make_repo_copy(self.tmp / "repo")
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        docs = self.storage(checkout) / "venv-docs"
        self.assertTrue(docs.is_dir())
        (docs / "bin").mkdir()
        (docs / "bin" / "python").write_text("x")
        result = self.make(checkout, "sandbox-clean")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(docs.exists())
        self.assertFalse((self.storage(checkout) / "venv").exists())

    def test_clean_all_removes_every_checkout_and_the_caches(self):
        checkout = self.checkout()
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        cache = self.home / ".cache" / "oxedi-sandbox"
        result = self.clean(checkout, "all")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, f"sandbox: removed {cache}\n")
        self.assertFalse(cache.exists())
        self.assertTrue(self.home.is_dir())

    def test_clean_refused_while_a_container_runs(self):
        checkout = self.checkout()
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        name = layout.container_name(checkout)
        for scope, doomed in (("checkout", self.storage(checkout)), ("all", self.home / ".cache" / "oxedi-sandbox")):
            with self.subTest(scope=scope):
                result = self.clean(checkout, scope, FAKE_RUNNING=name)
                self.assertEqual(result.returncode, 2)
                self.assertEqual(
                    result.stderr,
                    f"sandbox: {name} is running and uses {doomed}; wait for it or stop it with docker kill\n",
                )
                self.assertTrue(doomed.is_dir())

    def test_sandbox_clean_through_make(self):
        cache = self.home / ".cache" / "oxedi-sandbox"
        cache.mkdir(parents=True)
        checkout = make_repo_copy(self.tmp / "repo")
        result = self.make(checkout, "sandbox-clean")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stderr, f"sandbox: nothing to remove at {cache / layout.path_hash(checkout)}\n")
        result = self.make(checkout, "sandbox-clean", "ALL=1")
        self.assertEqual(result.stderr, f"sandbox: removed {cache}\n")
        self.assertFalse(cache.exists())

    def test_sandbox_check_through_make(self):
        result = self.make(make_repo_copy(self.tmp / "repo"), "sandbox-check")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        argv = self.run_phase()
        self.assertEqual(argv[-1], f'mkdir -p "$HOME" && bash tools/sandbox/isolation-check.sh "{self.home}"')


if __name__ == "__main__":
    unittest.main()
