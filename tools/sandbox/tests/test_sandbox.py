"""Tests of sandbox.py: the docker arguments of each phase, built from a surveyed checkout, and the
whole runner against a fake `docker` (fake_docker.py), so every refusal, error line and kill is
proven on the arguments docker would receive."""

import hashlib
import io
import json
import os
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from itertools import pairwise
from pathlib import Path
from unittest import mock

HERE = Path(__file__).resolve().parent
SCRIPT = HERE.parent / "sandbox.py"
REPO = HERE.parents[2]
sys.path.insert(0, str(SCRIPT.parent))
import arguments  # noqa: E402
import layout  # noqa: E402
import sandbox  # noqa: E402
import storage  # noqa: E402
import viewer_image  # noqa: E402

UID, GID = os.getuid(), os.getgid()
LIMITS = arguments.Limits(cpus=3, memory_gb=4, timeout_s=60)
SECRETS = {
    "GH_TOKEN": "ghs_sandboxtestsecret1",
    "AWS_SECRET_ACCESS_KEY": "awssandboxtestsecret2",
    "PYPI_TOKEN": "pypi-sandboxtestsecret3",
}


def mounts(argv: list[str]) -> list[dict]:
    """Every `--mount` of an argument list, as a dict of its fields (`readonly` maps to "")."""
    found = []
    for flag, spec in pairwise(argv):
        if flag == "--mount":
            found.append(dict(part.partition("=")[::2] for part in spec.split(",")))
    return found


def mount_at(argv: list[str], destination: str) -> dict:
    matches = [m for m in mounts(argv) if m.get("dst") == destination]
    assert len(matches) == 1, f"{len(matches)} mounts at {destination} in {argv}"
    return matches[0]


def envs(argv: list[str]) -> list[str]:
    return [value for flag, value in pairwise(argv) if flag == "--env"]


def has_pair(argv: list[str], flag: str, value: str) -> bool:
    return any(a == flag and b == value for a, b in pairwise(argv))


GIT_ENV = {"GIT_CONFIG_NOSYSTEM": "1", "GIT_CONFIG_GLOBAL": os.devnull}


def make_checkout(root: Path, *paths: str) -> Path:
    """A git checkout holding `paths`: a trailing slash makes a directory, anything else a file.
    Every file is added to the index unless the checkout's .gitignore ignores it."""
    root.mkdir(parents=True)
    for path in paths:
        target = root / path
        if path.endswith("/"):
            target.mkdir(parents=True, exist_ok=True)
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_text("x\n" if path != ".gitignore" else IGNORED)
    return git_init(root)


def git_init(root: Path) -> Path:
    """Makes `root` a git checkout with every file it holds in the index."""
    env = dict(os.environ, **GIT_ENV)
    for command in (["init", "-q"], ["add", "-A"]):
        subprocess.run(["git", "-C", str(root), *command], env=env, check=True)
    return root


# What `make sandbox` reads from this repository. The tests that go through make copy them into a
# throwaway checkout, so the runner audits and scrubs that checkout, never this one.
MAKE_FILES = (
    "Makefile", "make", "tools/sandbox", "rust-toolchain.toml", "Cargo.toml", "crates/oxedi_duckdb/Cargo.toml",
    "crates/oxedi_wasm/scripts/install_tools.sh", "docs/requirements.txt",
)


def make_repo_copy(root: Path) -> Path:
    """A git checkout holding this repository's make entry points and the sandbox runner."""
    for path in MAKE_FILES:
        source, target = REPO / path, root / path
        if source.is_dir():
            shutil.copytree(source, target, ignore=shutil.ignore_patterns("__pycache__"))
        else:
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, target)
    return git_init(root)


IGNORED = ".venv-docs/\n/viewer/pkg/\nnotes.log\n.env\n/target/\n/.venv/\n__pycache__/\n.pytest_cache/\n*.so\n"


FULL_SET = [
    "Makefile", "make/", "tools/", ".cargo/", ".github/", "rust-toolchain.toml", "Cargo.toml",
    "Cargo.lock", "CLAUDE.md", ".claude/", "crates/oxedi_py/pyproject.toml",
]


class World(unittest.TestCase):
    """A temporary home, a fake `docker` first on PATH and checkouts to run the sandbox over."""

    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)
        self.home = self.tmp / "home"
        isolated = mock.patch.dict(os.environ, GIT_ENV)
        isolated.start()
        self.addCleanup(isolated.stop)
        (self.home / ".ssh").mkdir(parents=True)
        self.fake = self.tmp / "fake"
        self.fake.mkdir()
        self.bin = self.tmp / "bin"
        self.bin.mkdir()
        shim = self.bin / "docker"
        shim.write_text(f'#!/bin/sh\nexec "{sys.executable}" -I "{HERE / "fake_docker.py"}" "$@"\n')
        shim.chmod(0o755)

    def tearDown(self):
        self._tmp.cleanup()

    def env(self, **extra: str) -> dict:
        env = {
            "PATH": f"{self.bin}{os.pathsep}{os.environ['PATH']}",
            "HOME": str(self.home),
            "FAKE_DIR": str(self.fake),
            "SANDBOX_CMD": "cargo test",
            # The viewer image's wasm labels, as built from this repository's install_tools.sh.
            "FAKE_WASM_BINDGEN_LABEL": WASM_PINS.bindgen_version,
            "FAKE_BINARYEN_LABEL": WASM_PINS.binaryen_version,
            **GIT_ENV,
        }
        env.update(extra)
        return env

    def checkout(self, name: str = "checkout", *paths: str) -> Path:
        return make_checkout(self.tmp / name, *(paths or ("Makefile",)))

    def make(self, checkout: Path, *args: str, **extra: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            ["make", "--no-print-directory", "-C", str(checkout), *args],
            env=self.env(**extra), capture_output=True, text=True, check=False, timeout=120,
        )

    def start(self, checkout: Path, kind: str = "base", **extra: str) -> subprocess.Popen:
        return subprocess.Popen(
            [sys.executable, "-I", str(SCRIPT), "--image", "oxedi-sandbox:test", "--kind", kind,
             "--checkout", str(checkout)],
            env=self.env(**extra), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )

    def sandbox(self, checkout: Path, kind: str = "base", **extra: str) -> subprocess.CompletedProcess:
        process = self.start(checkout, kind, **extra)
        out, err = process.communicate(timeout=60)
        return subprocess.CompletedProcess(process.args, process.returncode, out, err)

    def wait_for_run(self) -> None:
        """Waits until the run-phase container of the fake docker has started."""
        deadline = time.monotonic() + 20
        while not (self.fake / "run.pid").exists() and time.monotonic() < deadline:
            time.sleep(0.05)

    def calls(self) -> list[dict]:
        log = self.fake / "calls.jsonl"
        if not log.exists():
            return []
        return [json.loads(line) for line in log.read_text().splitlines()]

    def runs(self) -> list[dict]:
        return [call for call in self.calls() if call["argv"][:1] == ["run"]]

    def run_phase(self) -> list[str]:
        runs = [call["argv"] for call in self.runs() if "bash" in call["argv"]]
        self.assertEqual(len(runs), 1, self.calls())
        return runs[0]

    def own(self, checkout: Path) -> Path:
        """This checkout's storage directory."""
        return self.home / ".cache" / "oxedi-sandbox" / layout.path_hash(checkout)

    def phase_args(self, phase, checkout: Path, kind=arguments.ImageKind.BASE) -> list[str]:
        surveyed = layout.survey(checkout)
        own = storage.storage_for(self.home / ".cache" / "oxedi-sandbox", surveyed)
        return arguments.docker_args(phase, surveyed, UID, GID, arguments.Runtime.RUNC, LIMITS, own, kind)


class Arguments(World):
    def test_both_phases_hardened(self):
        checkout = self.checkout("c", "Makefile", ".cargo/config.toml")
        name = f"oxedi-sandbox-{layout.path_hash(checkout)}"
        cache = self.home / ".cache" / "oxedi-sandbox"
        for phase in arguments.Phase:
            with self.subTest(phase=phase):
                argv = self.phase_args(phase, checkout)
                for flag in ("--rm", "--init", "--read-only"):
                    self.assertIn(flag, argv)
                for flag, value in [
                    ("--name", name), ("--user", f"{UID}:{GID}"), ("--cap-drop", "ALL"),
                    ("--security-opt", "no-new-privileges"), ("--pids-limit", "4096"),
                    ("--memory", "4g"), ("--memory-swap", "4g"), ("--cpus", "3"),
                    ("--tmpfs", "/tmp:rw,exec,nosuid,nodev,size=2g"), ("--pull", "never"),
                ]:
                    self.assertTrue(has_pair(argv, flag, value), f"{flag} {value} missing in {argv}")
                self.assertIn("HOME=/tmp/home", envs(argv))
                self.assertFalse(any("seccomp" in a or "privileged" in a or "--cap-add" == a for a in argv))
                self.assertFalse(any(a in ("-e", "-v", "--volume", "--env-file") for a in argv))
        fetch = self.phase_args(arguments.Phase.FETCH, checkout)
        self.assertNotIn("--network", fetch)
        self.assertIn("readonly", mount_at(fetch, "/work"))
        self.assertEqual(mount_at(fetch, "/work/.cargo")["type"], "tmpfs")
        self.assertIn("readonly", mount_at(fetch, "/work/.cargo"))
        cargo = [m for m in mounts(fetch) if m.get("src") == str(cache / "cargo")]
        uv = [m for m in mounts(fetch) if m.get("src") == str(cache / "uv")]
        self.assertEqual(len(cargo), 1)
        self.assertEqual(len(uv), 1)
        self.assertNotIn("readonly", cargo[0])
        self.assertNotIn("readonly", uv[0])
        self.assertIn("CARGO_BUILD_RUSTC_WRAPPER=", envs(fetch))
        self.assertIn("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER=", envs(fetch))
        self.assertIn("UV_NO_CONFIG=1", envs(fetch))
        self.assertFalse([m for m in mounts(fetch) if m.get("dst") == "/work/target"])
        self.assertIn(f"UV_CACHE_DIR={arguments.UV_CACHE}", envs(fetch))
        self.assertIn(f"CARGO_HOME={arguments.CARGO_HOME}", envs(fetch))
        run = self.phase_args(arguments.Phase.RUN, checkout)
        self.assertTrue(has_pair(run, "--network", "none"))
        self.assertNotIn("readonly", mount_at(run, "/work"))
        cargo = [m for m in mounts(run) if m.get("src") == str(cache / "cargo")]
        self.assertEqual(len(cargo), 1)
        self.assertIn("readonly", cargo[0])
        self.assertFalse([m for m in mounts(run) if m.get("src") == str(cache / "uv")])
        for variable in ("UV_CACHE_DIR=/tmp/uv-cache", "CARGO_NET_OFFLINE=true", "UV_OFFLINE=1", "CARGO_INCREMENTAL=0"):
            self.assertIn(variable, envs(run))

    def test_host_executed_set_read_only(self):
        checkout = self.checkout("c", *FULL_SET, ".doc/scratch/notes.md", ".workflow-kit/x/brief.md")
        for phase in arguments.Phase:
            argv = self.phase_args(phase, checkout)
            for path in [p.rstrip("/") for p in FULL_SET] + [".git"]:
                with self.subTest(phase=phase, path=path):
                    mount = mount_at(argv, f"/work/{path}")
                    self.assertIn("readonly", mount)
                    if phase is not arguments.Phase.RUN and path == ".cargo":
                        self.assertEqual(mount["type"], "tmpfs")
                    else:
                        self.assertEqual((mount["type"], mount["src"]), ("bind", str(checkout / path)))
            for path in (".doc", ".workflow-kit"):
                with self.subTest(phase=phase, masked=path):
                    mount = mount_at(argv, f"/work/{path}")
                    self.assertEqual(mount["type"], "tmpfs")
                    self.assertIn("readonly", mount)
            self.assertFalse([m for m in mounts(argv) if m.get("dst") == "/work/.doc/scratch"])

    def test_configuration_anywhere_read_only(self):
        configuration = [
            "crates/m/Cargo.toml", "crates/m/uv.toml", "crates/p/pyproject.toml", "rust-toolchain",
            "sub/.python-version",
        ]
        checkout = self.checkout("c", "Makefile", *configuration, "crates/m/.cargo/config.toml", "crates/m/src/lib.rs")
        for phase in arguments.Phase:
            argv = self.phase_args(phase, checkout)
            for path in configuration + ["crates/m/.cargo"]:
                with self.subTest(phase=phase, path=path):
                    mount = mount_at(argv, f"/work/{path}")
                    self.assertEqual((mount["type"], mount["src"]), ("bind", str(checkout / path)))
                    self.assertIn("readonly", mount)
            self.assertFalse([m for m in mounts(argv) if m.get("dst") == "/work/crates/m/src/lib.rs"])

    def test_names_the_host_loads_read_only(self):
        loaded = [
            "crates/oxedi_py/tests/conftest.py", "crates/oxedi_py/pytest.ini", "setup.cfg", "tox.ini", "noxfile.py",
            "crates/p/sitecustomize.py", "crates/p/usercustomize.py", "sub/.envrc",
        ]
        checkout = self.checkout("c", "Makefile", *loaded, "crates/oxedi_py/tests/test_x.py")
        for phase in arguments.Phase:
            argv = self.phase_args(phase, checkout)
            for path in loaded:
                with self.subTest(phase=phase, path=path):
                    mount = mount_at(argv, f"/work/{path}")
                    self.assertEqual((mount["type"], mount["src"]), ("bind", str(checkout / path)))
                    self.assertIn("readonly", mount)
            self.assertFalse([m for m in mounts(argv) if m.get("dst") == "/work/crates/oxedi_py/tests/test_x.py"])

    def test_absent_root_names_held_in_the_run_only(self):
        checkout = self.checkout("c", "Makefile")
        empty = str(self.own(checkout) / "empty")
        run = self.phase_args(arguments.Phase.RUN, checkout)
        for name in ("GNUmakefile", "makefile", "conftest.py"):
            with self.subTest(name=name):
                mount = mount_at(run, f"/work/{name}")
                self.assertEqual((mount["type"], mount["src"]), ("bind", empty))
                self.assertIn("readonly", mount)
        for phase in (arguments.Phase.FETCH, arguments.Phase.AUDIT):
            with self.subTest(phase=phase):
                destinations = {m.get("dst") for m in mounts(self.phase_args(phase, checkout))}
                self.assertFalse(destinations & {"/work/GNUmakefile", "/work/makefile", "/work/conftest.py"})

    def test_a_present_root_name_is_not_held(self):
        checkout = self.checkout("c", "Makefile", "conftest.py", "GNUmakefile")
        run = self.phase_args(arguments.Phase.RUN, checkout)
        conftest = mount_at(run, "/work/conftest.py")
        self.assertEqual((conftest["src"], "readonly" in conftest), (str(checkout / "conftest.py"), True))
        self.assertEqual(mount_at(run, "/work/GNUmakefile")["src"], str(checkout / "GNUmakefile"))
        self.assertEqual(mount_at(run, "/work/makefile")["src"], str(self.own(checkout) / "empty"))

    def test_ignored_paths_masked_outputs_not(self):
        outputs = [
            "target/debug/x", ".venv/bin/python", "crates/a/__pycache__/x.cpython-313.pyc",
            "crates/oxedi_py/.pytest_cache/v/x", "crates/oxedi_py/python/oxedi/_core.abi3.so",
        ]
        tracked = [
            "crates/a/lib.py", "crates/oxedi_py/README.md", "crates/oxedi_py/python/oxedi/__init__.py", "tools/run.sh",
        ]
        ignored = [".venv-docs/bin/python", "notes.log", "tools/.env"]
        checkout = self.checkout("c", ".gitignore", "Makefile", *ignored, *tracked, *outputs)
        for phase in arguments.Phase:
            argv = self.phase_args(phase, checkout)
            with self.subTest(phase=phase):
                docs = mount_at(argv, "/work/.venv-docs")
                self.assertEqual(docs["type"], "tmpfs")
                self.assertIn("readonly", docs)
                for hidden in ("notes.log", "tools/.env"):
                    blank = mount_at(argv, f"/work/{hidden}")
                    self.assertEqual((blank["type"], blank["src"]), ("bind", str(self.own(checkout) / "empty")))
                    self.assertIn("readonly", blank)
                for path in outputs:
                    self.assertFalse([m for m in mounts(argv) if f"/work/{path}".startswith(m["dst"] + "/")
                                      and m["dst"] != "/work" and m["type"] == "tmpfs"], path)
                    self.assertFalse([m for m in mounts(argv) if m["dst"] == f"/work/{path}"], path)
        run = self.phase_args(arguments.Phase.RUN, checkout)
        self.assertEqual(mount_at(run, "/work/target")["type"], "bind")
        self.assertNotIn("readonly", mount_at(run, "/work/target"))

    def submodule_checkout(self, declared: str) -> Path:
        """A checkout with a registered gitlink at crates/sub and a .gitmodules that also names
        `declared`."""
        checkout = self.checkout("c", "Makefile", "crates/sub/", "crates/declared/x.mk")
        (checkout / ".gitmodules").write_text(
            '[submodule "sub"]\n\tpath = crates/sub\n\turl = https://example.invalid/sub\n'
            f'[submodule "declared"]\n\tpath = {declared}\n\turl = https://example.invalid/declared\n'
        )
        subprocess.run(["git", "-C", str(checkout), "update-index", "--add", "--cacheinfo",
                        f"160000,{'1' * 40},crates/sub"], check=True)
        subprocess.run(["git", "-C", str(checkout), "add", ".gitmodules"], check=True)
        return checkout

    def test_submodules_read_only(self):
        checkout = self.submodule_checkout("crates/declared")
        for phase in arguments.Phase:
            argv = self.phase_args(phase, checkout)
            for path in ("crates/sub", "crates/declared", ".gitmodules"):
                with self.subTest(phase=phase, path=path):
                    mount = mount_at(argv, f"/work/{path}")
                    self.assertEqual((mount["type"], mount["src"]), ("bind", str(checkout / path)))
                    self.assertIn("readonly", mount)

    def test_submodule_path_outside_the_checkout_refused(self):
        checkout = self.submodule_checkout("../outside")
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 2)
        self.assertEqual(
            result.stderr,
            "sandbox: refused: .gitmodules names the submodule path '../outside', outside the checkout\n",
        )
        self.assertFalse(self.runs())

    def test_not_a_git_checkout_refused(self):
        checkout = self.tmp / "plain"
        checkout.mkdir()
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 2)
        self.assertRegex(result.stderr, r"^sandbox: git ls-files .* failed in .*; the sandbox needs a git checkout")
        self.assertEqual(len(result.stderr.splitlines()), 1)
        self.assertFalse(self.runs())

    def test_a_git_error_of_several_lines_is_refused_in_one(self):
        # git outside a checkout may add a second line, such as the filesystem boundary note.
        failed = subprocess.CompletedProcess([], 128, b"", b"fatal: not a git repository\nStopping at filesystem boundary.\n")
        with mock.patch.object(layout.subprocess, "run", return_value=failed):
            with self.assertRaises(layout.SandboxError) as caught:
                layout.git_run(self.tmp, "ls-files")
        self.assertEqual(
            str(caught.exception),
            f"git ls-files failed in {self.tmp}; the sandbox needs a git checkout: "
            "fatal: not a git repository; Stopping at filesystem boundary.",
        )

    def test_masks_only_what_exists(self):
        checkout = self.checkout("c", "Makefile")
        for phase in arguments.Phase:
            destinations = {m.get("dst") for m in mounts(self.phase_args(phase, checkout))}
            self.assertFalse(destinations & {"/work/.doc", "/work/.workflow-kit", "/work/.cargo", "/work/tools"})

    def test_symlink_in_read_only_set_refused(self):
        checkout = self.checkout("c", "Makefile")
        (checkout / "tools").symlink_to(self.home / ".ssh")
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 2)
        self.assertIn("sandbox: refused: tools is a symbolic link", result.stderr)
        self.assertFalse(self.runs())

    def test_symlinked_parent_of_a_fixed_path_refused(self):
        # A linked parent would have the host file of the fixed name bound into the containers.
        checkout = self.checkout("c", "Makefile", "crates/README.md")
        outside = self.tmp / "host-oxedi_py"
        outside.mkdir()
        (outside / "pyproject.toml").write_text("host\n")
        (checkout / "crates" / "oxedi_py").symlink_to(outside)
        result = self.sandbox(checkout)
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, "sandbox: refused: crates/oxedi_py is a symbolic link; "
                             "the read-only and masked paths must be real files of the checkout"))
        self.assertEqual(self.runs(), [])


class Runner(World):
    def test_host_secrets_never_pass(self):
        checkout = self.checkout()
        result = self.sandbox(checkout, **SECRETS)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(self.calls())
        for call in self.calls():
            for argument in call["argv"]:
                for name, value in SECRETS.items():
                    self.assertNotIn(value, argument)
                    self.assertNotIn(name, argument)
        for call in self.runs():
            for mount in mounts(call["argv"]):
                source = mount.get("src", "")
                self.assertNotEqual(source, str(self.home))
                self.assertFalse(source.startswith(str(self.home / ".ssh")))
                self.assertNotIn("docker.sock", source)

    def test_cmd_through_make(self):
        command = 'cargo test -p oxedi_core --lib value:: && echo "a b; c" $$HOME'
        checkout = make_repo_copy(self.tmp / "repo")
        cache = checkout / "crates" / "__pycache__"
        cache.mkdir()
        result = self.make(checkout, "sandbox", f"CMD={command}")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        # The audit scrubbed the copy it ran over.
        self.assertFalse(cache.exists())
        argv = self.run_phase()
        self.assertEqual(argv[-3:-1], ["bash", "-c"])
        self.assertEqual(argv[-1], 'mkdir -p "$HOME" && cargo test -p oxedi_core --lib value:: && echo "a b; c" $HOME')

    def test_a_variable_from_an_exported_makeflags_is_refused(self):
        checkout = make_repo_copy(self.tmp / "repo")
        cases = (
            ("sandbox-pr", ["REF=x"], {"MAKEFLAGS": "CONFIRM=yes"}, "sandbox-pr: CONFIRM comes from MAKEFLAGS in "
             "the environment; unset MAKEFLAGS and pass it as make sandbox-pr CONFIRM=yes"),
            ("sandbox-pr", ["REF=x"], {"GNUMAKEFLAGS": "CONFIRM=yes"}, "sandbox-pr: CONFIRM comes from GNUMAKEFLAGS "
             "in the environment; unset GNUMAKEFLAGS and pass it as make sandbox-pr CONFIRM=yes"),
            ("sandbox-pr", [], {"MAKEFLAGS": "-k -- PR:=7"}, "sandbox-pr: PR comes from MAKEFLAGS in the "
             "environment; unset MAKEFLAGS and pass it as make sandbox-pr PR=251"),
            ("sandbox", ["CMD=true"], {"MAKEFLAGS": "IMAGE=viewer"}, "sandbox: IMAGE comes from MAKEFLAGS in the "
             "environment; unset MAKEFLAGS and pass it as make sandbox IMAGE=viewer"),
            ("sandbox", [], {"MAKEFLAGS": "CMD=true"}, "sandbox: CMD comes from MAKEFLAGS in the environment; "
             'unset MAKEFLAGS and pass it as make sandbox CMD="make gates"'),
            ("sandbox-check", [], {"GNUMAKEFLAGS": "IMAGE=viewer"}, "sandbox-check: IMAGE comes from GNUMAKEFLAGS "
             "in the environment; unset GNUMAKEFLAGS and pass it as make sandbox-check IMAGE=viewer"),
        )
        for target, args, env, line in cases:
            with self.subTest(target=target, env=env):
                result = self.make(checkout, target, *args, **env)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stderr.splitlines()[0], line)
                self.assertEqual(self.calls(), [])
        result = self.make(checkout, "sandbox", "CMD=true", MAKEFLAGS="-k")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_image_built_when_missing(self):
        result = self.make(make_repo_copy(self.tmp / "repo"), "sandbox", "CMD=true", FAKE_NO_IMAGE="1")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        builds = [c["argv"] for c in self.calls() if c["argv"][:1] == ["build"]]
        self.assertEqual(len(builds), 2)
        self.assertTrue(any(a.startswith("oxedi.sandbox.apt=") for a in builds[1]))
        helper = [c["argv"] for c in self.runs() if "cat" in c["argv"]]
        self.assertEqual(len(helper), 1)
        self.assertTrue(has_pair(helper[0], "--user", "65534:65534"))
        self.assertTrue(has_pair(helper[0], "--security-opt", "no-new-privileges"))
        self.run_phase()

    def test_cmd_required(self):
        result = self.sandbox(self.checkout(), SANDBOX_CMD="")
        self.assertEqual(result.returncode, 2)
        self.assertIn('sandbox: no command: set CMD, for example make sandbox CMD="make gates"', result.stderr)

    def test_storage_per_checkout(self):
        first, second = self.checkout("a"), self.checkout("b")
        for checkout in (first, second):
            self.assertEqual(self.sandbox(checkout).returncode, 0)
        cache = self.home / ".cache" / "oxedi-sandbox"
        runs = [c for c in self.runs() if "bash" in c["argv"]]
        self.assertEqual(len(runs), 2)
        targets = [mount_at(c["argv"], "/work/target")["src"] for c in runs]
        venvs = [mount_at(c["argv"], "/work/.venv")["src"] for c in runs]
        self.assertEqual(targets, [str(cache / layout.path_hash(c) / "target") for c in (first, second)])
        self.assertEqual(venvs, [str(cache / layout.path_hash(c) / "venv") for c in (first, second)])
        self.assertNotEqual(targets[0], targets[1])
        cargo = {m["src"] for c in runs for m in mounts(c["argv"]) if m.get("dst") == arguments.CARGO_HOME}
        self.assertEqual(cargo, {str(cache / "cargo")})
        self.assertTrue((cache / "uv").is_dir())
        for call in runs:
            for source, state in call["binds"].items():
                with self.subTest(source=source):
                    self.assertEqual((state["exists"], state["uid"]), (True, UID))
            for destination in ("/work/target", "/work/.venv"):
                with self.subTest(mount_point=destination):
                    self.assertIs(call["binds"][mount_at(call["argv"], destination)["src"]]["mount_point"], True)

    def test_ignored_file_hidden_by_an_empty_file(self):
        checkout = self.checkout("c", ".gitignore", "Makefile", ".env")
        (checkout / ".env").write_text("PYPI_TOKEN=pypi-sandboxtestsecret3\n")
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        argv = self.run_phase()
        blank = mount_at(argv, "/work/.env")
        empty = self.own(checkout) / "empty"
        self.assertEqual((blank["type"], blank["src"]), ("bind", str(empty)))
        self.assertIn("readonly", blank)
        self.assertIs([c for c in self.runs() if "bash" in c["argv"]][0]["binds"][str(empty)]["mount_point"], True)
        self.assertEqual(empty.read_bytes(), b"")
        self.assertEqual(empty.stat().st_mode & 0o777, 0o444 & ~current_umask())

    def test_absent_root_names_held_during_the_run_then_removed(self):
        checkout = self.checkout()
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("held in the run: GNUmakefile, makefile, conftest.py", result.stderr)
        call = [c for c in self.runs() if "bash" in c["argv"]][0]
        empty = str(self.own(checkout) / "empty")
        for name in ("GNUmakefile", "makefile", "conftest.py"):
            with self.subTest(name=name):
                self.assertEqual(mount_at(call["argv"], f"/work/{name}")["src"], empty)
                self.assertIs(call["binds"][empty]["mount_point"], True)
                self.assertFalse(os.path.lexists(checkout / name))
        self.assertNotIn("the run left a new path", result.stderr)

    def test_a_held_name_the_run_filled_is_left_for_the_audit(self):
        checkout = self.checkout()
        result = self.sandbox(checkout, FAKE_RUN_WRITES="chmod u+w GNUmakefile && echo 'all:' > GNUmakefile")
        self.assertEqual(result.returncode, 3, result.stderr)
        self.assertIn("sandbox: the run left a new path GNUmakefile", result.stderr)
        self.assertEqual((checkout / "GNUmakefile").read_text(), "all:\n")

    def test_xdg_cache_home_holds_the_storage(self):
        checkout = self.checkout()
        xdg = self.tmp / "xdg"
        self.assertEqual(self.sandbox(checkout, XDG_CACHE_HOME=str(xdg)).returncode, 0)
        target = mount_at(self.run_phase(), "/work/target")["src"]
        self.assertEqual(target, str(xdg / "oxedi-sandbox" / layout.path_hash(checkout) / "target"))

    def test_runsc_when_listed(self):
        checkout = self.checkout()
        info = {"SecurityOptions": [], "Runtimes": {"runc": {}, "runsc": {}}, "NCPU": 4, "MemTotal": 8 << 30}
        result = self.sandbox(checkout, FAKE_INFO=json.dumps(info))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(has_pair(self.run_phase(), "--runtime", "runsc"))
        self.assertIn("runtime runsc", result.stderr)

    def test_runc_otherwise(self):
        result = self.sandbox(self.checkout())
        self.assertNotIn("--runtime", self.run_phase())
        self.assertIn("runtime runc", result.stderr)

    def test_rootless_refused(self):
        info = {"SecurityOptions": ["name=seccomp,profile=builtin", "name=rootless"], "Runtimes": {"runc": {}},
                "NCPU": 4, "MemTotal": 8 << 30}
        result = self.sandbox(self.checkout(), FAKE_INFO=json.dumps(info))
        self.assertEqual(result.returncode, 2)
        self.assertEqual(
            result.stderr,
            "sandbox: refused: the Docker daemon is rootless (name=rootless in docker info); "
            "use a rootful Docker Engine or Docker Desktop\n",
        )
        self.assertFalse(self.runs())

    def test_limits_default_from_the_daemon(self):
        result = self.sandbox(self.checkout())
        argv = self.run_phase()
        self.assertTrue(has_pair(argv, "--cpus", "15"))
        self.assertTrue(has_pair(argv, "--memory", "12g"))
        self.assertIn("15 CPUs, 12 GB, 3600 s", result.stderr)

    def test_limits_from_the_knobs(self):
        result = self.sandbox(self.checkout(), SANDBOX_CPUS="2", SANDBOX_MEMORY="5", SANDBOX_TIMEOUT="30")
        argv = self.run_phase()
        self.assertTrue(has_pair(argv, "--cpus", "2"))
        self.assertTrue(has_pair(argv, "--memory-swap", "5g"))
        self.assertIn("2 CPUs, 5 GB, 30 s", result.stderr)

    def test_knob_not_a_positive_integer_refused(self):
        for value in ("abc", "0", "-1", "1.5", "\u00b2"):
            with self.subTest(value=value):
                result = self.sandbox(self.checkout(f"c{value}"), SANDBOX_MEMORY=value)
                self.assertEqual(result.returncode, 2)
                self.assertIn(f'sandbox: SANDBOX_MEMORY must be a positive whole number of GB, got "{value}"',
                              result.stderr)

    def test_already_running_refused(self):
        checkout = self.checkout()
        name = f"oxedi-sandbox-{layout.path_hash(checkout)}"
        result = self.sandbox(checkout, FAKE_RUNNING=name)
        self.assertEqual(result.returncode, 2)
        self.assertIn(f"sandbox: {name} is already running for this checkout", result.stderr)
        self.assertIn(f"docker kill {name}", result.stderr)
        self.assertFalse(self.runs())

    def test_docker_missing_explained(self):
        env = self.env()
        self.bin.joinpath("docker").unlink()
        env["PATH"] = str(self.bin)
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--image", "oxedi-sandbox:test", "--kind", "base", "--checkout",
             str(self.checkout())],
            env=env, capture_output=True, text=True, check=False,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("sandbox: docker was not found on PATH", result.stderr)

    def test_missing_image_explained(self):
        result = self.sandbox(self.checkout(), FAKE_NO_IMAGE="1")
        self.assertEqual(result.returncode, 2)
        self.assertIn("sandbox: image oxedi-sandbox:test not found; build it with make sandbox-image", result.stderr)

    def test_start_failure_distinct(self):
        cause = "docker: Error response from daemon: invalid mount config for type bind"
        result = self.sandbox(self.checkout(), FAKE_RUN_EXIT="125", FAKE_RUN_STDERR=cause + "\n")
        self.assertEqual(result.returncode, 125)
        self.assertIn(
            f"sandbox: docker could not start the container, or the command itself exited 125: {cause}",
            result.stderr,
        )
        self.assertNotIn("command exited", result.stderr)

    def test_command_status_passes_through(self):
        result = self.sandbox(self.checkout(), FAKE_RUN_EXIT="3")
        self.assertEqual(result.returncode, 3)
        self.assertIn("sandbox: command exited 3", result.stderr)

    def test_oom_hint(self):
        result = self.sandbox(self.checkout(), FAKE_RUN_EXIT="137", SANDBOX_MEMORY="6")
        self.assertEqual(result.returncode, 137)
        self.assertIn(
            "sandbox: the container was killed (exit 137), most likely by its 6 GB memory limit; "
            "raise it with SANDBOX_MEMORY=<GB>",
            result.stderr,
        )

    def test_timeout_kills(self):
        checkout = self.checkout()
        started = time.monotonic()
        result = self.sandbox(checkout, FAKE_RUN_SLEEP="30", SANDBOX_TIMEOUT="1")
        self.assertLess(time.monotonic() - started, 20)
        self.assertEqual(result.returncode, 124)
        self.assertIn("sandbox: the command ran past SANDBOX_TIMEOUT=1 s; the container was killed", result.stderr)
        name = f"oxedi-sandbox-{layout.path_hash(checkout)}"
        self.assertIn(["kill", name], [c["argv"] for c in self.calls()])

    def test_ctrl_c_kills(self):
        checkout = self.checkout()
        process = self.start(checkout, FAKE_RUN_SLEEP="30")
        deadline = time.monotonic() + 20
        while not (self.fake / "run.pid").exists() and time.monotonic() < deadline:
            time.sleep(0.05)
        process.send_signal(signal.SIGINT)
        _, err = process.communicate(timeout=20)
        self.assertEqual(process.returncode, 130)
        self.assertIn("sandbox: interrupted by SIGINT; the container, if it had started, was killed", err)
        name = f"oxedi-sandbox-{layout.path_hash(checkout)}"
        self.assertIn(["kill", name], [c["argv"] for c in self.calls()])

    def test_sighup_kills_and_removes_the_placeholders(self):
        checkout = self.checkout()
        process = self.start(checkout, FAKE_RUN_SLEEP="30")
        self.wait_for_run()
        self.assertTrue((checkout / "GNUmakefile").exists())
        process.send_signal(signal.SIGHUP)
        _, err = process.communicate(timeout=20)
        self.assertEqual(process.returncode, 129, err)
        self.assertIn("sandbox: interrupted by SIGHUP; the container, if it had started, was killed", err)
        self.assertEqual([n for n in ("GNUmakefile", "makefile", "conftest.py") if (checkout / n).exists()], [])

    def test_an_ignored_sighup_stays_ignored(self):
        checkout = self.checkout()
        process = subprocess.Popen(
            [sys.executable, "-I", str(SCRIPT), "--image", "oxedi-sandbox:test", "--kind", "base",
             "--checkout", str(checkout)],
            env=self.env(FAKE_RUN_SLEEP="2"), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
            preexec_fn=lambda: signal.signal(signal.SIGHUP, signal.SIG_IGN),
        )
        self.wait_for_run()
        process.send_signal(signal.SIGHUP)
        _, err = process.communicate(timeout=20)
        self.assertEqual(process.returncode, 0, err)
        self.assertNotIn("interrupted", err)
        name = f"oxedi-sandbox-{layout.path_hash(checkout)}"
        self.assertNotIn(["kill", name], [c["argv"] for c in self.calls()])

    def test_sigterm_kills(self):
        checkout = self.checkout()
        process = self.start(checkout, FAKE_RUN_SLEEP="30")
        self.wait_for_run()
        process.send_signal(signal.SIGTERM)
        _, err = process.communicate(timeout=20)
        self.assertEqual(process.returncode, 143)
        self.assertIn("sandbox: interrupted by SIGTERM; the container, if it had started, was killed", err)
        self.assertNotIn("Traceback", err)
        name = f"oxedi-sandbox-{layout.path_hash(checkout)}"
        self.assertIn(["kill", name], [c["argv"] for c in self.calls()])

    def test_a_failed_audit_keeps_the_first_error(self):
        plan = mock.Mock()
        failure = layout.SandboxError("git status failed in /c; the sandbox needs a git checkout: boom")
        work = mock.Mock(side_effect=sandbox.Stopped(signal.SIGINT))
        with (mock.patch.object(sandbox, "snapshot"),
              mock.patch.object(sandbox, "conclude", side_effect=failure),
              mock.patch("sys.stderr", new_callable=io.StringIO) as err):
            with self.assertRaises(sandbox.Stopped) as caught:
                sandbox.audited(plan, work)
        self.assertEqual(caught.exception.signum, signal.SIGINT)
        self.assertEqual(
            err.getvalue(),
            "sandbox: the audit failed as well, so review the checkout with git status and git diff: "
            "git status failed in /c; the sandbox needs a git checkout: boom\n",
        )

    def test_no_kill_after_a_normal_exit(self):
        self.assertEqual(self.sandbox(self.checkout()).returncode, 0)
        self.assertFalse([c for c in self.calls() if c["argv"][:1] == ["kill"]])


class EmptyDir(unittest.TestCase):
    def test_an_unreadable_directory_is_emptied(self):
        with tempfile.TemporaryDirectory() as tmp:
            venv = Path(tmp) / "venv"
            locked = venv / "x" / "deeper"
            locked.mkdir(parents=True)
            (locked / "file").write_text("x")
            (venv / "x" / "file").write_text("x")
            outside = Path(tmp) / "outside"
            outside.mkdir()
            (outside / "keep").write_text("x")
            (venv / "link").symlink_to(outside)
            locked.chmod(0)
            (venv / "x").chmod(0)
            venv.chmod(0)
            storage.empty_dir(venv)
            self.assertEqual(list(venv.iterdir()), [])
            self.assertTrue((outside / "keep").exists())

    def test_a_tree_deeper_than_the_recursion_limit_is_emptied(self):
        with tempfile.TemporaryDirectory() as tmp:
            venv = Path(tmp) / "venv"
            venv.mkdir()
            deepest = venv
            for _ in range(sys.getrecursionlimit() + 200):
                deepest = deepest / "a"
                deepest.mkdir()
            (deepest / "file").write_text("x")
            storage.empty_dir(venv)
            self.assertEqual(list(venv.iterdir()), [])

    def test_what_cannot_be_removed_is_one_line(self):
        with tempfile.TemporaryDirectory() as tmp:
            venv = Path(tmp) / "venv"
            venv.mkdir()
            (venv / "pinned").write_text("x")
            refused = PermissionError(1, "Operation not permitted", str(venv / "pinned"))
            with mock.patch.object(Path, "unlink", side_effect=refused):
                with self.assertRaises(layout.SandboxError) as caught:
                    storage.empty_dir(venv)
            self.assertEqual(
                str(caught.exception),
                f"cannot empty {venv} before the fetch: [Errno 1] Operation not permitted: "
                f"'{venv / 'pinned'}'; remove that directory by hand",
            )


def current_umask() -> int:
    mask = os.umask(0)
    os.umask(mask)
    return mask


def command_of(argv: list[str]) -> list[str]:
    return argv[argv.index("oxedi-sandbox:test") + 1:]


CARGO_FETCH = ["cargo", "fetch", "--locked"]
UV_VENV = ["uv", "venv", "--managed-python", "--python", "3.13", "/work/.venv"]
UV_INSTALL = [
    "uv", "pip", "install", "--python", "/work/.venv/bin/python", "--no-build",
    "--group", "crates/oxedi_py/pyproject.toml:contributor",
]


class Fetch(World):
    def fetches(self) -> list[list[str]]:
        return [c["argv"] for c in self.runs() if "bash" not in c["argv"]]

    def venv(self, checkout: Path) -> Path:
        return self.home / ".cache" / "oxedi-sandbox" / layout.path_hash(checkout) / "venv"

    def test_fetch_commands_fixed(self):
        self.assertEqual(arguments.fetch_commands(arguments.ImageKind.BASE), [CARGO_FETCH, UV_VENV, UV_INSTALL])

    def test_fetch_runs_before_the_command(self):
        result = self.sandbox(self.checkout())
        self.assertEqual(result.returncode, 0, result.stderr)
        runs = [c["argv"] for c in self.runs()]
        self.assertEqual([command_of(a) for a in runs[:3]], [CARGO_FETCH, UV_VENV, UV_INSTALL])
        self.assertEqual(command_of(runs[3])[:2], ["bash", "-c"])
        for argv in runs[:3]:
            self.assertNotIn("--network", argv)
            self.assertIn("readonly", mount_at(argv, "/work"))
        self.assertRegex(result.stderr, r"(?m)^sandbox: fetch took \d+\.\d s$")
        self.assertRegex(result.stderr, r"(?m)^sandbox: run took \d+\.\d s$")

    def test_what_a_run_leaves_never_reaches_the_next_fetch(self):
        checkout = self.checkout()
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        clean = self.fetches()
        venv = self.venv(checkout)
        outside = self.tmp / "outside"
        (outside / "keep").mkdir(parents=True)
        site = venv / "lib" / "python3.13" / "site-packages"
        site.mkdir(parents=True)
        (site / "evil.pth").write_text("import os; os.system('touch /work/marker')\n")
        (venv / "bin").mkdir()
        (venv / "bin" / "python").symlink_to(outside / "keep")
        (venv / "outside-link").symlink_to(outside)
        (venv / "pyvenv.cfg").write_text("version_info = 3.13\n")
        (checkout / "uv.toml").write_text('index-url = "https://attacker.example/simple"\n')
        (checkout / ".python-version").write_text("3.11\n")
        (self.fake / "calls.jsonl").unlink()
        self.assertEqual(self.sandbox(checkout).returncode, 0)
        planted = self.fetches()
        self.assertEqual(planted, clean)
        for argv in planted:
            self.assertIn("UV_NO_CONFIG=1", envs(argv))
            self.assertFalse([m for m in mounts(argv) if m.get("dst") == "/work/target"])
            self.assertIn("readonly", mount_at(argv, "/work"))
        first = [c for c in self.runs() if "bash" not in c["argv"]][0]
        self.assertEqual(first["binds"][str(venv)]["entries"], [])
        self.assertTrue(venv.is_dir())
        self.assertTrue((outside / "keep").is_dir())

    def test_fetch_failure_stops_without_offline_fallback(self):
        result = self.sandbox(self.checkout(), FAKE_FETCH_EXIT="101")
        self.assertEqual(result.returncode, 101)
        self.assertEqual(len(self.fetches()), 1)
        self.assertFalse([c for c in self.runs() if "bash" in c["argv"]])
        self.assertIn(
            "sandbox: fetch failed: cargo fetch --locked exited 101; read its output above "
            "(no network, or a package or group the checkout does not declare)",
            result.stderr,
        )


VIEWER = arguments.ImageKind.VIEWER
DOCS_FETCH = [
    ["uv", "venv", "--managed-python", "--python", "3.13", "/work/.venv-docs"],
    ["uv", "pip", "install", "--python", "/work/.venv-docs/bin/python", "--no-build", "--require-hashes",
     "-r", "/work/docs/requirements.txt"],
    ["touch", "/work/.venv-docs/.ready"],
]
PIP_AUDIT = ["pip-audit", "--progress-spinner", "off", "--format", "json", "--path"]
PINNED = "playwright==1.64.0 \\\n    --hash=sha256:00\n"
INSTALL_TOOLS = (REPO / viewer_image.INSTALL_TOOLS).read_text()
WASM_PINS = viewer_image.wasm_pins(INSTALL_TOOLS)


def base_tag() -> str:
    """The base image tag make derives from this repository's sandbox Dockerfile, pip-audit.txt and
    rust-toolchain.toml."""
    inputs = b"".join(path.read_bytes() for path in (
        SCRIPT.parent / "Dockerfile", SCRIPT.parent / "pip-audit.txt", REPO / "rust-toolchain.toml"))
    return f"oxedi-sandbox:{hashlib.sha256(inputs).hexdigest()[:12]}"


class Images(World):
    def viewer_checkout(self, name: str = "viewer") -> Path:
        return self.checkout(name, ".gitignore", "Makefile", "docs/requirements.txt", ".venv-docs/bin/python")

    def pin(self, checkout: Path, text: str = PINNED) -> Path:
        """Pins Playwright in the checkout's docs/requirements.txt and the wasm tools as this
        repository's install_tools.sh does."""
        (checkout / "docs" / "requirements.txt").write_text(text)
        tools = checkout / viewer_image.INSTALL_TOOLS
        tools.parent.mkdir(parents=True, exist_ok=True)
        tools.write_text(INSTALL_TOOLS)
        return checkout

    def test_image_values(self):
        checkout = make_repo_copy(self.tmp / "repo")
        refusals = (
            ({"args": ["IMAGE=Viewer"]}, "sandbox: IMAGE must be base or viewer, not 'Viewer'"),
            ({"args": ["IMAGE=../x"]}, "sandbox: IMAGE must be base or viewer, not '../x'"),
            ({"args": ["IMAGE="]}, "sandbox: IMAGE must be base or viewer, not ''"),
            ({"args": [], "IMAGE": "viewer"},
             "sandbox: IMAGE is read from the command line only; pass it as make sandbox IMAGE=viewer"),
        )
        for case, line in refusals:
            with self.subTest(case=case):
                env = {k: v for k, v in case.items() if k != "args"}
                result = self.make(checkout, "sandbox", "CMD=true", *case["args"], **env)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stderr.splitlines()[0], line)
                self.assertEqual(self.calls(), [])
        pr = self.make(checkout, "sandbox-pr", "REF=x", "IMAGE=Viewer")
        self.assertEqual(pr.stderr.splitlines()[0], "sandbox-pr: IMAGE must be base or viewer, not 'Viewer'")
        self.assertEqual(self.calls(), [])
        check = self.make(checkout, "sandbox-check", IMAGE="viewer")
        self.assertEqual(check.stderr.splitlines()[0],
                         "sandbox-check: IMAGE is read from the command line only; "
                         "pass it as make sandbox-check IMAGE=viewer")
        self.assertEqual(self.calls(), [])

        runs = {}
        for args in ([], ["IMAGE=base"]):
            with self.subTest(args=args):
                result = self.make(checkout, "sandbox", "CMD=true", *args)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"image {base_tag()} (base, Rust 1.99.0)", result.stderr)
                runs[tuple(args)] = [c["argv"] for c in self.runs()]
                (self.fake / "calls.jsonl").unlink()
        self.assertEqual(runs[()], runs[("IMAGE=base",)])
        self.assertEqual([command_of_image(a, base_tag()) for a in runs[()][:3]], [CARGO_FETCH, UV_VENV, UV_INSTALL])
        for argv in runs[()]:
            self.assertEqual(argv[-len(command_of_image(argv, base_tag())) - 1], base_tag())
            self.assertFalse([m for m in mounts(argv) if m.get("dst") == "/work/.venv-docs" and m["type"] == "bind"])

        label = viewer_image.playwright_pin((REPO / "docs" / "requirements.txt").read_text())
        result = self.make(checkout, "sandbox", "CMD=true", "IMAGE=viewer", FAKE_PLAYWRIGHT_LABEL=label)
        self.assertEqual(result.returncode, 0, result.stderr)
        viewer_tag = viewer_image.tag(checkout, base_tag())
        self.assertIn(f"image {viewer_tag} (viewer, Rust 1.99.0)", result.stderr)
        argv = self.run_phase()
        self.assertEqual(command_of_image(argv, viewer_tag)[:2], ["bash", "-c"])
        self.assertEqual(mount_at(argv, "/work/.venv-docs")["src"], str(self.own(checkout) / "venv-docs"))

        (self.fake / "calls.jsonl").unlink()
        result = self.make(checkout, "sandbox-check", "IMAGE=viewer", FAKE_PLAYWRIGHT_LABEL=label)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(command_of_image(self.run_phase(), viewer_tag)[-1],
                         f'mkdir -p "$HOME" && bash tools/sandbox/isolation-check.sh "{self.home}"')

    def test_base_run_has_no_docs_venv(self):
        checkout = self.viewer_checkout()
        docs_storage = str(self.own(checkout) / "venv-docs")
        for phase in arguments.Phase:
            with self.subTest(phase=phase):
                argv = self.phase_args(phase, checkout)
                self.assertFalse([m for m in mounts(argv) if m.get("src") == docs_storage])
                self.assertEqual(mount_at(argv, "/work/.venv-docs")["type"], "tmpfs")
                self.assertFalse([a for a in argv if "OXEDI_CHROMIUM_NO_SANDBOX" in a])
        self.assertEqual(arguments.fetch_commands(arguments.ImageKind.BASE), [CARGO_FETCH, UV_VENV, UV_INSTALL])
        self.assertEqual(arguments.audit_commands(arguments.ImageKind.BASE)[1:],
                         [[*PIP_AUDIT, "/work/.venv/lib/python3.13/site-packages"]])
        result = self.sandbox(checkout)
        self.assertEqual(result.returncode, 0, result.stderr)
        for call in self.runs():
            self.assertFalse([m for m in mounts(call["argv"]) if m.get("src") == docs_storage])
            self.assertFalse([a for a in call["argv"] if "OXEDI_CHROMIUM_NO_SANDBOX" in a])

    def test_viewer_fetch_installs_the_docs_venv(self):
        fetch = arguments.fetch_commands(VIEWER)
        self.assertEqual(fetch, [CARGO_FETCH, UV_VENV, UV_INSTALL, *DOCS_FETCH])
        self.assertIn("--require-hashes", fetch[-2])
        self.assertIn("--no-build", fetch[-2])
        self.assertEqual(arguments.audit_commands(VIEWER)[1:], [
            [*PIP_AUDIT, "/work/.venv/lib/python3.13/site-packages"],
            [*PIP_AUDIT, "/work/.venv-docs/lib/python3.13/site-packages"],
        ])
        checkout = self.pin(self.viewer_checkout())
        docs_storage = self.own(checkout) / "venv-docs"
        for phase, read_only in ((arguments.Phase.FETCH, False), (arguments.Phase.RUN, False),
                                 (arguments.Phase.AUDIT, True)):
            with self.subTest(phase=phase):
                mount = mount_at(self.phase_args(phase, checkout, VIEWER), "/work/.venv-docs")
                self.assertEqual((mount["type"], mount["src"]), ("bind", str(docs_storage)))
                self.assertEqual("readonly" in mount, read_only)
        # What a run left in the docs venv never reaches the next fetch.
        docs_storage.mkdir(parents=True)
        (docs_storage / "evil.pth").write_text("import os\n")
        result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        fetches = [c for c in self.runs() if "bash" not in c["argv"]]
        self.assertEqual([command_of(c["argv"]) for c in fetches], [CARGO_FETCH, UV_VENV, UV_INSTALL, *DOCS_FETCH])
        self.assertEqual(fetches[0]["binds"][str(docs_storage)]["entries"], [])
        self.assertIn("readonly", mount_at(fetches[0]["argv"], "/work"))

    def test_viewer_run_builds_into_its_own_pkg(self):
        # The host's built module is masked in every phase; the viewer image's runs build theirs in
        # the checkout's storage, so the host never serves what a run wrote.
        checkout = self.pin(self.checkout("viewer", ".gitignore", "Makefile", "docs/requirements.txt",
                                          "viewer/index.html", "viewer/pkg/module.js"))
        pkg_storage = self.own(checkout) / "viewer-pkg"
        for phase in arguments.Phase:
            with self.subTest(phase=phase, kind="base"):
                self.assertEqual(mount_at(self.phase_args(phase, checkout), "/work/viewer/pkg")["type"], "tmpfs")
        for phase, read_only in ((arguments.Phase.FETCH, True), (arguments.Phase.RUN, False),
                                 (arguments.Phase.AUDIT, True)):
            with self.subTest(phase=phase, kind="viewer"):
                mount = mount_at(self.phase_args(phase, checkout, VIEWER), "/work/viewer/pkg")
                self.assertEqual((mount["type"], mount["src"]), ("bind", str(pkg_storage)))
                self.assertEqual("readonly" in mount, read_only)
        # A checkout with no built module (a sandbox-pr clone) gets the mount point before the run
        # and its audit stays clean.
        fresh = self.pin(self.viewer_checkout("fresh"))
        result = self.sandbox(fresh, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("(target, venv, venv-docs, viewer-pkg)", result.stderr)
        run = [c for c in self.runs() if "bash" in c["argv"]][0]
        state = run["binds"][str(self.own(fresh) / "viewer-pkg")]
        self.assertEqual((state["exists"], state["mount_point"]), (True, True))
        # The host makes that mount point: a link on its path would have it made outside the checkout.
        linked = self.pin(self.viewer_checkout("linked"))
        outside = self.tmp / "outside-viewer"
        outside.mkdir()
        (linked / "viewer").symlink_to(outside)
        calls = len(self.runs())
        result = self.sandbox(linked, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, "sandbox: refused: viewer is a symbolic link; "
                             "the mount point viewer/pkg must be a real directory of the checkout"))
        self.assertEqual(len(self.runs()), calls)
        self.assertEqual(list(outside.iterdir()), [])

    def test_playwright_mismatch_refuses(self):
        checkout = self.pin(self.viewer_checkout())
        cases = (
            ({"FAKE_PLAYWRIGHT_LABEL": "1.63.0"},
             "sandbox: this checkout pins playwright 1.64.0 but the viewer image has 1.63.0; "
             "run make sandbox-image-viewer"),
            ({}, "sandbox: this checkout pins playwright 1.64.0 but the viewer image has no "
                 "oxedi.sandbox.playwright label; run make sandbox-image-viewer"),
        )
        for env, line in cases:
            with self.subTest(env=env):
                result = self.sandbox(checkout, "viewer", **env)
                self.assertEqual(result.returncode, 2, result.stderr)
                self.assertEqual(result.stderr.strip(), line)
                self.assertEqual(self.runs(), [])
        self.pin(checkout, "pytest==8.0.0\n")
        result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, "sandbox: docs/requirements.txt pins no playwright==<version>"))
        (checkout / "docs" / "requirements.txt").unlink()
        (checkout / "docs" / "requirements.txt").symlink_to(self.tmp / "elsewhere.txt")
        result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, "sandbox: refused: docs/requirements.txt is a symbolic link; "
                             "docs/requirements.txt must be a real file of the checkout"))
        self.assertEqual(self.runs(), [])
        outside = self.tmp / "outside-docs"
        outside.mkdir()
        (outside / "requirements.txt").write_text(PINNED)
        shutil.rmtree(checkout / "docs")
        (checkout / "docs").symlink_to(outside)
        result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, "sandbox: refused: docs is a symbolic link; docs/requirements.txt must be a real file of the checkout"))
        self.assertEqual(self.runs(), [])

    def test_wasm_pin_mismatch_refuses(self):
        # The image's wasm tools are those of the install_tools.sh it was built from: a checkout that
        # pins others is refused before any container, as a stale browser is.
        checkout = self.pin(self.viewer_checkout())
        bindgen, binaryen = WASM_PINS.bindgen_version, WASM_PINS.binaryen_version
        cases = (
            ({"FAKE_WASM_BINDGEN_LABEL": "0.2.1"},
             f"sandbox: this checkout pins wasm-bindgen {bindgen} but the viewer image has 0.2.1; "
             "run make sandbox-image-viewer"),
            ({"FAKE_BINARYEN_LABEL": "120"},
             f"sandbox: this checkout pins binaryen {binaryen} but the viewer image has 120; "
             "run make sandbox-image-viewer"),
            ({"FAKE_BINARYEN_LABEL": ""},
             f"sandbox: this checkout pins binaryen {binaryen} but the viewer image has no "
             "oxedi.sandbox.binaryen label; run make sandbox-image-viewer"),
        )
        for env, line in cases:
            with self.subTest(env=env):
                result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0", **env)
                self.assertEqual((result.returncode, result.stderr.strip()), (2, line))
                self.assertEqual(self.runs(), [])
        tools = checkout / viewer_image.INSTALL_TOOLS
        tools.write_text(INSTALL_TOOLS.replace(f'BINARYEN_VERSION="{binaryen}"', 'BINARYEN_VERSION="134"'))
        result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, f"sandbox: this checkout pins binaryen 134 but the viewer image has {binaryen}; "
                             "run make sandbox-image-viewer"))
        tools.write_text("#!/usr/bin/env bash\n")
        result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, f'sandbox: {viewer_image.INSTALL_TOOLS} has no WASM_BINDGEN_VERSION="<value>" line'))
        tools.unlink()
        elsewhere = self.tmp / "install_tools.sh"
        elsewhere.write_text(INSTALL_TOOLS)
        tools.symlink_to(elsewhere)
        result = self.sandbox(checkout, "viewer", FAKE_PLAYWRIGHT_LABEL="1.64.0")
        self.assertEqual((result.returncode, result.stderr.strip()),
                         (2, f"sandbox: refused: {viewer_image.INSTALL_TOOLS} is a symbolic link; "
                             f"{viewer_image.INSTALL_TOOLS} must be a real file of the checkout"))
        self.assertEqual(self.runs(), [])

    def test_storage_names_follow_the_own_mounts(self):
        # The plan line names the storage directories the runs mount, from the table the mounts use.
        checkout = self.viewer_checkout()
        surveyed = layout.survey(checkout)
        own = storage.storage_for(storage.cache_root({"HOME": str(self.home)}), surveyed)
        cases = (
            (arguments.ImageKind.BASE, {}, "(target, venv)"),
            (VIEWER, {}, "(target, venv, venv-docs, viewer-pkg)"),
            (arguments.ImageKind.BASE, {arguments.ImageKind.BASE: (arguments.VENV, arguments.VIEWER_PKG)},
             "(target, venv, viewer-pkg)"),
        )
        for kind, table, names in cases:
            with self.subTest(kind=kind, table=table), mock.patch.dict(arguments.OWN_MOUNTS, table):
                plan = sandbox.Plan("oxedi-sandbox:test", kind, surveyed, own, arguments.Runtime.RUNC, LIMITS)
                self.assertIn(f"storage {own.target.parent} {names},", sandbox.describe(plan, {}))

    def test_viewer_image_names_the_docker_server(self):
        # The viewer image's wasm tools are amd64 only; with no daemon there is no architecture to blame.
        checkout = make_repo_copy(self.tmp / "repo")
        cases = (
            ({}, "sandbox-image-viewer: the docker daemon could not be reached (docker version names no server); "
                 "start it and retry"),
            ({"FAKE_ARCH": "arm64"},
             "sandbox-image-viewer: the docker server is 'arm64' and the wasm tools are pinned for amd64"),
        )
        for env, line in cases:
            with self.subTest(env=env):
                result = self.make(checkout, "sandbox-image-viewer", **env)
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stderr.splitlines()[0], line)
                # No build or run: docker is only asked for its server's architecture.
                self.assertEqual({c["argv"][0] for c in self.calls()}, {"version"})
                (self.fake / "calls.jsonl").unlink(missing_ok=True)

    def test_image_refuses_a_channel_that_is_not_exact(self):
        checkout = make_repo_copy(self.tmp / "repo")
        for channel in ("stable", "1.99", "nightly-2026-01-01"):
            with self.subTest(channel=channel):
                (checkout / "rust-toolchain.toml").write_text(f'[toolchain]\nchannel = "{channel}"\n')
                result = self.make(checkout, "sandbox-image")
                self.assertNotEqual(result.returncode, 0, result.stderr)
                self.assertEqual(
                    result.stderr.splitlines()[0],
                    f"sandbox-image: rust-toolchain.toml names no exact channel X.Y.Z (found '{channel}'); "
                    "set channel to a version such as 1.99.0")
                self.assertEqual([c for c in self.calls() if c["argv"][0] == "build"], [])

    def test_image_builds_with_the_channel_of_the_file(self):
        checkout = make_repo_copy(self.tmp / "repo")
        result = self.make(checkout, "sandbox-image")
        self.assertEqual(result.returncode, 0, result.stderr)
        builds = [c["argv"] for c in self.calls() if c["argv"][0] == "build"]
        self.assertEqual(len(builds), 2)
        for argv in builds:
            self.assertIn("RUST_VERSION=1.99.0", argv)

    def test_base_tag_follows_rust_toolchain(self):
        checkout = make_repo_copy(self.tmp / "repo")

        def tag() -> str:
            result = self.make(checkout, "sandbox", "CMD=true")
            self.assertEqual(result.returncode, 0, result.stderr)
            return re.search(r"image (oxedi-sandbox:\S+) \(base", result.stderr).group(1)

        before = tag()
        self.assertTrue(before.startswith("oxedi-sandbox:"))
        (checkout / "rust-toolchain.toml").write_text('[toolchain]\nchannel = "1.99.1"\n')
        self.assertNotEqual(tag(), before)

    def test_kind_required_with_image(self):
        result = subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--image", "oxedi-sandbox:test", "--checkout", str(self.checkout())],
            env=self.env(), capture_output=True, text=True, check=False, timeout=60,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn("--image needs --kind base or --kind viewer", result.stderr)
        self.assertEqual(self.calls(), [])


def command_of_image(argv: list[str], image: str) -> list[str]:
    return argv[argv.index(image) + 1:]


if __name__ == "__main__":
    unittest.main()
