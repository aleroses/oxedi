"""Tests of deps.py: the Cargo.lock and the declared Python dependency diffs, on temporary
repositories only."""

import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
import deps  # noqa: E402
import risk  # noqa: E402
from deps import PackageDiff  # noqa: E402
from test_risk import PYPROJECT, REGISTRY, Repo  # noqa: E402

GIT_SOURCE = "git+https://github.com/example/c?rev=abc#abc0123"

OLD_LOCK = f"""version = 3

[[package]]
name = "a"
version = "1.0.0"
source = "{REGISTRY}"

[[package]]
name = "b"
version = "0.1.0"
source = "{REGISTRY}"

[[package]]
name = "syn"
version = "1.0.109"
source = "{REGISTRY}"

[[package]]
name = "syn"
version = "2.0.0"
source = "{REGISTRY}"

[[package]]
name = "oxedi_core"
version = "0.6.0"
"""
NEW_LOCK = f"""version = 3

[[package]]
name = "a"
version = "1.1.0"
source = "{REGISTRY}"

[[package]]
name = "c"
version = "2.0.0"
source = "{GIT_SOURCE}"

[[package]]
name = "syn"
version = "2.0.0"
source = "{REGISTRY}"

[[package]]
name = "oxedi_core"
version = "0.6.0"
"""


def pyproject(arrow: str, build: str = '"maturin>=1.5"') -> str:
    return f"""[project]
name = "oxedi"
dependencies = []

[project.optional-dependencies]
pandas = ["pandas>=2"]

[dependency-groups]
dev = [{{include-group = "arrow"}}, "pytest>=8"]
arrow = ["{arrow}"]

[build-system]
requires = [{build}]
"""


class Cargo(unittest.TestCase):
    def test_cargo_lock_added_changed_removed(self):
        self.assertEqual(deps.cargo_lock_diff(OLD_LOCK, NEW_LOCK), PackageDiff(
            added=(("c", "", "2.0.0", GIT_SOURCE),),
            removed=(("b", "0.1.0", "", REGISTRY), ("syn", "1.0.109", "", REGISTRY)),
            changed=(("a", "1.0.0", "1.1.0", REGISTRY),),
        ))

    def test_the_same_version_from_a_second_source_is_added(self):
        evil = "git+https://evil/x#1"
        new = OLD_LOCK.replace('[[package]]\nname = "syn"\nversion = "2.0.0"',
                               f'[[package]]\nname = "syn"\nversion = "2.0.0"\nsource = "{evil}"\n\n'
                               '[[package]]\nname = "syn"\nversion = "2.0.0"')
        self.assertEqual(deps.cargo_lock_diff(OLD_LOCK, new), PackageDiff(added=(("syn", "", "2.0.0", evil),)))

    def test_a_version_from_another_source_is_a_change(self):
        new = OLD_LOCK.replace(f'version = "0.1.0"\nsource = "{REGISTRY}"', f'version = "0.1.0"\nsource = "{GIT_SOURCE}"')
        self.assertEqual(deps.cargo_lock_diff(OLD_LOCK, new), PackageDiff(changed=(("b", "0.1.0", "0.1.0", GIT_SOURCE),)))

    def test_a_new_lock_adds_every_package(self):
        diff = deps.cargo_lock_diff("", NEW_LOCK, "crates/x/Cargo.lock")
        self.assertEqual(diff.added, (("a", "", "1.1.0", REGISTRY), ("c", "", "2.0.0", GIT_SOURCE),
                                      ("oxedi_core", "", "0.6.0", "local"), ("syn", "", "2.0.0", REGISTRY)))

    def test_unparseable_lock_is_flagged(self):
        diff = deps.cargo_lock_diff(OLD_LOCK, "garbage [[")
        self.assertEqual(diff, PackageDiff(errors=(
            "Cargo.lock after the change is not valid TOML: Expected '=' after a key in a key/value pair "
            "(at line 1, column 9)",
        )))
        self.assertIs(risk.flagged([], diff, PackageDiff()), True)

    def test_a_package_without_a_version_is_a_finding(self):
        diff = deps.cargo_lock_diff(OLD_LOCK, '[[package]]\nname = "a"\n')
        self.assertEqual(diff.errors, (
            "Cargo.lock after the change holds a [[package]] without a name, a version and a source as text: "
            "{'name': 'a'}",
        ))


PATCHED = '[package]\nname = "w"\n\n[patch.crates-io]\nx = { git = "https://evil/x" }\n'


class CargoSources(unittest.TestCase):
    def test_patch_git_source(self):
        self.assertEqual(deps.cargo_sources({}, {"Cargo.toml": PATCHED}), ("https://evil/x",))

    def test_registry_only(self):
        new = OLD_LOCK.replace('version = "1.0.0"', 'version = "1.0.1"')
        sparse = new.replace(REGISTRY, "sparse+https://index.crates.io/")
        self.assertEqual(deps.cargo_sources({"Cargo.lock": OLD_LOCK}, {"Cargo.lock": new}), ())
        self.assertEqual(deps.cargo_sources({"Cargo.lock": OLD_LOCK}, {"Cargo.lock": sparse}), ())

    def test_every_place_a_manifest_names_a_source(self):
        manifest = (
            '[dependencies]\na = { git = "https://g/a", branch = "main" }\n\n'
            '[target.\'cfg(unix)\'.dev-dependencies]\nb = { git = "https://g/b" }\n\n'
            '[workspace.dependencies]\nc = { git = "https://g/c" }\n\n'
            '[replace]\n"d:1.0.0" = { git = "https://g/d" }\n\n'
            '[source.mirror]\nregistry = "https://g/index"\n\n'
            '[source.vendored]\ngit = "https://g/vendored"\n\n'
            '[build-dependencies]\ne = { version = "1", registry = "internal" }\n'
        )
        self.assertEqual(deps.cargo_sources({}, {"crates/x/Cargo.toml": manifest}), (
            "https://g/a", "https://g/b", "https://g/c", "https://g/d", "https://g/index", "https://g/vendored"))

    def test_lock_sources_from_elsewhere(self):
        found = deps.cargo_sources({"Cargo.lock": OLD_LOCK}, {"Cargo.lock": NEW_LOCK})
        self.assertEqual(found, ("https://github.com/example/c",))
        other = OLD_LOCK.replace(REGISTRY, "registry+https://evil/index", 1)
        self.assertEqual(deps.cargo_sources({"Cargo.lock": OLD_LOCK}, {"Cargo.lock": other}), ("https://evil/index",))
        lookalike = OLD_LOCK.replace(REGISTRY, f"{REGISTRY}.evil.example", 1)
        self.assertEqual(deps.cargo_sources({"Cargo.lock": OLD_LOCK}, {"Cargo.lock": lookalike}),
                         ("https://github.com/rust-lang/crates.io-index.evil.example",))

    def test_credentials_are_redacted(self):
        manifest = '[dependencies]\na = { git = "https://user:tok@g/a" }\n'
        self.assertEqual(deps.cargo_sources({}, {"Cargo.toml": manifest}), ("https://<redacted>@g/a",))

    def test_a_source_already_there_is_not_new(self):
        old = {"Cargo.toml": PATCHED, "Cargo.lock": NEW_LOCK}
        self.assertEqual(deps.cargo_sources(old, dict(old)), ())
        moved = {"crates/y/Cargo.toml": PATCHED, "Cargo.lock": NEW_LOCK}
        self.assertEqual(deps.cargo_sources(old, moved), ())

    def test_an_unparseable_file_names_no_source(self):
        self.assertEqual(deps.cargo_sources({}, {"Cargo.toml": "garbage [[", "Cargo.lock": "package = 1\n"}), ())


class Python(unittest.TestCase):
    def test_python_declared_include_group(self):
        diff = deps.declared_python_diff({PYPROJECT: pyproject("pyarrow>=14")}, {PYPROJECT: pyproject("pyarrow>=15")})
        self.assertEqual(diff, PackageDiff(changed=(("pyarrow", ">=14", ">=15", PYPROJECT),)))

    def test_every_table_counts(self):
        new = pyproject("pyarrow>=14", build='"maturin>=1.5", "setuptools"')
        new = new.replace('pandas = ["pandas>=2"]', 'pandas = ["pandas>=2", "Typing_Extensions>=4"]')
        diff = deps.declared_python_diff({PYPROJECT: pyproject("pyarrow>=14")}, {PYPROJECT: new})
        self.assertEqual(diff.added, (("setuptools", "", "", PYPROJECT), ("typing-extensions", "", ">=4", PYPROJECT)))

    def test_an_undefined_or_circular_group_is_a_finding(self):
        for groups, name in (('dev = [{include-group = "nope"}]', "'nope'"),
                             ('dev = [{include-group = "dev"}]', "'dev'")):
            with self.subTest(groups=groups):
                diff = deps.declared_python_diff({}, {PYPROJECT: f"[dependency-groups]\n{groups}\n"})
                self.assertEqual(diff.errors, (
                    f"{PYPROJECT} after the change [dependency-groups] includes {name}, "
                    "which is undefined or includes itself",
                ))

    def test_requirements_line_by_line(self):
        old = "# tools\nrequests==2.31  # http\n-r base.txt\nnumpy>=1 \\\n  --hash=sha256:ab\n"
        new = "requests==2.32\n-r base.txt\nrich ; python_version >= '3.11'\nhttps://example.com/x-1.0.whl\n"
        self.assertEqual(deps.declared_python_diff({"requirements.txt": old}, {"requirements.txt": new}), PackageDiff(
            added=(("https://example.com/x-1.0.whl", "", "", "requirements.txt"),
                   ("rich", "", "; python_version >= '3.11'", "requirements.txt")),
            removed=(("numpy", ">=1", "", "requirements.txt"),),
            changed=(("requests", "==2.31", "==2.32", "requirements.txt"),),
        ))

    def test_option_lines_are_declared_sources(self):
        new = ("-e git+https://evil/x#egg=x\n-r  base.txt\n-c constraints.txt\n"
               "--index-url https://user:s3cret@pypi.example/simple\nevil @ https://tok@example.com/e.whl\n")
        diff = deps.declared_python_diff({}, {"requirements.txt": new})
        self.assertEqual(diff.added, (
            ("--index-url https://<redacted>@pypi.example/simple", "", "", "requirements.txt"),
            ("-c constraints.txt", "", "", "requirements.txt"),
            ("-e git+https://evil/x#egg=x", "", "", "requirements.txt"),
            ("-r base.txt", "", "", "requirements.txt"),
            ("evil", "", "@ https://<redacted>@example.com/e.whl", "requirements.txt"),
        ))
        self.assertIs(risk.flagged([], PackageDiff(), diff), True)

    def test_uv_source_changed(self):
        new = pyproject("pyarrow>=14") + '\n[tool.uv.sources]\npyarrow = { git = "https://x/pyarrow" }\n'
        diff = deps.declared_python_diff({PYPROJECT: pyproject("pyarrow>=14")}, {PYPROJECT: new})
        self.assertEqual(diff, PackageDiff(added=(("pyarrow", "", "git=https://x/pyarrow", PYPROJECT),)))
        self.assertIs(risk.flagged([], PackageDiff(), diff), True)

    def test_uv_index_added(self):
        new = pyproject("pyarrow>=14") + '\n[[tool.uv.index]]\nurl = "https://evil/simple"\n'
        diff = deps.declared_python_diff({PYPROJECT: pyproject("pyarrow>=14")}, {PYPROJECT: new})
        self.assertEqual(diff, PackageDiff(added=(("uv:index", "", "url=https://evil/simple", PYPROJECT),)))

    def test_uv_index_keys_and_credentials(self):
        uv = ('[tool.uv]\nindex-url = "https://tok@pypi.example/simple"\n'
              'extra-index-url = ["https://a/simple", "https://b/simple"]\nfind-links = ["https://c/wheels"]\n\n'
              '[tool.uv.sources]\nTyping_Extensions = [{ index = "a", marker = "sys_platform == \'linux\'" }, '
              '{ path = "vendor/te", editable = true }]\n')
        diff = deps.declared_python_diff({}, {PYPROJECT: uv})
        self.assertEqual(diff.added, (
            ("typing-extensions", "", "index=a, marker=sys_platform == 'linux' | editable=true, path=vendor/te", PYPROJECT),
            ("uv:extra-index-url", "", "https://a/simple | https://b/simple", PYPROJECT),
            ("uv:find-links", "", "https://c/wheels", PYPROJECT),
            ("uv:index-url", "", "https://<redacted>@pypi.example/simple", PYPROJECT),
        ))

    def test_uv_source_url_changed_and_removed(self):
        old = '[tool.uv.sources]\na = { git = "https://x/a" }\nb = { path = "b" }\n'
        new = '[tool.uv.sources]\na = { git = "https://evil/a" }\n'
        self.assertEqual(deps.declared_python_diff({PYPROJECT: old}, {PYPROJECT: new}), PackageDiff(
            removed=(("b", "path=b", "", PYPROJECT),),
            changed=(("a", "git=https://x/a", "git=https://evil/a", PYPROJECT),),
        ))

    def test_an_untouched_uv_table_is_no_change(self):
        text = pyproject("pyarrow>=14") + '\n[tool.uv.sources]\npyarrow = { git = "https://x/pyarrow" }\n'
        self.assertEqual(deps.declared_python_diff({PYPROJECT: text}, {PYPROJECT: text}), PackageDiff())

    def test_a_malformed_uv_table_is_a_finding(self):
        diff = deps.declared_python_diff({}, {PYPROJECT: '[tool]\nuv = "x"\n'})
        self.assertEqual(diff.errors, (f"{PYPROJECT} after the change [tool.uv] is not a table",))

    def test_a_malformed_pyproject_is_a_finding(self):
        diff = deps.declared_python_diff({}, {PYPROJECT: "[project]\ndependencies = \"pyarrow\"\n"})
        self.assertEqual(diff.errors, (f"{PYPROJECT} after the change [project] dependencies is not a list of strings",))


class Dependencies(Repo):
    def test_dependency_diffs_read_both_ends(self):
        base = self.commit({"Cargo.lock": OLD_LOCK, PYPROJECT: pyproject("pyarrow>=14"), "src/lib.rs": "x\n"})
        head = self.commit({"Cargo.lock": NEW_LOCK, PYPROJECT: pyproject("pyarrow>=15"),
                            "requirements-docs.txt": "mkdocs==1.6\n", "src/lib.rs": "y\n"})
        change = self.change(base, head)
        cargo, python = deps.dependency_diffs(change, self.repo, [t.path for t in risk.touched(change, self.repo)])
        self.assertEqual(cargo, deps.cargo_lock_diff(OLD_LOCK, NEW_LOCK))
        self.assertEqual(python, PackageDiff(
            added=(("mkdocs", "", "==1.6", "requirements-docs.txt"),),
            changed=(("pyarrow", ">=14", ">=15", PYPROJECT),),
        ))

    def test_a_lock_that_is_not_utf8_is_a_finding(self):
        base = self.commit({"Cargo.lock": OLD_LOCK})
        head = self.commit({"Cargo.lock": b"\xff\xfe"})
        change = self.change(base, head)
        cargo, python = deps.dependency_diffs(change, self.repo, [t.path for t in risk.touched(change, self.repo)])
        self.assertEqual(len(cargo.errors), 1)
        self.assertTrue(cargo.errors[0].startswith("Cargo.lock after the change is not UTF-8 text: "), cargo.errors)
        self.assertEqual(python, PackageDiff())


if __name__ == "__main__":
    unittest.main()
