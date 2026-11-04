# Tools

Repository tools used both locally and in CI. A script that belongs to one crate stays in that
crate, and a script only CI runs stays in `.github/scripts/`.

## deps

The check that no development dependency reaches what oxedi publishes. `deps.py crates` compares
the normal dependencies `cargo tree` reports for `oxedi_core`, `oxedi_py`, `oxedi_duckdb` and
`oxedi_wasm` with the allowed list kept in the script; `deps.py wheel` requires every
`Requires-Dist` of a built wheel to carry an `extra ==` marker of a user extra. `make deps-check`
runs the crate check and, when `make dist` left a wheel in `target/wheels`, the wheel check;
`make gates` includes it. `tests/deps.sh` proves both fail on a leak, with a fake `cargo` and
hand-built wheels.

## complexity

The complexity ratchet. `complexity.sh` measures the cyclomatic complexity and nesting of every
Rust, Python and JS function with a pinned lizard and compares them with
`.github/complexity-baseline.txt` through `complexity.py`; `make complexity` and
`make complexity-update` run it. `complexity-baseline-diff.sh` reports, on a pull request, the
baseline entries the change adds or raises. `tests/complexity.sh` checks the comparison on
hand-written measurements. CONTRIBUTING.md (Complexity) explains the targets and the baseline.

## sandbox

The container `make sandbox` builds and tests in. `Dockerfile` is the pinned image, and
`Dockerfile.viewer` the viewer image built on it (`make sandbox-image-viewer`, run with
`IMAGE=viewer`): `viewer_image.py` derives its build arguments and tag from the repository's wasm
and Playwright pins and the digest of the installed Chromium tree (modes and symbolic links
included), and `playwright.txt` locks the Playwright it installs the browser with. `sandbox.py`
is the runner; its parts are `layout.py` (which paths are read-only, masked or hidden),
`storage.py` (the storage on the host and `make sandbox-clean`), `arguments.py` (the `docker run`
arguments of each phase, for either image), `lifecycle.py` (the daemon, the limits, one container's run and kill)
and `audit.py` (what a run changed or left in the checkout, and the record that blocks the next
start until it is gone). `hold.py` makes the placeholders of the root names a run cannot create and clears those a killed run left. `guard.py` reads that record for `make/common.mk`: the top-level host
`make` refuses while a path it names is unreviewed. `rust_version.py` reads the Rust version of the
image from `rust-toolchain.toml`. `isolation-check.sh` runs inside the
container for `make sandbox-check`. `audit-tools-check.sh` is `make sandbox-tools-check`: it fetches
the advisory database and checks a scratch crate with the image's cargo-deny. `sandbox_pr.py` is
`make sandbox-pr`: `source.py` makes the clone of a pull request or a ref with git isolated from
the host's configuration, `risk.py` classifies the paths the change touches and decides whether
it needs a confirmation (and whether the base image can run its tests), `deps.py` diffs the
dependencies and the sources it adds (Cargo and Python, uv sources included), `report.py` renders
`report.txt` and `report.json` and holds the progress and confirmation types, all without running
any of it, and the flow reuses `sandbox.py`'s plan and phases, adding the audit
containers (cargo-deny and pip-audit, with the host's `deny.toml`), the confirmations and the
reports. `malicious-pr.sh` is `make sandbox-pr-check`: it plants the probes of `tests/malicious/`
in a temporary clone and reviews it for real. `tests/sandbox.sh` runs the runner's tests against a fake
`docker` (`tests/fake_docker.py`), so they need no Docker; those that go through `make` run it
over a throwaway copy of the make files, never over this checkout. CONTRIBUTING.md (Sandbox) explains
how to use it.
