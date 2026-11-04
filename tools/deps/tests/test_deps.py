"""Tests of deps.py: its comparison on hand-written input, and the whole script against a fake
`cargo` and hand-built wheels, so a leak is proven to fail with a message that names it."""

import os
import stat
import subprocess
import sys
import tempfile
import unittest
import zipfile
from pathlib import Path

SCRIPT = Path(__file__).resolve().parent.parent / "deps.py"
sys.path.insert(0, str(SCRIPT.parent))
import deps  # noqa: E402

CORE_TREE = """oxedi_core v0.6.0 (/repo/crates/oxedi_core)
hmac v0.12.1
serde v1.0.229
serde_json v1.0.151
sha2 v0.10.9
"""

EXPECTED_TREES = {
    "oxedi_core": CORE_TREE,
    "oxedi_py": "oxedi_py v0.6.0 (/r)\n" + "".join(f"{d} v1.0.0\n" for d in deps.ALLOWED_CRATES["oxedi_py"]),
    "oxedi_duckdb": "oxedi_duckdb v0.2.0 (/r)\n" + "".join(f"{d} v1.0.0\n" for d in deps.ALLOWED_CRATES["oxedi_duckdb"]),
    "oxedi_wasm": "oxedi_wasm v0.6.0 (/r)\n" + "".join(f"{d} v1.0.0\n" for d in deps.ALLOWED_CRATES["oxedi_wasm"]),
}


def run_script(*args, trees=None, cwd=None):
    """Runs deps.py with a fake `cargo` that prints `trees[crate]` for `cargo tree ... -p crate`."""
    with tempfile.TemporaryDirectory() as tmp:
        bin_dir = Path(tmp) / "bin"
        bin_dir.mkdir()
        for crate, tree in (trees or {}).items():
            (Path(tmp) / f"{crate}.tree").write_text(tree)
        fake = bin_dir / "cargo"
        fake.write_text(
            '#!/bin/sh\nfor last; do :; done\ncat "$TREES/$last.tree" || exit 3\n'
        )
        fake.chmod(fake.stat().st_mode | stat.S_IXUSR)
        env = dict(os.environ, PATH=f"{bin_dir}{os.pathsep}{os.environ['PATH']}", TREES=tmp)
        return subprocess.run(
            [sys.executable, str(SCRIPT), *args], capture_output=True, text=True, env=env, cwd=cwd, check=False
        )


def make_wheel(directory, *requirements):
    path = Path(directory) / "oxedi-0.0.0-py3-none-any.whl"
    lines = ["Metadata-Version: 2.4", "Name: oxedi", "Version: 0.0.0"]
    lines += [f"Requires-Dist: {r}" for r in requirements]
    with zipfile.ZipFile(path, "w") as archive:
        archive.writestr("oxedi-0.0.0.dist-info/METADATA", "\n".join(lines) + "\n")
    return path


class Crates(unittest.TestCase):
    def test_the_allowed_dependencies_pass(self):
        done = run_script("crates", trees=EXPECTED_TREES)
        self.assertEqual(done.returncode, 0, done.stderr)

    def test_a_dev_only_crate_in_the_core_fails_naming_crate_dependency_and_list(self):
        trees = dict(EXPECTED_TREES, oxedi_core=CORE_TREE + "pprof v0.15.0\n")
        done = run_script("crates", trees=trees)
        self.assertEqual(done.returncode, 1)
        self.assertEqual(
            done.stderr.strip(),
            "deps-check: oxedi_core: normal dependency `pprof` is not in the allowed list "
            "(hmac, serde, serde_json, sha2); a development tool belongs in [dev-dependencies], "
            "and a real new dependency needs review and an entry in ALLOWED_CRATES of "
            "tools/deps/deps.py",
        )

    def test_an_allowed_entry_the_crate_no_longer_has_fails_with_the_fix(self):
        trees = dict(EXPECTED_TREES, oxedi_core=CORE_TREE.replace("sha2 v0.10.9\n", ""))
        done = run_script("crates", trees=trees)
        self.assertEqual(done.returncode, 1)
        self.assertEqual(
            done.stderr.strip(),
            "deps-check: oxedi_core: allowed dependency `sha2` is no longer a normal dependency; "
            "remove it from ALLOWED_CRATES in tools/deps/deps.py",
        )

    def test_the_crate_itself_on_the_first_line_is_not_a_dependency(self):
        self.assertEqual(deps.tree_dependencies(CORE_TREE), ["hmac", "serde", "serde_json", "sha2"])
        self.assertEqual(deps.crate_findings("oxedi_core", ["oxedi_core"], []), [])

    def test_a_cargo_failure_is_reported_as_a_check_that_could_not_run(self):
        done = run_script("crates", trees={})
        self.assertEqual(done.returncode, 2)
        self.assertIn("deps-check: oxedi_core: `cargo tree -e normal --depth 1 --target all --all-features --prefix none --locked -p oxedi_core` failed", done.stderr)


class Wheels(unittest.TestCase):
    def check(self, *requirements):
        with tempfile.TemporaryDirectory() as tmp:
            return run_script("wheel", str(make_wheel(tmp, *requirements)))

    def test_requirements_gated_by_user_extras_pass(self):
        done = self.check("pandas>=2 ; extra == 'pandas'", "pyarrow>=14 ; extra == 'pandas'", 'polars>=1.0 ; extra == "polars"')
        self.assertEqual(done.returncode, 0, done.stderr)
        self.assertEqual(done.stdout.strip(), "deps-check: wheel ok (checked wheel: oxedi-0.0.0-py3-none-any.whl)")

    def test_a_wheel_without_requirements_passes(self):
        self.assertEqual(self.check().returncode, 0)

    def test_an_unconditional_requirement_fails_naming_wheel_requirement_and_extras(self):
        done = self.check("pytest>=8")
        self.assertEqual(done.returncode, 1)
        self.assertEqual(
            done.stderr.strip(),
            "deps-check: oxedi-0.0.0-py3-none-any.whl: Requires-Dist `pytest>=8` has no `extra ==` "
            "marker, so every install of the wheel would pull it in; allowed extras: "
            "edi-835-parser, pandas, polars, pyx12",
        )

    def test_a_requirement_of_another_extra_fails(self):
        done = self.check("pytest>=8 ; extra == 'test'")
        self.assertEqual(done.returncode, 1)
        self.assertEqual(
            done.stderr.strip(),
            "deps-check: oxedi-0.0.0-py3-none-any.whl: Requires-Dist `pytest>=8 ; extra == 'test'` "
            "belongs to extra `test`, which is not a user extra; allowed extras: "
            "edi-835-parser, pandas, polars, pyx12",
        )

    def test_a_marker_that_is_not_an_extra_does_not_gate_the_requirement(self):
        done = self.check("colorama ; sys_platform == 'win32'")
        self.assertEqual(done.returncode, 1)

    def test_a_wheel_found_in_target_wheels_is_named_and_hinted_as_possibly_stale(self):
        with tempfile.TemporaryDirectory() as tmp:
            (Path(tmp) / "target/wheels").mkdir(parents=True)
            make_wheel(Path(tmp) / "target/wheels", "pytest>=8")
            done = run_script("wheel", cwd=tmp)
            self.assertEqual(done.returncode, 1)
            self.assertIn("Requires-Dist `pytest>=8` has no `extra ==` marker", done.stderr)
            self.assertIn(
                "deps-check: the wheel was read from target/wheels, not built by this check; "
                "if it is stale, run make clean-dist && make dist",
                done.stderr,
            )
            make_wheel(Path(tmp) / "target/wheels", "pandas>=2 ; extra == 'pandas'")
            ok = run_script("wheel", cwd=tmp)
            self.assertEqual(ok.returncode, 0, ok.stderr)
            self.assertEqual(ok.stdout.strip(), "deps-check: wheel ok (checked existing wheel: oxedi-0.0.0-py3-none-any.whl)")

    def test_no_wheel_is_reported_as_a_check_that_could_not_run(self):
        with tempfile.TemporaryDirectory() as tmp:
            done = run_script("wheel", cwd=tmp)
        self.assertEqual(done.returncode, 2)
        self.assertEqual(done.stderr.strip(), "deps-check: no wheel given and none in target/wheels; run make dist first")

    def test_a_file_that_is_not_a_wheel_is_reported_with_its_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            bad = Path(tmp) / "x.whl"
            bad.write_text("not a zip")
            done = run_script("wheel", str(bad))
        self.assertEqual(done.returncode, 2)
        self.assertIn(f"deps-check: {bad}: cannot read the wheel", done.stderr)


if __name__ == "__main__":
    unittest.main()
