"""Tests of lock.py: one sandbox run per checkout storage, a second one refused naming the first,
the lock gone with a killed holder, `sandbox-clean` refused while a run holds it, and separate
storages (other worktrees, a sandbox-pr clone) locked on their own."""

import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from test_sandbox import SCRIPT, World, layout

import lock  # noqa: E402  (test_sandbox put tools/sandbox on sys.path)
import storage  # noqa: E402

HOLDER = """import subprocess, sys, time
from pathlib import Path
sys.path.insert(0, sys.argv[1])
from lock import RunLock
with RunLock(Path(sys.argv[2])):
    if len(sys.argv) > 3:
        child = subprocess.Popen(["sleep", "60"])
        print(child.pid, flush=True)
    print("held", flush=True)
    time.sleep(60)
"""


def hold(test, storage_dir: Path, spawn: bool = False) -> tuple[subprocess.Popen, list[str]]:
    """A child process holding the run lock of `storage_dir` until it is killed; with `spawn`, it
    also starts a `sleep` of its own first, as a runner starts docker clients. Returns the process
    and the lines it printed before `held`."""
    argv = [sys.executable, "-I", "-c", HOLDER, str(SCRIPT.parent), str(storage_dir), *(["spawn"] if spawn else [])]
    process = subprocess.Popen(argv, stdout=subprocess.PIPE, text=True)
    test.addCleanup(lambda: (process.kill(), process.wait(), process.stdout.close()))
    printed = []
    for line in process.stdout:
        if line.strip() == "held":
            return process, printed
        printed.append(line.strip())
    test.fail(f"the holder ended before it held the lock: {printed}")


def holder_text(storage_dir: Path, pid: int) -> str:
    record = json.loads(lock.lock_path(storage_dir).read_text())
    assert record["pid"] == pid, record
    return f"pid {pid}, since {record['since']}"


class RunLockTests(World):
    def test_second_runner_refused(self):
        checkout = self.checkout()
        holder, _ = hold(self, self.own(checkout))
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 2, result.stderr)
        self.assertEqual(
            result.stderr,
            f"sandbox: another sandbox run of this checkout is in progress ({holder_text(self.own(checkout), holder.pid)}); "
            "wait for it to end, or use another worktree\n",
        )
        self.assertEqual(self.calls(), [])
        for name in ("GNUmakefile", "makefile", "conftest.py"):
            self.assertFalse(os.path.lexists(checkout / name), name)
        self.assertFalse((self.own(checkout) / "held.json").exists())

    def test_the_lock_names_its_holder_while_held_and_lets_the_next_run_in(self):
        checkout = self.checkout()
        first = self.start(checkout, FAKE_RUN_SLEEP="2")
        self.wait_for_run()
        second = self.sandbox(checkout)
        out, err = first.communicate(timeout=60)
        self.assertEqual(first.returncode, 0, err)
        self.assertEqual(second.returncode, 2, second.stderr)
        self.assertRegex(second.stderr, rf"^sandbox: another sandbox run of this checkout is in progress "
                                        rf"\(pid {first.pid}, since \d{{4}}-\d\d-\d\dT\d\d:\d\d:\d\d[+-]\d\d:\d\d\); ")
        self.run_phase()
        third = self.sandbox(checkout)
        self.assertEqual(third.returncode, 0, third.stderr)

    def test_killed_holder_releases(self):
        checkout = self.checkout()
        holder, printed = hold(self, self.own(checkout), spawn=True)
        survivor = int(printed[0])
        self.addCleanup(lambda: os.kill(survivor, signal.SIGKILL))
        holder.send_signal(signal.SIGKILL)
        holder.wait(timeout=10)
        # The holder's own child outlives it, as a docker client may outlive a killed runner.
        os.kill(survivor, 0)
        with lock.RunLock(self.own(checkout)):
            pass
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_clean_refused_while_held(self):
        checkout = self.checkout()
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        own = self.own(checkout)
        holder, _ = hold(self, own)
        text = holder_text(own, holder.pid)
        for scope, whose in (("checkout", "a sandbox run of this checkout"),
                             ("all", f"a sandbox run that uses {own}")):
            with self.subTest(scope=scope):
                result = subprocess.run(
                    [sys.executable, "-I", str(SCRIPT), "--clean", scope, "--checkout", str(checkout)],
                    env=self.env(), capture_output=True, text=True, check=False, timeout=60,
                )
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stderr, f"sandbox-clean: {whose} is in progress ({text}); wait for it to end\n")
                self.assertTrue((own / "target").is_dir())

    def test_clean_takes_no_lock_where_none_was_made(self):
        checkout = self.checkout()
        cache = self.home / ".cache" / "oxedi-sandbox"
        (cache / "cargo").mkdir(parents=True)
        with lock.RunLock(self.own(checkout), create=False):
            self.assertFalse(self.own(checkout).exists())
        result = subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--clean", "all", "--checkout", str(checkout)],
            env=self.env(), capture_output=True, text=True, check=False, timeout=60,
        )
        self.assertEqual(result.stderr, f"sandbox: removed {cache}\n")

    def test_two_storages_independent(self):
        first, second = self.checkout("a"), self.checkout("b")
        holder, _ = hold(self, self.own(first))
        with self.assertRaises(lock.Busy) as caught, lock.RunLock(self.own(first)):
            pass
        self.assertEqual(caught.exception.holder, holder_text(self.own(first), holder.pid))
        with lock.RunLock(self.own(second)):
            pass
        result = self.sandbox(second)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_an_unreadable_holder_record_is_named(self):
        own = self.tmp / "own"
        holder, _ = hold(self, own)
        lock.lock_path(own).write_text("not json")
        with self.assertRaises(lock.Busy) as caught, lock.RunLock(own):
            pass
        self.assertTrue(caught.exception.holder.startswith(f"its pid and start time unreadable in {own / 'run.lock'}: "),
                        caught.exception.holder)
        self.assertEqual(holder.poll(), None)


class Release(unittest.TestCase):
    def test_a_released_lock_leaves_an_empty_record(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        own = Path(temporary.name) / "own"
        with lock.RunLock(own):
            self.assertNotEqual(lock.lock_path(own).read_text(), "")
        self.assertEqual(lock.lock_path(own).read_text(), "")

    def test_an_empty_record_is_named_unreadable(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        path = Path(temporary.name) / "run.lock"
        path.write_text("")
        self.assertTrue(lock.holder(path).startswith(f"its pid and start time unreadable in {path}: "))


class Renew(unittest.TestCase):
    def test_renew_keeps_the_held_lock(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        cache = Path(temporary.name) / "cache"
        review = cache / "prs" / "ref-x-0123456789ab"
        own = storage.own_dir(cache, review / "checkout")
        (own / "target").mkdir(parents=True)
        (review / "checkout").mkdir(parents=True)
        with lock.RunLock(own):
            storage.renew(cache, review)
            self.assertEqual(sorted(entry.name for entry in own.iterdir()), ["run.lock"])
            self.assertFalse(review.exists())
        storage.forget(cache, review)
        self.assertFalse(own.exists())


if __name__ == "__main__":
    unittest.main()
