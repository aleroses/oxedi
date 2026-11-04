"""The `docker run` arguments of each phase: one hardened base plus what the phase changes."""

from dataclasses import dataclass
from enum import Enum
from pathlib import Path

from layout import Checkout, covered
from storage import Storage

WORK = "/work"
HOME = "/tmp/home"
CARGO_HOME = "/sandbox/cargo"
UV_CACHE = "/sandbox/uv"
# Docker mounts a --tmpfs noexec by default; rustdoc builds and runs every doctest from the
# temporary directory, so /tmp is exec (still nosuid and nodev).
TMP_MOUNT = "/tmp:rw,exec,nosuid,nodev,size=2g"
VENV = f"{WORK}/.venv"
# The viewer image's docs venv: the site tools, Playwright and pytest, from the hashed
# docs/requirements.txt. `make docs-venv` checks its stamp.
DOCS_VENV = f"{WORK}/.venv-docs"
DOCS_REQUIREMENTS = f"{WORK}/docs/requirements.txt"
# The Python the venv is made with, and the dependency group the fetch installs into it.
VENV_PYTHON = "3.13"
CONTRIBUTOR_GROUP = "crates/oxedi_py/pyproject.toml:contributor"
# The audit tools' policy and advisory database. The policy is this module's own copy, from the
# checkout that runs the sandbox: a change under review never chooses the rules it is judged by.
HOST_DENY_TOML = Path(__file__).resolve().parent / "deny.toml"
DENY_TOML = "/sandbox/deny.toml"
ADVISORY = "/sandbox/advisory"


class ImageKind(Enum):
    """The image a run uses: the base one, or the viewer one built on it with Node, Chromium and the
    wasm tools."""

    BASE = "base"
    VIEWER = "viewer"


# The venvs each image's runs mount, by their place in the container.
VENVS = {ImageKind.BASE: (VENV,), ImageKind.VIEWER: (VENV, DOCS_VENV)}
# The viewer's built module and its glue (make wasm): the viewer image's runs build it in the
# checkout's storage, so the host's copy is never one a run wrote.
VIEWER_PKG = f"{WORK}/viewer/pkg"
# What each image's runs mount from the checkout's storage over the checkout: the venvs and, for the
# viewer image, its built module.
OWN_MOUNTS = {ImageKind.BASE: VENVS[ImageKind.BASE], ImageKind.VIEWER: (*VENVS[ImageKind.VIEWER], VIEWER_PKG)}


class Phase(Enum):
    FETCH = "fetch"
    RUN = "run"
    AUDIT = "audit"


class Runtime(Enum):
    RUNC = "runc"
    RUNSC = "runsc"

    def args(self) -> list[str]:
        """runc is the daemon's default and needs no flag."""
        return ["--runtime", self.value] if self is Runtime.RUNSC else []


@dataclass(frozen=True)
class Limits:
    cpus: int
    memory_gb: int
    timeout_s: int


def bind(source: Path, destination: str, read_only: bool) -> list[str]:
    return ["--mount", f"type=bind,src={source},dst={destination}" + (",readonly" if read_only else "")]


def mask(destination: str) -> list[str]:
    return ["--mount", f"type=tmpfs,dst={destination},readonly,tmpfs-size=4096"]


def env_args(variables: dict[str, str]) -> list[str]:
    """Always NAME=value: a bare `--env NAME` would copy the host's value."""
    return [a for name, value in variables.items() for a in ("--env", f"{name}={value}")]


def hardened_base(checkout: Checkout, uid: int, gid: int, runtime: Runtime, limits: Limits) -> list[str]:
    memory = f"{limits.memory_gb}g"
    return [
        "--rm", "--init", "--pull", "never", "--name", checkout.name, "--user", f"{uid}:{gid}",
        "--read-only", "--cap-drop", "ALL", "--security-opt", "no-new-privileges",
        "--pids-limit", "4096", "--memory", memory, "--memory-swap", memory, "--cpus", str(limits.cpus),
        "--tmpfs", TMP_MOUNT, "--workdir", WORK, *runtime.args(),
    ]


def own_sources(storage: Storage, kind: ImageKind) -> dict[str, Path]:
    """Each of the image's own mounts, by its place in the container, with its source in the
    checkout's storage."""
    sources = {VENV: storage.venv, DOCS_VENV: storage.docs_venv, VIEWER_PKG: storage.viewer_pkg}
    return {place: sources[place] for place in OWN_MOUNTS[kind]}


def own_binds(storage: Storage, kind: ImageKind, writable: tuple[str, ...]) -> list[str]:
    """A bind of each of the image's own mounts from the checkout's storage, read-only unless its
    place is in `writable`."""
    sources = own_sources(storage, kind).items()
    return [a for place, source in sources for a in bind(source, place, place not in writable)]


def own_dirs(kind: ImageKind) -> tuple[str, ...]:
    """The image's own mounts as paths of the checkout: their mount points."""
    return tuple(place.removeprefix(f"{WORK}/") for place in OWN_MOUNTS[kind])


def read_only_set(checkout: Checkout, storage: Storage, skip: tuple[str, ...], kind: ImageKind) -> list[str]:
    """The read-only binds, the masked directories and the empty file over each hidden one."""
    binds = [bind(checkout.root / p, f"{WORK}/{p}", True) for p in checkout.read_only if p not in skip]
    # A bind from the storage stands in for what the host holds there, so that path is neither
    # masked nor hidden.
    own = set(own_dirs(kind))
    masks = [mask(f"{WORK}/{p}") for p in checkout.masked if not covered(p, own)]
    blanks = [bind(storage.empty, f"{WORK}/{p}", True) for p in checkout.hidden if not covered(p, own)]
    return [a for mount in binds + masks + blanks for a in mount]


def placeholder_binds(checkout: Checkout, storage: Storage) -> list[str]:
    """The empty read-only file over each absent root name the host loads: the run cannot create
    one."""
    return [a for name in checkout.placeholders for a in bind(storage.empty, f"{WORK}/{name}", True)]


def network_delta(checkout: Checkout, storage: Storage, kind: ImageKind, writable: tuple[str, ...]) -> list[str]:
    """Network on, the checkout read-only with .cargo masked (no repository cargo configuration
    applies), the shared caches writable, rustc wrappers cleared, the image's own mounts in place
    and only the `writable` ones writable."""
    cargo_mask = mask(f"{WORK}/.cargo") if ".cargo" in checkout.read_only else []
    return [
        *env_args({
            "HOME": HOME, "CARGO_HOME": CARGO_HOME, "CARGO_BUILD_RUSTC_WRAPPER": "",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER": "", "UV_CACHE_DIR": UV_CACHE,
            # The cache and the venv are separate mounts, so uv copies instead of hardlinking.
            "UV_LINK_MODE": "copy",
            # A uv.toml or [tool.uv] the run phase left in the checkout must not steer the fetch.
            "UV_NO_CONFIG": "1",
        }),
        *bind(checkout.root, WORK, True), *read_only_set(checkout, storage, (".cargo",), kind), *cargo_mask,
        *own_binds(storage, kind, writable),
        *bind(storage.cargo, CARGO_HOME, False), *bind(storage.uv, UV_CACHE, False),
    ]


def fetch_delta(checkout: Checkout, storage: Storage, kind: ImageKind) -> list[str]:
    """The network set with the venvs writable: the fetch installs into them."""
    return network_delta(checkout, storage, kind, VENVS[kind])


def audit_delta(checkout: Checkout, storage: Storage, kind: ImageKind) -> list[str]:
    """The network set with the venvs read-only, the host's cargo-deny policy read-only and the
    advisory database writable: the audit tools read the change and fetch advisories only."""
    return [
        *network_delta(checkout, storage, kind, ()),
        *bind(HOST_DENY_TOML, DENY_TOML, True), *bind(storage.advisory, ADVISORY, False),
    ]


def run_delta(checkout: Checkout, storage: Storage, kind: ImageKind) -> list[str]:
    """No network, the checkout writable except the read-only set and the absent names the host
    loads, the cargo registry read-only, no shared uv cache."""
    return [
        "--network", "none",
        *env_args({
            "HOME": HOME, "CARGO_HOME": CARGO_HOME, "UV_CACHE_DIR": "/tmp/uv-cache",
            "CARGO_NET_OFFLINE": "true", "UV_OFFLINE": "1", "CARGO_INCREMENTAL": "0",
        }),
        *bind(checkout.root, WORK, False), *read_only_set(checkout, storage, (), kind),
        *placeholder_binds(checkout, storage),
        *bind(storage.target, f"{WORK}/target", False), *own_binds(storage, kind, OWN_MOUNTS[kind]),
        *bind(storage.cargo, CARGO_HOME, True),
    ]


PHASE_DELTA = {Phase.FETCH: fetch_delta, Phase.RUN: run_delta, Phase.AUDIT: audit_delta}


def docker_args(
    phase: Phase, checkout: Checkout, uid: int, gid: int, runtime: Runtime, limits: Limits, storage: Storage,
    kind: ImageKind,
) -> list[str]:
    """The `docker run` options of a phase: the hardened base, then what the phase changes."""
    return hardened_base(checkout, uid, gid, runtime, limits) + PHASE_DELTA[phase](checkout, storage, kind)


def fetch_commands(kind: ImageKind) -> list[list[str]]:
    """The fetch phase, fixed here: it never runs make, a repository script, a build script or an
    sdist. `cargo fetch` builds nothing; `--no-build` lets uv install wheels only. The venv is made
    anew from the image's own CPython (`--managed-python`: the one under /opt/python), so the only
    interpreter the fetch executes is one no run has written. The viewer image also gets the docs
    venv, from the hashed docs/requirements.txt, and the stamp `make docs-venv` checks."""
    venv = ["uv", "venv", "--managed-python", "--python", VENV_PYTHON, VENV]
    install = ["uv", "pip", "install", "--python", f"{VENV}/bin/python", "--no-build", "--group", CONTRIBUTOR_GROUP]
    base = [["cargo", "fetch", "--locked"], venv, install]
    if kind is ImageKind.BASE:
        return base
    docs_venv = ["uv", "venv", "--managed-python", "--python", VENV_PYTHON, DOCS_VENV]
    docs_install = ["uv", "pip", "install", "--python", f"{DOCS_VENV}/bin/python", "--no-build", "--require-hashes",
                    "-r", DOCS_REQUIREMENTS]
    return [*base, docs_venv, docs_install, ["touch", f"{DOCS_VENV}/.ready"]]


def deny_command() -> list[str]:
    """cargo-deny over the change's Cargo.lock, with the host's policy. It is called by its own name:
    `cargo deny` would look in the writable $CARGO_HOME/bin first, where a binary planted in the
    shared cargo cache would run instead of the image's."""
    return ["cargo-deny", "--manifest-path", f"{WORK}/Cargo.toml", "check", "--config", DENY_TOML,
            "advisories", "sources", "bans"]


def pip_audit_command(venv: str) -> list[str]:
    """pip-audit over the packages the fetch installed in `venv`."""
    site_packages = f"{venv}/lib/python{VENV_PYTHON}/site-packages"
    return ["pip-audit", "--progress-spinner", "off", "--format", "json", "--path", site_packages]


def audit_commands(kind: ImageKind) -> list[list[str]]:
    """The audit tools, fixed here: cargo-deny before the fetch, then pip-audit over each venv of
    the image."""
    return [deny_command(), *(pip_audit_command(venv) for venv in VENVS[kind])]


def run_command(command: str) -> list[str]:
    return ["bash", "-c", f'mkdir -p "$HOME" && {command}']
