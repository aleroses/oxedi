"""Tests of source.py: the request variables, the GitHub URL, and the clone of a branch or a pull
request, on temporary repositories and a temporary home only (no network)."""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import source  # noqa: E402
from layout import SandboxError  # noqa: E402
from source import Change, Request  # noqa: E402

IDENTITY = {
    "GIT_AUTHOR_NAME": "t", "GIT_AUTHOR_EMAIL": "t@example.com",
    "GIT_COMMITTER_NAME": "t", "GIT_COMMITTER_EMAIL": "t@example.com",
}
ISOLATED = {"GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1", **IDENTITY}
# Lets the tests serve a pull request from a local bare repository.
FILE_TRANSPORT = ("-c", "protocol.file.allow=always")


def git(root: Path, *args: str, env: dict[str, str] = ISOLATED) -> str:
    result = subprocess.run(["git", "-C", str(root), *args], env=dict(os.environ, **env), capture_output=True,
                            text=True, check=True)
    return result.stdout.strip()


def commit(root: Path, path: str, text: str) -> str:
    (root / path).write_text(text)
    git(root, "add", path)
    git(root, "commit", "-q", "-m", path)
    return git(root, "rev-parse", "HEAD")


def new_repo(root: Path) -> Path:
    root.mkdir(parents=True)
    git(root, "init", "-q", "-b", "master")
    return root


class Requests(unittest.TestCase):
    def test_request_rejects(self):
        cases = [
            ({"SANDBOX_PR": "12a"}, 'PR must be a pull request number, 1 to 999999999, got "12a"'),
            ({"SANDBOX_PR": "0"}, 'PR must be a pull request number, 1 to 999999999, got "0"'),
            ({"SANDBOX_PR": "-1"}, 'PR must be a pull request number, 1 to 999999999, got "-1"'),
            ({"SANDBOX_PR": "1234567890"}, 'PR must be a pull request number, 1 to 999999999, got "1234567890"'),
            ({"SANDBOX_PR": ""}, "set PR=<number> or REF=<name>, got neither"),
            ({"SANDBOX_PR": "7", "SANDBOX_REF": "feature"}, 'set PR or REF, not both: got PR="7" and REF="feature"'),
            ({"SANDBOX_REF": "--upload-pack=x"}, 'REF must not start with "-", got "--upload-pack=x"'),
            ({"SANDBOX_REF": "a..b"}, 'REF is not a valid git ref name, got "a..b"'),
            ({"SANDBOX_REF": "feature", "SANDBOX_HEAD": "1"},
             'HEAD=1 goes with PR, not REF: got HEAD="1" and REF="feature"'),
            ({"SANDBOX_PR": "7", "SANDBOX_HEAD": "yes"}, 'HEAD must be 1 or unset, got "yes"'),
        ]
        for env, message in cases:
            with self.subTest(env=env), self.assertRaises(SandboxError) as caught:
                Request.parse(env)
            self.assertEqual(str(caught.exception), message)

    def test_a_ref_that_reads_as_an_option_never_reaches_git(self):
        with mock.patch.object(source.subprocess, "run") as run, self.assertRaises(SandboxError):
            Request.parse({"SANDBOX_REF": "--upload-pack=x"})
        run.assert_not_called()

    def test_request_accepts(self):
        self.assertEqual(Request.parse({"SANDBOX_PR": "7"}), Request("pr", "7", False))
        self.assertEqual(Request.parse({"SANDBOX_PR": "999999999", "SANDBOX_HEAD": "1"}),
                         Request("pr", "999999999", True))
        self.assertEqual(Request.parse({"SANDBOX_REF": "feature/x", "SANDBOX_PR": ""}),
                         Request("ref", "feature/x", False))

    def test_labels(self):
        self.assertEqual(Request("pr", "7", True).label, "pr7")
        self.assertEqual(Request("ref", "feature/añ_1.x", False).label, "ref-feature-a--_1.x")


class GitHub(unittest.TestCase):
    def test_redacts_to_the_last_at(self):
        for text, expected in (
            ("https://u:p@ss@github.com/o/r.git", "https://<redacted>@github.com/o/r.git"),
            ("https://x-access-token:ghs_secret@github.com/o/r.git", "https://<redacted>@github.com/o/r.git"),
            ("ssh://git@github.com/o/r", "ssh://<redacted>@github.com/o/r"),
            ("see https://a:b@c@host/p@th and https://u@h/x", "see https://<redacted>@host/p@th and https://<redacted>@h/x"),
            ("https://github.com/o/r@v1", "https://github.com/o/r@v1"),
            ("no url, user@host only", "no url, user@host only"),
        ):
            with self.subTest(text=text):
                self.assertEqual(source.redacted(text), expected)

    def test_github_https_forms(self):
        for url in ("git@github.com:javillegasna/oxedi.git", "ssh://git@github.com/javillegasna/oxedi",
                    "https://github.com/javillegasna/oxedi.git", "https://github.com/javillegasna/oxedi"):
            with self.subTest(url=url):
                self.assertEqual(source.github_https(url), "https://github.com/javillegasna/oxedi.git")
        with self.assertRaises(SandboxError) as caught:
            source.github_https("https://x-access-token:ghs_secret@github.com/o/r.git")
        self.assertIn('origin is "https://<redacted>@github.com/o/r.git"', str(caught.exception))
        self.assertNotIn("ghs_secret", str(caught.exception))
        for url in ("git@gitlab.com:o/r.git", "file:///x", "https://github.com/o/r/extra",
                    "https://github.com/o/..", "ext::sh -c touch% /tmp/x"):
            with self.subTest(url=url), self.assertRaises(SandboxError) as caught:
                source.github_https(url)
            self.assertIn(f'origin is "{url}"; sandbox-pr fetches pull requests only from a GitHub origin',
                          str(caught.exception))


class Clones(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)
        isolated = mock.patch.dict(os.environ, ISOLATED)
        isolated.start()
        self.addCleanup(isolated.stop)

    def test_ref_mode_on_a_local_repo(self):
        host = new_repo(self.tmp / "host")
        master = commit(host, "readme.txt", "r\n")
        git(host, "checkout", "-q", "-b", "feature")
        commit(host, "feature.txt", "f\n")
        feature = commit(host, "feature.txt", "f2\n")
        git(host, "checkout", "-q", "master")
        into = self.tmp / "clone"
        change = source.clone(host, Request.parse({"SANDBOX_REF": "feature"}), into)
        self.assertEqual(change, Change(kind="ref", label="ref-feature", tested=feature, base=master, head=feature,
                                        merged=False))
        self.assertEqual((into / "feature.txt").read_text(), "f2\n")
        self.assertEqual(git(into, "rev-parse", "HEAD"), feature)

    def test_ref_that_names_no_commit_is_refused_with_its_name(self):
        host = new_repo(self.tmp / "host")
        commit(host, "readme.txt", "r\n")
        with self.assertRaises(SandboxError) as caught:
            source.clone(host, Request.parse({"SANDBOX_REF": "nothing"}), self.tmp / "clone")
        self.assertEqual(str(caught.exception), f'"nothing" names no commit in {host}')

    def served(self) -> tuple[Path, Path, dict[str, str]]:
        """A host checkout with master only, and a bare repository serving pull request 7: its head
        (two commits on master) and its merge into a master that moved on."""
        author = new_repo(self.tmp / "author")
        root = commit(author, "readme.txt", "r\n")
        host = self.tmp / "host"
        git(self.tmp, "clone", "-q", str(author), str(host))
        git(host, "remote", "set-url", "origin", "git@github.com:javillegasna/oxedi.git")
        git(author, "checkout", "-q", "-b", "pr")
        commit(author, "pr.txt", "p\n")
        head = commit(author, "pr.txt", "p2\n")
        git(author, "checkout", "-q", "master")
        moved = commit(author, "other.txt", "o\n")
        git(author, "merge", "-q", "--no-ff", "-m", "merge", "pr")
        merge = git(author, "rev-parse", "HEAD")
        served = self.tmp / "served.git"
        git(self.tmp, "init", "-q", "--bare", str(served))
        git(author, "push", "-q", str(served), f"{head}:refs/pull/7/head", f"{merge}:refs/pull/7/merge")
        return host, served, {"root": root, "head": head, "moved": moved, "merge": merge}

    def clone_pr(self, host: Path, served: Path, env: dict[str, str]) -> Change:
        with mock.patch.object(source, "github_https", return_value=str(served)), \
                mock.patch.object(source, "TEST_OPTIONS", FILE_TRANSPORT):
            return source.clone(host, Request.parse(env), self.tmp / "clone")

    def test_pr_mode_from_a_served_repo(self):
        host, served, sha = self.served()
        change = self.clone_pr(host, served, {"SANDBOX_PR": "7"})
        self.assertEqual(change, Change(kind="pr", label="pr7", tested=sha["merge"], base=sha["moved"],
                                        head=sha["head"], merged=True))
        self.assertEqual((self.tmp / "clone" / "other.txt").read_text(), "o\n")
        self.assertEqual((self.tmp / "clone" / "pr.txt").read_text(), "p2\n")

    def test_a_merge_ref_of_an_older_head_is_refused_with_both_commits(self):
        host, served, sha = self.served()
        author = self.tmp / "author"
        git(author, "checkout", "-q", "pr")
        newer = commit(author, "pr.txt", "p3\n")
        git(author, "push", "-q", "-f", str(served), f"{newer}:refs/pull/7/head")
        with self.assertRaises(SandboxError) as caught:
            self.clone_pr(host, served, {"SANDBOX_PR": "7"})
        self.assertEqual(
            str(caught.exception),
            f"refs/pull/7/merge merges {sha['head']}, but refs/pull/7/head is {newer}: "
            "GitHub has not yet recomputed the merge after a push; run it again in a minute",
        )

    def test_pr_without_a_merge_ref_is_refused(self):
        host, served, _ = self.served()
        git(served, "update-ref", "-d", "refs/pull/7/merge")
        with self.assertRaises(SandboxError) as caught:
            self.clone_pr(host, served, {"SANDBOX_PR": "7"})
        self.assertEqual(
            str(caught.exception),
            "pull request 7 has no merge ref refs/pull/7/merge: it has conflicts or is closed; "
            "set HEAD=1 to test refs/pull/7/head instead",
        )

    def test_pr_head_mode(self):
        host, served, sha = self.served()
        git(served, "update-ref", "-d", "refs/pull/7/merge")
        change = self.clone_pr(host, served, {"SANDBOX_PR": "7", "SANDBOX_HEAD": "1"})
        self.assertEqual(change, Change(kind="pr", label="pr7", tested=sha["head"], base=sha["root"],
                                        head=sha["head"], merged=False))
        self.assertFalse((self.tmp / "clone" / "other.txt").exists())

    def test_a_missing_ref_is_told_apart_under_any_locale(self):
        host, served, _ = self.served()
        git(served, "update-ref", "-d", "refs/pull/7/merge")
        spanish = {"LANGUAGE": "es", "LANG": "es_ES.UTF-8", "LC_ALL": "es_ES.UTF-8", "LC_MESSAGES": "es_ES.UTF-8"}
        with mock.patch.dict(os.environ, spanish):
            for env, message in (({"SANDBOX_PR": "7"}, "pull request 7 has no merge ref refs/pull/7/merge: it has "
                                  "conflicts or is closed; set HEAD=1 to test refs/pull/7/head instead"),
                                 ({"SANDBOX_PR": "8"}, f"pull request 8 not found at {served}")):
                with self.subTest(env=env), self.assertRaises(SandboxError) as caught:
                    self.clone_pr(host, served, env)
                self.assertEqual(str(caught.exception), message)
                shutil.rmtree(self.tmp / "clone")
        with mock.patch.dict(os.environ, spanish):
            self.assertEqual((source.git_environment()["LC_ALL"], source.git_environment()["LANGUAGE"]), ("C", ""))

    def test_pr_that_does_not_exist_is_refused(self):
        host, served, _ = self.served()
        with self.assertRaises(SandboxError) as caught:
            self.clone_pr(host, served, {"SANDBOX_PR": "8"})
        self.assertEqual(str(caught.exception), f"pull request 8 not found at {served}")

    def test_the_file_transport_stays_refused_for_the_pull_request(self):
        host, served, _ = self.served()
        with mock.patch.object(source, "github_https", return_value=str(served)), \
                self.assertRaises(SandboxError) as caught:
            source.clone(host, Request.parse({"SANDBOX_PR": "7"}), self.tmp / "clone")
        self.assertIn("transport 'file' not allowed", str(caught.exception))

    def test_no_host_filter_or_hook_runs(self):
        home = self.tmp / "home"
        hooks = home / "hooks"
        hooks.mkdir(parents=True)
        filter_canary, hook_canary = self.tmp / "filter-ran", self.tmp / "hook-ran"
        (hooks / "post-checkout").write_text(f"#!/bin/sh\ntouch '{hook_canary}'\n")
        (hooks / "post-checkout").chmod(0o755)
        (home / ".gitconfig").write_text(
            f"[filter \"evil\"]\n\tsmudge = touch '{filter_canary}' && cat\n\trequired = true\n"
            f"[core]\n\thooksPath = {hooks}\n"
        )
        host = new_repo(self.tmp / "host")
        commit(host, "readme.txt", "r\n")
        git(host, "checkout", "-q", "-b", "feature")
        commit(host, ".gitattributes", "* filter=evil\n")
        commit(host, "feature.txt", "f\n")
        git(host, "checkout", "-q", "master")
        with mock.patch.dict(os.environ, {"HOME": str(home)}):
            source.clone(host, Request.parse({"SANDBOX_REF": "feature"}), self.tmp / "clone")
        self.assertFalse(filter_canary.exists())
        self.assertFalse(hook_canary.exists())
        self.assertEqual((self.tmp / "clone" / "feature.txt").read_text(), "f\n")
        # The same checkout with the host's configuration runs both: the canaries can fire.
        unisolated = {k: v for k, v in os.environ.items() if k not in ("GIT_CONFIG_GLOBAL", "GIT_CONFIG_NOSYSTEM")}
        control = self.tmp / "control"
        subprocess.run(["git", "clone", "-q", "-b", "feature", str(host), str(control)],
                       env=dict(unisolated, HOME=str(home)), capture_output=True, check=True)
        self.assertTrue(filter_canary.exists())
        self.assertTrue(hook_canary.exists())


if __name__ == "__main__":
    unittest.main()
