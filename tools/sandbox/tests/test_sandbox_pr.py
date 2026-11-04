"""Tests of sandbox_pr.py: the whole flow against a fake `docker` (fake_docker.py), over temporary
repositories and a temporary home only, so the order of the containers, every confirmation, the
reports and every refusal are proven on what docker would receive."""

import io
import json
import os
import signal
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
import sandbox_pr  # noqa: E402
import source  # noqa: E402
from test_sandbox import make_repo_copy  # noqa: E402
from test_source import FILE_TRANSPORT, commit, git, new_repo  # noqa: E402

IMAGE = "oxedi-sandbox:test"
HOST_DENY = str(HERE.parent / "deny.toml")


class Stdin:
    """A standard input that is a terminal or not, answering `answer` when read."""

    def __init__(self, tty: bool, answer: str = ""):
        self.tty = tty
        self.answer = answer
        self.reads = 0

    def isatty(self) -> bool:
        return self.tty

    def readline(self) -> str:
        self.reads += 1
        return self.answer


def kind(call: dict) -> str:
    """Which container a `docker run` was: deny, pip-audit, run or fetch."""
    argv = call["argv"]
    for name in ("cargo-deny", "pip-audit", "bash"):
        if name in argv:
            return {"bash": "run", "cargo-deny": "deny"}.get(name, name)
    return "fetch"


class Flow(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)
        self.home = self.tmp / "home"
        self.home.mkdir()
        self.fake = self.tmp / "fake"
        self.fake.mkdir()
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        shim = self.bin / "docker"
        shim.write_text(f'#!/bin/sh\nexec "{sys.executable}" -I "{HERE / "fake_docker.py"}" "$@"\n')
        shim.chmod(0o755)
        handlers = {signum: signal.getsignal(signum) for signum in sandbox_pr.STOP_SIGNALS}
        self.addCleanup(lambda: [signal.signal(signum, handler) for signum, handler in handlers.items()])
        self.host = new_repo(self.tmp / "host")
        (self.host / "tools" / "sandbox").mkdir(parents=True)
        (self.host / "crates" / "oxedi_core" / "src").mkdir(parents=True)
        (self.host / ".gitignore").write_text("/target/\n/.venv/\n")
        git(self.host, "add", ".gitignore")
        commit(self.host, "tools/sandbox/deny.toml", "[bans]\n")
        commit(self.host, "crates/oxedi_core/src/lib.rs", "pub fn f() {}\n")
        git(self.host, "checkout", "-q", "-b", "feature")

    def change(self, path: str, text: str = "x\n") -> None:
        """Commits `path` on the feature branch."""
        (self.host / path).parent.mkdir(parents=True, exist_ok=True)
        commit(self.host, path, text)

    def env(self, **extra: str) -> dict[str, str]:
        return {"PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}", "HOME": str(self.home),
                "FAKE_DIR": str(self.fake), "SANDBOX_REF": "feature", "SANDBOX_TARGET": "make gates", **extra}

    def review(self, tty: bool = False, answer: str = "", kind: str = "base", **extra: str) -> tuple[int, str, str]:
        """Runs sandbox_pr.main in this process with only `env` as the environment and a standard
        input that is a terminal answering `answer` when `tty`; returns its status, stdout and
        stderr. The standard input is kept as `self.stdin`."""
        self.stdin = stdin = Stdin(tty, answer)
        err = io.TextIOWrapper(io.BytesIO(), write_through=True)
        out = io.StringIO()
        with (mock.patch.dict(os.environ, self.env(**extra), clear=True),
              mock.patch.object(sys, "stdin", stdin), mock.patch.object(sys, "stdout", out),
              mock.patch.object(sys, "stderr", err)):
            status = sandbox_pr.main(["--image", IMAGE, "--kind", kind, "--checkout", str(self.host)])
        return status, out.getvalue(), err.buffer.getvalue().decode()

    def calls(self) -> list[dict]:
        log = self.fake / "calls.jsonl"
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    def runs(self) -> list[dict]:
        return [call for call in self.calls() if call["argv"][:1] == ["run"]]

    def kinds(self) -> list[str]:
        return [kind(call) for call in self.runs()]

    def reviews(self) -> Path:
        return self.home / ".cache" / "oxedi-sandbox" / "prs"

    def report(self) -> dict:
        found = sorted(self.reviews().glob("ref-feature-*/report.json"))
        self.assertEqual(len(found), 1, found)
        return json.loads(found[0].read_text())

    def test_flagged_without_tty_stops_with_4(self):
        self.change("crates/oxedi_core/build.rs", "fn main() {}\n")
        status, out, err = self.review()
        self.assertEqual(status, 4, err)
        self.assertEqual(self.kinds(), ["deny"])
        self.assertIn(
            "sandbox-pr: the change touches what the report flags; there is no terminal to ask, so it stops with "
            "exit 4: read the report, then set CONFIRM=yes to go on\n", err)
        self.assertIn("  A crates/oxedi_core/build.rs\n      BUILD_SCRIPT: a build script", out)
        report = self.report()
        self.assertEqual((report["confirmed"], report["run"], report["pip_audit"]), ("refused", None, []))
        self.assertTrue(report["flagged"])
        self.assertEqual(report["touched"], [{"path": "crates/oxedi_core/build.rs", "status": "A",
                                              "kinds": ["BUILD_SCRIPT"]}])
        self.assertEqual(report["change"]["kind"], "ref")
        self.assertEqual(report["cargo_deny"], {"status": 0, "output": ""})

    def test_confirm_env_runs(self):
        self.change("crates/oxedi_core/build.rs", "fn main() {}\n")
        status, _, err = self.review(SANDBOX_CONFIRM="yes", FAKE_PIP_AUDIT_OUTPUT='{"dependencies": []}')
        self.assertEqual(status, 0, err)
        self.assertEqual(self.kinds(), ["deny", "fetch", "fetch", "fetch", "pip-audit", "run"])
        self.assertEqual(self.runs()[-1]["argv"][-1], 'mkdir -p "$HOME" && make gates')
        report = self.report()
        self.assertEqual(report["confirmed"], "env")
        self.assertEqual(report["pip_audit"], [{"venv": "/work/.venv", "status": 0, "output": '{"dependencies": []}'}])
        self.assertEqual(report["run"], {"target": "make gates", "status": 0, "audit": []})
        self.assertEqual((report["image"], report["runtime"]), (IMAGE, "runc"))
        text = (self.reviews() / f"ref-feature-{report['change']['tested'][:12]}" / "report.txt").read_text()
        self.assertIn("confirmed: env\nrun: make gates: exit 0\n  audit: clean\n", text)

    def test_audit_commands_and_mounts(self):
        self.change("crates/oxedi_core/src/lib.rs", "pub fn g() {}\n")
        self.review()
        deny, pip = [call["argv"] for call in self.runs() if kind(call) in ("deny", "pip-audit")]
        self.assertEqual(deny[deny.index(IMAGE) + 1:], [
            "cargo-deny", "--manifest-path", "/work/Cargo.toml", "check", "--config", "/sandbox/deny.toml",
            "advisories", "sources", "bans"])
        self.assertEqual(pip[pip.index(IMAGE) + 1:], [
            "pip-audit", "--progress-spinner", "off", "--format", "json", "--path",
            "/work/.venv/lib/python3.13/site-packages"])
        for argv in (deny, pip):
            specs = [b for a, b in zip(argv, argv[1:]) if a == "--mount"]
            self.assertIn(f"type=bind,src={self.home}/.cache/oxedi-sandbox/advisory,dst=/sandbox/advisory", specs)
            self.assertTrue([s for s in specs if s.endswith("dst=/work/.venv,readonly")], specs)
            self.assertIn("type=bind,src=" + str(self.reviews().resolve() / Path(*self.clone_parts())) +
                          ",dst=/work,readonly", specs)
            self.assertNotIn("--network", argv)

    def clone_parts(self) -> tuple[str, str]:
        report = self.report()
        return f"ref-feature-{report['change']['tested'][:12]}", "checkout"

    def test_tty_answer(self):
        self.change("crates/oxedi_core/build.rs", "fn main() {}\n")
        status, _, err = self.review(True, "n\n")
        self.assertEqual(status, 4, err)
        self.assertIn("sandbox-pr: the change touches what the report flags. Go on? [y/N] ", err)
        self.assertIn('sandbox-pr: not confirmed (answer "n"), so it stops with exit 4\n', err)
        self.assertEqual(self.kinds(), ["deny"])
        self.assertEqual(self.report()["confirmed"], "refused")
        (self.fake / "calls.jsonl").unlink()
        status, _, err = self.review(True, "yes\n")
        self.assertEqual(status, 0, err)
        self.assertEqual(self.kinds(), ["deny", "fetch", "fetch", "fetch", "pip-audit", "run"])
        self.assertEqual(self.report()["confirmed"], "tty")

    def test_unflagged_runs_without_asking(self):
        self.change("crates/oxedi_core/src/lib.rs", "pub fn g() {}\n")
        status, out, err = self.review(True, "n\n")
        self.assertEqual(status, 0, err)
        self.assertEqual(self.stdin.reads, 0)
        self.assertIn("flagged: no\n", out)
        report = self.report()
        self.assertEqual((report["confirmed"], report["flagged"]), ("not needed", False))
        self.assertEqual(report["touched"], [{"path": "crates/oxedi_core/src/lib.rs", "status": "M", "kinds": []}])

    def test_a_failed_pip_audit_needs_the_second_confirmation(self):
        self.change("crates/oxedi_core/src/lib.rs", "pub fn g() {}\n")
        status, out, err = self.review(FAKE_PIP_AUDIT_EXIT="1", FAKE_PIP_AUDIT_OUTPUT='{"vulns": 1}')
        self.assertEqual(status, 4, err)
        self.assertEqual(self.kinds(), ["deny", "fetch", "fetch", "fetch", "pip-audit"])
        self.assertIn("sandbox-pr: pip-audit failed; there is no terminal to ask", err)
        self.assertIn('pip-audit /work/.venv: exit 1\n  {"vulns": 1}\n', out)
        report = self.report()
        self.assertEqual((report["confirmed"], report["run"]), ("refused", None))
        self.assertEqual(report["pip_audit"], [{"venv": "/work/.venv", "status": 1, "output": '{"vulns": 1}'}])

    def test_a_failed_cargo_deny_needs_the_first_confirmation(self):
        self.change("crates/oxedi_core/src/lib.rs", "pub fn g() {}\n")
        status, out, err = self.review(FAKE_DENY_EXIT="1", FAKE_DENY_OUTPUT="error[vulnerability]: x\n")
        self.assertEqual(status, 4, err)
        self.assertEqual(self.kinds(), ["deny"])
        self.assertIn("sandbox-pr: cargo-deny failed; there is no terminal to ask", err)
        self.assertIn("cargo-deny: exit 1\n  error[vulnerability]: x\n", out)
        self.assertEqual(self.report()["cargo_deny"], {"status": 1, "output": "error[vulnerability]: x\n"})

    def test_a_failed_fetch_ends_with_its_status(self):
        self.change("crates/oxedi_core/src/lib.rs", "pub fn g() {}\n")
        status, _, err = self.review(FAKE_FETCH_EXIT="5")
        self.assertEqual(status, 5, err)
        self.assertEqual(self.kinds(), ["deny", "fetch"])
        report = self.report()
        self.assertEqual((report["confirmed"], report["pip_audit"], report["run"]), ("not needed", [], None))

    def test_viewer_audits_both_venvs(self):
        self.change("docs/requirements.txt", "playwright==1.64.0 \\\n    --hash=sha256:00\n")
        self.change("crates/oxedi_wasm/scripts/install_tools.sh",
                    'WASM_BINDGEN_VERSION="0.2.129"\nWASM_BINDGEN_SHA256="aa"\nBINARYEN_VERSION="133"\nBINARYEN_SHA256="bb"\n')
        self.change("viewer/app.js")
        status, out, err = self.review(kind="viewer", SANDBOX_CONFIRM="yes", FAKE_PLAYWRIGHT_LABEL="1.64.0",
                                       FAKE_WASM_BINDGEN_LABEL="0.2.129", FAKE_BINARYEN_LABEL="133",
                                       FAKE_PIP_AUDIT_OUTPUT='{"dependencies": []}')
        self.assertEqual(status, 0, err)
        self.assertEqual(self.kinds(), ["deny", *["fetch"] * 6, "pip-audit", "pip-audit", "run"])
        self.assertIn("pip-audit /work/.venv-docs: exit 0\n", out)
        report = self.report()
        self.assertEqual([audit["venv"] for audit in report["pip_audit"]], ["/work/.venv", "/work/.venv-docs"])
        self.assertEqual(report["image_hint"], "")
        self.assertNotIn("rerun with IMAGE=viewer", out)
        pip = [call["argv"] for call in self.runs() if kind(call) == "pip-audit"]
        self.assertTrue(pip[1][-1].startswith("/work/.venv-docs/"))
        specs = [b for a, b in zip(pip[1], pip[1][1:]) if a == "--mount"]
        self.assertTrue([s for s in specs if s.endswith("dst=/work/.venv-docs,readonly")], specs)

    def test_a_viewer_change_reviewed_with_the_base_image_says_to_rerun(self):
        self.change("viewer/app.js")
        hint = "this change touches the viewer; rerun with IMAGE=viewer to run its tests"
        status, out, err = self.review(SANDBOX_CONFIRM="yes")
        self.assertEqual(status, 0, err)
        # The terminal shows it with the risk report, before the first confirmation.
        risk_report = out.split("cargo-deny: exit")[0]
        self.assertTrue(risk_report.endswith(f"flagged: yes\n{hint}\n"), out)
        report = self.report()
        self.assertEqual(report["image_hint"], hint)
        text = (self.reviews() / f"ref-feature-{report['change']['tested'][:12]}" / "report.txt").read_text()
        self.assertTrue(text.endswith(f"\n{hint}\n"), text)

    def test_flow_asks_before_deny(self):
        self.change("Cargo.toml", '[package]\nname = "w"\n\n[patch.crates-io]\nx = { git = "https://evil/x" }\n')
        status, out, err = self.review()
        self.assertEqual(status, 4, err)
        self.assertEqual(self.runs(), [])
        self.assertIn("sandbox-pr: the change adds a cargo source: https://evil/x\n", err)
        self.assertIn(
            "sandbox-pr: cargo-deny would fetch from a source the change adds; there is no terminal to ask, so it "
            "stops with exit 4: read the report, then set CONFIRM=yes to go on\n", err)
        self.assertIn("new cargo sources:\n  https://evil/x\n", out)
        self.assertIn("cargo-deny: not run\n", self.review_text())
        report = self.report()
        self.assertEqual((report["new_sources"], report["confirmed"]), (["https://evil/x"], "refused"))
        self.assertEqual((report["cargo_deny"], report["pip_audit"], report["run"]), (None, [], None))
        (self.fake / "calls.jsonl").unlink()
        status, _, err = self.review(SANDBOX_CONFIRM="yes")
        self.assertEqual(status, 0, err)
        self.assertEqual(self.kinds(), ["deny", "fetch", "fetch", "fetch", "pip-audit", "run"])
        self.assertEqual(self.report()["confirmed"], "env")

    def test_no_new_source_asks_nothing_before_deny(self):
        self.change("crates/oxedi_core/src/lib.rs", "pub fn g() {}\n")
        status, out, err = self.review()
        self.assertEqual(status, 0, err)
        self.assertNotIn("cargo source", err)
        self.assertIn("new cargo sources:\n  none\n", out)
        self.assertEqual(self.report()["new_sources"], [])

    def review_text(self) -> str:
        found = sorted(self.reviews().glob("ref-feature-*/report.txt"))
        self.assertEqual(len(found), 1, found)
        return found[0].read_text()

    def test_deny_toml_comes_from_the_host(self):
        self.change("tools/sandbox/deny.toml", "[advisories]\nignore = ['RUSTSEC-0000-0000']\n")
        status, _, err = self.review(SANDBOX_CONFIRM="yes")
        self.assertEqual(status, 0, err)
        deny = next(call for call in self.runs() if kind(call) == "deny")
        self.assertIn(f"type=bind,src={HOST_DENY},dst=/sandbox/deny.toml,readonly", deny["argv"])
        self.assertEqual(self.report()["touched"], [{"path": "tools/sandbox/deny.toml", "status": "M",
                                                     "kinds": ["HOST_EXECUTED"]}])

    def test_rerun_starts_from_an_empty_target(self):
        self.change("crates/oxedi_core/src/lib.rs", "pub fn g() {}\n")
        status, _, err = self.review(FAKE_RUN_LEAVES="leftover")
        self.assertEqual(status, 0, err)
        run = next(call for call in self.runs() if kind(call) == "run")
        target = next(m["src"] for m in layout_mounts(run["argv"]) if m["dst"] == "/work/target")
        leftover = Path(target) / "leftover"
        self.assertTrue(leftover.exists())
        (self.fake / "calls.jsonl").unlink()
        status, _, err = self.review(FAKE_WATCH=str(leftover))
        self.assertEqual(status, 0, err)
        self.assertEqual([call["watched"] for call in self.runs()], [False] * 6)
        run = next(call for call in self.runs() if kind(call) == "run")
        self.assertEqual(run["binds"][target]["entries"], [])

    def test_source_names_another_checkout(self):
        other = new_repo(self.tmp / "other")
        commit(other, "readme.txt", "r\n")
        git(other, "checkout", "-q", "-b", "feature")
        commit(other, "build.rs", "fn main() {}\n")
        status, _, err = self.review(SANDBOX_SOURCE=str(other))
        self.assertEqual(status, 4, err)
        self.assertEqual([t["path"] for t in self.report()["touched"]], ["build.rs"])
        deny = next(call for call in self.runs() if kind(call) == "deny")
        self.assertIn(f"type=bind,src={HOST_DENY},dst=/sandbox/deny.toml,readonly", deny["argv"])

    def test_refusal_lines(self):
        plain = self.tmp / "plain"
        plain.mkdir()
        cases = [
            ({"SANDBOX_REF": "", "SANDBOX_PR": "12a"},
             'sandbox-pr: PR must be a pull request number, 1 to 999999999, got "12a"\n'),
            ({"SANDBOX_CONFIRM": "y"}, 'sandbox-pr: CONFIRM must be yes or unset, got "y"\n'),
            ({"SANDBOX_REF": "nothing"}, f'sandbox-pr: "nothing" names no commit in {self.host}\n'),
            ({"SANDBOX_SOURCE": str(plain)},
             f'sandbox-pr: SOURCE must be the top directory of a git checkout, got "{plain}"\n'),
        ]
        for extra, line in cases:
            with self.subTest(extra=extra):
                status, _, err = self.review(**extra)
                self.assertEqual((status, err), (2, line))
        self.assertEqual(self.runs(), [])

    def test_docker_missing_is_refused(self):
        bare = self.tmp / "bare-bin"
        bare.mkdir()
        (bare / "git").symlink_to(next(Path(p) / "git" for p in os.environ["PATH"].split(os.pathsep)
                                       if (Path(p) / "git").is_file()))
        status, _, err = self.review(PATH=str(bare))
        self.assertEqual((status, err), (2, "sandbox-pr: docker was not found on PATH; install Docker Engine "
                                            "(Linux) or Docker Desktop\n"))

    def test_an_old_git_is_refused(self):
        real = next(Path(p) / "git" for p in os.environ["PATH"].split(os.pathsep) if (Path(p) / "git").is_file())
        (self.bin / "git").write_text(
            f'#!/bin/sh\n[ "$1" = version ] && echo "git version 2.39.2" && exit 0\nexec "{real}" "$@"\n')
        (self.bin / "git").chmod(0o755)
        status, _, err = self.review()
        self.assertEqual((status, err), (2, "sandbox-pr: needs git 2.40 or later (for --attr-source), "
                                            "found git version 2.39.2\n"))
        self.assertEqual(self.calls(), [])

    def test_no_merge_ref_is_refused(self):
        author = new_repo(self.tmp / "author")
        commit(author, "readme.txt", "r\n")
        host = self.tmp / "pr-host"
        git(self.tmp, "clone", "-q", str(author), str(host))
        git(host, "remote", "set-url", "origin", "git@github.com:javillegasna/oxedi.git")
        head = commit(author, "pr.txt", "p\n")
        served = self.tmp / "served.git"
        git(self.tmp, "init", "-q", "--bare", str(served))
        git(author, "push", "-q", str(served), f"{head}:refs/pull/7/head")
        self.host = host
        with mock.patch.object(source, "github_https", return_value=str(served)), \
                mock.patch.object(source, "TEST_OPTIONS", FILE_TRANSPORT):
            status, _, err = self.review(SANDBOX_REF="", SANDBOX_PR="7")
        self.assertEqual((status, err), (2, "sandbox-pr: pull request 7 has no merge ref refs/pull/7/merge: it has "
                                            "conflicts or is closed; set HEAD=1 to test refs/pull/7/head instead\n"))
        self.assertEqual(self.runs(), [])

    def test_make_passes_the_variables(self):
        checkout = make_repo_copy(self.tmp / "copy")
        for args, line in ((["PR=12a"], 'sandbox-pr: PR must be a pull request number, 1 to 999999999, got "12a"'),
                           (["PR=7", "CONFIRM=y"], 'sandbox-pr: CONFIRM must be yes or unset, got "y"'),
                           (["PR=7", "HEAD=2"], 'sandbox-pr: HEAD must be 1 or unset, got "2"'),
                           (["REF=-x"], 'sandbox-pr: REF must not start with "-", got "-x"')):
            with self.subTest(args=args):
                result = subprocess.run(["make", "--no-print-directory", "-C", str(checkout), "sandbox-pr", *args],
                                        env=self.env(SANDBOX_REF=""), capture_output=True, text=True, timeout=120)
                self.assertEqual(result.stderr.splitlines()[0], line)
        self.assertEqual(self.runs(), [])

    def test_every_git_of_the_process_is_isolated(self):
        environ = {"GIT_DIR": "/elsewhere", "GIT_CONFIG_GLOBAL": "/home/x/.gitconfig", "PATH": "/bin"}
        sandbox_pr.git_isolation(environ)
        self.assertNotIn("GIT_DIR", environ)
        self.assertEqual((environ["GIT_CONFIG_GLOBAL"], environ["GIT_CONFIG_NOSYSTEM"]), (os.devnull, "1"))
        pairs = {environ[f"GIT_CONFIG_KEY_{i}"]: environ[f"GIT_CONFIG_VALUE_{i}"]
                 for i in range(int(environ["GIT_CONFIG_COUNT"]))}
        self.assertEqual(pairs, {"core.hooksPath": "/dev/null", "core.attributesFile": "/dev/null",
                                 "protocol.file.allow": "never"})


def layout_mounts(argv: list[str]) -> list[dict]:
    return [dict(part.partition("=")[::2] for part in spec.split(",")) for flag, spec in zip(argv, argv[1:])
            if flag == "--mount"]


if __name__ == "__main__":
    unittest.main()
