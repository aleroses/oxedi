"""Tests of risk.py: the kinds of the paths a change touches and when a change is flagged, on
temporary repositories only."""

import os
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import risk  # noqa: E402
from deps import PackageDiff  # noqa: E402
from risk import Kind, Touched  # noqa: E402
from source import Change  # noqa: E402
from test_source import ISOLATED, git, new_repo  # noqa: E402

REGISTRY = "registry+https://github.com/rust-lang/crates.io-index"
PYPROJECT = "crates/oxedi_py/pyproject.toml"


class Repo(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self._tmp.cleanup)
        self.tmp = Path(self._tmp.name)
        isolated = mock.patch.dict(os.environ, ISOLATED)
        isolated.start()
        self.addCleanup(isolated.stop)
        self.repo = new_repo(self.tmp / "repo")

    def write(self, files: dict[str, str | bytes]) -> None:
        for path, content in files.items():
            target = self.repo / path
            target.parent.mkdir(parents=True, exist_ok=True)
            if isinstance(content, bytes):
                target.write_bytes(content)
            else:
                target.write_text(content)

    def commit(self, files: dict[str, str | bytes]) -> str:
        self.write(files)
        git(self.repo, "add", "-A")
        git(self.repo, "commit", "-q", "--allow-empty", "-m", "c")
        return git(self.repo, "rev-parse", "HEAD")

    def change(self, base: str, head: str) -> Change:
        return Change("ref", "ref-x", head, base, head, False)


class Kinds(Repo):
    def test_host_executed_reason_says_the_change_runs_inside(self):
        self.assertEqual(
            Kind.HOST_EXECUTED.why,
            "a file the host executes or trusts after a run: the checkout's copy, this change's, is bound "
            "read-only and runs inside the sandbox (only the sandbox runner, this path list and deny.toml "
            "are always the host's)",
        )

    def test_kinds_table(self):
        base = self.commit({"Makefile": "all:\n", "crates/oxedi_core/src/value/parse.rs": "fn a() {}\n"})
        expected = {
            "crates/oxedi_core/build.rs": (Kind.BUILD_SCRIPT,),
            "crates/x/Cargo.toml": (Kind.MANIFEST,),
            "crates/m/Cargo.toml": (Kind.MANIFEST, Kind.PROC_MACRO),
            "rust-toolchain.toml": (Kind.MANIFEST,),
            "requirements-dev.txt": (Kind.MANIFEST,),
            "make/sandbox.mk": (Kind.HOST_EXECUTED,),
            "tools/sandbox/deny.toml": (Kind.HOST_EXECUTED,),
            "tools/sandbox/risk.py": (Kind.HOST_EXECUTED, Kind.SCRIPT),
            ".github/scripts/lint.sh": (Kind.HOST_EXECUTED, Kind.SCRIPT),
            "crates/x/.cargo/config.toml": (Kind.HOST_EXECUTED,),
            "viewer/app.js": (Kind.SCRIPT,),
            "crates/oxedi_py/tests/conftest.py": (Kind.SCRIPT, Kind.TEST_HOOK),
            "docs/python/index.md": (Kind.RUN_DOCS,),
            "README.md": (Kind.RUN_DOCS,),
            "assets/logo.bin": (Kind.BINARY,),
            "link": (Kind.SYMLINK,),
            "vendor/sub": (Kind.SUBMODULE,),
            "crates/oxedi_core/src/value/parse.rs": (),
            "crates/oxedi_py/python/oxedi/__init__.py": (),
            "Makefile": (Kind.HOST_EXECUTED,),
        }
        files = {path: "x\n" for path in expected if path not in ("link", "vendor/sub", "Makefile")}
        files["crates/m/Cargo.toml"] = '[package]\nname = "m"\n\n[lib]\nproc-macro = true\n'
        files["assets/logo.bin"] = b"\0\1\2\3"
        files["crates/oxedi_core/src/value/parse.rs"] = "fn b() {}\n"
        self.write(files)
        (self.repo / "Makefile").unlink()
        os.symlink("README.md", self.repo / "link")
        git(self.repo, "add", "-A")
        git(self.repo, "update-index", "--add", "--cacheinfo", f"160000,{base},vendor/sub")
        git(self.repo, "commit", "-q", "-m", "head")
        head = git(self.repo, "rev-parse", "HEAD")
        found = risk.touched(self.change(base, head), self.repo)
        self.assertEqual({t.path: t.kinds for t in found}, expected)
        self.assertEqual([t.path for t in found], sorted(expected))
        statuses = {t.path: t.status for t in found}
        self.assertEqual(statuses["Makefile"], "D")
        self.assertEqual(statuses["crates/oxedi_core/src/value/parse.rs"], "M")
        self.assertEqual(statuses["crates/oxedi_core/build.rs"], "A")

    def test_nested_src_is_script(self):
        for path in ("tools/crates/x/src/evil.py", "crates/x/y/src/evil.py", "crates/x/y/python/evil.sh",
                     "docs/crates/x/src/evil.js", "crates/src/evil.py", "crates/x/evil.py"):
            with self.subTest(path=path):
                self.assertIs(risk.is_script(path), True)
        for path in ("crates/oxedi_core/src/lib.py", "crates/oxedi_py/python/oxedi/__init__.py",
                     "crates/oxedi_core/src/value/deep/x.sh"):
            with self.subTest(path=path):
                self.assertIs(risk.is_script(path), False)

    def test_run_docs_are_the_root_readme_and_docs_markdown(self):
        for path, expected in (("README.md", True), ("docs/a.md", True), ("docs/python/b.md", True),
                               ("docs/a.txt", False), ("crates/oxedi_py/README.md", False),
                               ("x/docs/a.md", False), ("docsx/a.md", False)):
            with self.subTest(path=path):
                self.assertIs(risk.is_run_docs(path), expected)

    def test_the_change_attributes_cannot_hide_a_binary(self):
        base = self.commit({"a.txt": "a\n"})
        head = self.commit({"x.bin": b"\0\1\2\3", ".gitattributes": "*.bin diff\n"})
        self.assertEqual(git(self.repo, "rev-parse", "HEAD"), head)  # the worktree holds the change
        found = risk.touched(self.change(base, head), self.repo)
        self.assertEqual({t.path: t.kinds for t in found},
                         {"x.bin": (Kind.BINARY,), ".gitattributes": (Kind.GIT_ATTRIBUTES,)})

    def test_every_kind_says_why(self):
        for kind in Kind:
            with self.subTest(kind=kind):
                self.assertTrue(kind.why and "\n" not in kind.why)


class Flagged(unittest.TestCase):
    def test_flagged(self):
        plain = [Touched("crates/oxedi_core/src/lib.rs", (), "M")]
        self.assertIs(risk.flagged(plain, PackageDiff(), PackageDiff()), False)
        removed_only = PackageDiff(removed=(("b", "0.1.0", "", REGISTRY),))
        self.assertIs(risk.flagged(plain, removed_only, PackageDiff()), False)
        self.assertIs(risk.flagged([Touched("build.rs", (Kind.BUILD_SCRIPT,), "A")], PackageDiff(), PackageDiff()), True)
        self.assertIs(risk.flagged(plain, PackageDiff(), PackageDiff(added=(("x", "", "", PYPROJECT),))), True)
        self.assertIs(risk.flagged(plain, PackageDiff(changed=(("a", "1", "2", REGISTRY),)), PackageDiff()), True)


class ImageHint(unittest.TestCase):
    def test_image_hint_table(self):
        hint = "this change touches the viewer; rerun with IMAGE=viewer to run its tests"
        base, viewer = risk.ImageKind.BASE, risk.ImageKind.VIEWER
        for path, kind, expected in (
            ("viewer/app.js", base, hint),
            ("crates/oxedi_wasm/src/lib.rs", base, hint),
            ("viewer/app.js", viewer, ""),
            ("crates/oxedi_wasm/src/lib.rs", viewer, ""),
            ("crates/oxedi_core/src/lib.rs", base, ""),
            ("crates/oxedi_wasmish/src/lib.rs", base, ""),
            ("docs/viewer/index.md", base, ""),
        ):
            with self.subTest(path=path, kind=kind):
                self.assertEqual(risk.image_hint([Touched(path, (), "M")], kind), expected)
        mixed = [Touched("crates/oxedi_core/src/lib.rs", (), "M"), Touched("viewer/app.js", (), "M")]
        self.assertEqual(risk.image_hint(mixed, base), hint)
        self.assertEqual(risk.image_hint([], base), "")



if __name__ == "__main__":
    unittest.main()
