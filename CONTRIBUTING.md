# Contributing

Thank you for helping. This page says how to start, how to test fast, the rules every change
keeps, and how to send it. [ARCHITECTURE.md](ARCHITECTURE.md) explains how the code is laid out.

## Start here

1. Read [ARCHITECTURE.md](ARCHITECTURE.md): the pipelines, where to change what, and the
   glossary of internal names.
2. Install the tools (see [Setup](#setup)), then run `make venv && make gates`. When it passes,
   your machine builds the Rust workspace and passes the commit gates. The Python, SQL and
   viewer checks are separate targets (`make py-test`, `make test_release`, `make viewer-test`).
3. While you work, run only the tests you need: see [the fast loop](#the-fast-loop).

## The fast loop

`make gates` runs every check. While you work, run only the part you touch:

| What | Command | Warm time |
|---|---|---|
| One core module | `cargo test -p oxedi_core --lib engine::` | under 1 s |
| One integration suite | `cargo test -p oxedi_core --test project_golden` | 2 s |
| One test by name | `cargo test -p oxedi_core --test write_findings a_value_holding_a_delimiter` | 1 s |
| The DuckDB crate (Rust) | `cargo test -p oxedi_duckdb` | 5 s |
| One Python test | `make py-dev && .venv/bin/pytest crates/oxedi_py/tests/test_spec.py -k version` | 5 s |
| The SQL tests | `make release && make test_release` | 40 s |
| The viewer's unit checks | `make viewer-units` | under 1 s |
| Before pushing | `make gates`, plus `make py-test` when `crates/oxedi_py` changed | a few minutes |

Times are for a machine that has built once. Some notes:

- `--lib <path>` filters the core's unit tests by module path (`engine::`, `project::`,
  `write::`); `--test <name>` runs one file of `crates/oxedi_core/tests/`. The suites are listed
  in [crates/oxedi_core/tests/README.md](crates/oxedi_core/tests/README.md).
- The first `cargo test -p …` after a workspace build compiles the crate again, once (about a
  minute), because a single package builds with fewer features than the workspace.
- `crates/oxedi_py` has no Rust tests (`test = false`): its tests are pytest, and `make py-dev`
  builds the extension first. `make py-dev` always installs into the checkout's own `.venv`,
  whatever virtual environment is active in your shell; set `OXEDI_VENV=/path` to use another (a relative path is taken from the repository root).
- The SQL tests need the DuckDB tools once: `git submodule update --init && make configure_ci`.
  See [DuckDB extension](#duckdb-extension).

## Code conventions

Each rule comes with an example from the code.

**Lossless.** Every input byte lands in exactly one segment's `raw`. Input the code does not
understand is data, an event or a diagnostic, never dropped. A segment no loop holds stays in the
document, as `Event::Unmatched` and a `Rule::UnknownSegment` diagnostic. From
`crates/oxedi_core/src/tokenizer/tests.rs`:

```rust
assert_eq!(segments[1].raw, b"\r\nSE*2*1~"); // the line break before SE is kept
assert_eq!(concat_raw(&segments), input);    // every raw, joined, is the input
```

**No panics on input.** No `unwrap`, `expect`, `panic!` or indexing that can fail on input
data in `src/`. Return an error, report a diagnostic, or skip with a reason. Indexing with ids
the code itself created (`LoopId`, `NodeId`) is fine. The only exception is the built-in specs
(`Spec::builtin_835`, `Spec::builtin_835_4010`), which `expect` because a test proves they
load. From `crates/oxedi_core/src/write/writer.rs`:

```rust
let Some(table) = data.get(reason.table) else { continue; };
```

**Null safety first.** A default is a typed value and an absence is a type; no `None`/null
sentinel or untyped value in an API, and conversions are checked. A reader then sees the
default in the signature, and a value that does not fit is refused with the reason instead of
truncated, wrapped or replaced. When brevity competes with this rule, the rule wins. From
`crates/oxedi_py/python/oxedi/_core.pyi`:

```python
def builtin(version: Version = Version.V5010) -> Spec: ...
def all() -> builtins.list[Version]: ...
```

**Errors explain themselves.** Every error and diagnostic names the rule that failed, where it
failed and the offending value. "Where" is the loop and key as written for a spec, and the
segment index, byte range, element and component position and loop path for a file. Each
variant has one test of its full `Display` text. `Display` reads well on its own and prints any
inner error in full. `source()` returns the inner error only when `Display` does not print it,
and one test per error type pins which (for example `no_spec_error_chains_its_source` in
`crates/oxedi_core/src/spec/tests/display.rs`). From
`crates/oxedi_core/src/diagnostic/tests.rs`:

```text
SNIP 1 · segment "XX" is not part of the structure: no open loop holds it and it opens no loop · segment #7 · at interchange#1/group#1/transaction#1/1000A#1 · datum "XX"
```

**The structure is data.** No code in the core is specific to the 835: the spec JSON holds
the structure. The Rust bindings (`crates/oxedi_py/src`, `crates/oxedi_duckdb/src`,
`crates/oxedi_wasm/src`) and the viewer never name an 835 segment; they ask the core. From
`crates/oxedi_wasm/src/view/mod.rs`:

```rust
&Builtin::select(document.segments()).spec
```

**The core stays sans-IO.** `oxedi_core`'s `[dependencies]` are `serde`, `serde_json`, `hmac`
and `sha2`. Nothing that does I/O, uses threads or needs a runtime. Open an issue before adding
any dependency to the core. The Python binding holds the only `pyo3` and `arrow-*`
dependencies, and every binding delegates the work to the core.

**Comments describe the code, not its history.** Comments and doc comments say what the code
does and why. No ticket or planning references, no "added in", no internal codes. From
`crates/oxedi_core/src/process/mod.rs`:

```rust
// One segment buffer for the whole pass: each segment is parsed into the
// previous one's element and composite vectors.
```

**One folder per module.** A module of a crate is a folder `x/mod.rs`; its parts are files in
that folder, and its tests live in `x/tests.rs` or `x/tests/<topic>.rs`. Clippy's
`self_named_module_files` lint rejects `x.rs` next to `x/`. A folder's `mod.rs` re-exports and
lists what each file holds (see `crates/oxedi_core/src/write/mod.rs`). The Python binding's
`crates/oxedi_py/src/` is the exception: its modules are leaves tested from pytest, so each is
one file.

**Files stay small.** About 400 lines of code per file, tests excluded. A longer file needs a
reason it is one coherent piece.

**Positions say their base.** Element, component and loop ordinal are 1-based; a segment index
and a table row are 0-based. A new field says which in its doc comment. From
`crates/oxedi_core/src/diagnostic/mod.rs`:

```rust
/// 1-based element position inside the segment, when the finding is.
pub element: Option<usize>,
```

**One name per concept.** A binding type that mirrors a core type is `Py<Core>` in Python
(`PyDiagnostic`) and `<Core>View` in the viewer (`DiagnosticView`, `DelimitersView`). Do not
reuse a core type's name for a different shape. In prose, a "diagnostic" is a reading result and
a "finding" is a writing or external-validator result.

**A rename moves the map too.** A change that renames a public core item or moves a module
updates [ARCHITECTURE.md](ARCHITECTURE.md) and `crates/oxedi_core/tests/public_paths.rs` (which
names every public path) in the same commit.

**Every commit passes the gates.** CI runs the same commands:

```bash
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo bench --workspace --no-run --locked
```

`make gates` runs these four plus rustdoc with warnings as errors, `make readme-py-check` and
`make deps-check` (no development dependency in a published crate or in the wheel's
`Requires-Dist`; the allowed lists are in `tools/deps/deps.py`).

## Tests and test data

Test data comes in three kinds, all under `crates/oxedi_core/tests/`:

- **Fixtures** (`fixtures/`): small synthetic files. Never edited.
- **Samples** (`samples/`): real payer files, anonymized before they entered the repository.
  Never edited. The originals and the key that re-identifies them live outside the repository
  and are never committed.
- **Goldens** (`golden/`): generated expected output. Regenerate them, then read the diff:

  ```bash
  UPDATE_GOLDEN=1 cargo test -p oxedi_core --test engine_golden    # events per file
  UPDATE_GOLDEN=1 cargo test -p oxedi_core --test project_golden   # tables and diagnostics
  ```

A new fixture or sample goes in the lists of `crates/oxedi_core/tests/common/mod.rs`
(`FIXTURES`, `SAMPLES`). `tests/file_lists.rs` and the Python `test_file_lists.py` fail when a
file is missing from them. [crates/oxedi_core/tests/README.md](crates/oxedi_core/tests/README.md)
maps every suite to what it pins.

**Never attach a real 835 to an issue or a pull request.** It holds patient data (names, member
ids, claim numbers, service dates). Instead:

- share masked output: parse with `oxedi.Mask` (see
  [Sharing data without patient information](docs/python/without-patient-data.md)) and share
  the tables or the diagnostics. The mask hides only those: never share `result.document`,
  whose every `Segment` keeps the original bytes. In the viewer, the mask switch masks the
  tables, the diagnostics and the CSV export; its Text and Loops views still show the original
  file;
- or cut the file down to the few segments that show the problem and replace every name, id
  and date by hand.

## Pull requests

- Branch from `master`; keep one change per pull request.
- The body has two sections. **Intent**: what the change is for and why. **Verification**: how
  you checked it (the commands you ran, a golden diff you read). Do not list the diff; reviewers
  read it.
- Run `make gates` before pushing, and `make py-test` when `crates/oxedi_py` changed. CI runs
  only the areas your change touches; see [CI](#ci).
- A change users can see adds a line under `[Unreleased]` in `CHANGELOG.md` (or
  `crates/oxedi_duckdb/CHANGELOG.md` for the extension).
- The maintainer merges.

## Good first issues

Issues labelled `good first issue` have a **Pointers** section: where the code lives, which test
pins it and how to check it. Comment on the issue before starting, so two people do not take
the same one.

## Setup

Prerequisites: [rustup](https://rustup.rs), Python 3.11 or later, and
[uv](https://docs.astral.sh/uv/). `make venv` creates `.venv` with maturin and the test tools.

The Rust version lives in one place: the `channel` of `rust-toolchain.toml`, an exact `X.Y.Z`
with clippy and rustfmt. rustup installs it on the first `cargo` command in the checkout, and CI
and the sandbox image read it from that file, so clippy and fmt give the same result on your
machine, in CI and in the sandbox. Nothing else names the version, so a bump edits only that line
of `rust-toolchain.toml`, in a pull request of its own (see [Bumping Rust](#sandbox) in the
Sandbox section).

```bash
make help       # list every target, grouped by area (the targets live in make/*.mk)
make venv       # create .venv with the dev tools
make py-dev     # build the extension into .venv (debug)
make gates      # format, clippy, Rust tests, benches compile, docs
make py-test    # build and run the Python test suite
make dist       # build the sdist and the release wheel into target/wheels
make smoke      # install the built wheel in a clean venv and run the suite
make stubs      # regenerate the native module's type stub (CI fails on drift)
make stubtest   # check the stub against the built module and the public API
```

`make stubs` links the libpython of `.venv` (or of `python3` without it), so that Python must have a
shared libpython: uv's Pythons, distribution and Homebrew packages and framework builds do; a
static pyenv build does not.

## DuckDB extension

From the repository root (Rust, Python 3 and the `extension-ci-tools` submodule are needed):

```bash
git submodule update --init
make configure_ci          # test venv, platform and version (the crate's, as v<version>)
make release               # build/release/extension/oxedi/oxedi.duckdb_extension
make test_release          # SQLLogicTest files in crates/oxedi_duckdb/test/sql
make duckdb-oracle         # read_835 against oxedi.parse_file, row by row
```

Load a local build with `duckdb -unsigned` and
`LOAD 'build/release/extension/oxedi/oxedi.duckdb_extension';`. `description.yml` in
`crates/oxedi_duckdb` is the descriptor for the community repository.

## Browser viewer

The viewer (`viewer/`) is a static page that runs the core as WebAssembly (`crates/oxedi_wasm`,
an internal binding with no public API). It has no JavaScript build step: `make wasm` builds the
module and generates `viewer/pkg/` (git-ignored).

```bash
make wasm              # build the module into viewer/pkg
make wasm-size-check   # fail if the gzipped module is over 250 KB
make viewer-serve      # build, then serve at http://127.0.0.1:8001
make viewer-test       # unit checks (node), the oracle against oxedi.parse, the browser tests
```

Tools: the `wasm32-unknown-unknown` Rust target (`make wasm` adds it), the `wasm-bindgen` CLI at
exactly the version `make wasm-bindgen-version` prints (from `Cargo.lock`), `wasm-opt` from
binaryen (CI uses release 133), Node 24 for the oracle and unit checks, and Chromium for the
browser tests. `crates/oxedi_wasm/scripts/install_tools.sh` installs the two prebuilt tools with
pinned checksums; CI and the deploy use it. Bumping the crate's `wasm-bindgen` pin means bumping
the version and checksum in that script. The browser tests run in the docs environment
(`make docs-venv`); install the browser once with `.venv-docs/bin/playwright install chromium`.
To run the viewer checks without installing any of these tools, use the sandbox's viewer image:
`make sandbox CMD="make viewer-test" IMAGE=viewer` (see [Sandbox](#sandbox)).

The page's Content-Security-Policy forbids network connections, so it loads the module from
generated ES modules instead of fetching it. Do not add a request, a font or a script from
another origin.

## Documentation

`README.md` is for GitHub and links to the guides under `docs/` with relative links.
`crates/oxedi_py/README.md` is the PyPI page and has the same content with absolute
`https://github.com/javillegasna/oxedi/blob/master/...` links, because relative links break on
PyPI. Change the root `README.md` and regenerate the other with `make readme-py`; `make gates` and CI
run `make readme-py-check`, which fails when the two differ.

## Documentation site

The site is built with MkDocs Material from `mkdocs.yml` and `docs/`. Its tools live in
`docs/requirements.txt`, in their own environment (`.venv-docs`), never in the package.

```bash
make docs         # strict build into site/: a broken link or a page outside the navigation fails
make docs-serve   # preview at http://127.0.0.1:8000
make release      # build the DuckDB extension once; the SQL examples load it
make docs-test    # run every python and sql block of docs/**/*.md and README.md
```

The API reference under `docs/reference/api/` is generated from the installed package and its type stub;
write a symbol's documentation in its docstring, not in the page.

`make docs-test` runs the blocks of a page in order, in one namespace (Python) or one DuckDB
connection (SQL), inside a temporary directory that holds `remittance.835`, `remittance-4010.835`, `big.835`, `a.835`,
`b.835`, `remits/*.835` and `remittances/*.835` (copies of the files under
`crates/oxedi_core/tests/fixtures/`). A block that cannot run carries a comment on the line
right before its fence, with a reason; the run lists every skipped block:

    <!-- docs-test: skip needs the reader's own tables -->

Use it for blocks that need something the runner cannot provide (a network, an orchestrator,
dbt templating), and when the equivalent runs elsewhere on the page, say so in the reason. A
`python` or `sql` block without the marker must run.

To add a page, put it in `docs/` and in the `nav` of `mkdocs.yml`; the strict build rejects a
page outside the navigation.

### Deploy

`.github/workflows/docs.yml` publishes with `mike` to the `gh-pages` branch. A push to `master`
that changes what the site or the viewer is built from (the `paths` of the workflow) publishes
`dev` (titled "dev (unreleased)"). A final tag `vX.Y.Z` publishes version `X.Y` and moves the `latest` alias, which
the site root redirects to. Pre-release tags and `duckdb-v*` tags publish nothing. Pull requests
only build the site (the Docs job in `ci.yml`). To publish a release that was tagged before the workflow existed, run "Docs deploy" manually
from `master` with the `version` input (`X.Y`, for example `0.5`); it also moves `latest`.
The same workflow publishes the viewer next to the versions: a push to `master` puts it in
`viewer/dev/`, and a final tag (or the manual run) puts it in `viewer/`, which keeps `viewer/dev/`.
It then checks that `versions.json`, every version directory and the viewer are still on
`gh-pages`. Never commit to `gh-pages` by hand.

## CI

`ci.yml` runs six areas, each a reusable workflow (`_rust.yml`, `_python.yml`, `_duckdb.yml`,
`_docs.yml`, `_viewer.yml`, and `_lint.yml`, which lints the workflows and their scripts), only
when files that area depends on changed: a pull request is compared with its base, a push to
`master` with the previous commit. The `Changes` job holds the
path filters; a change under `.github/` runs Rust, and a change to the composite actions in
`.github/actions/` runs every area. A draft pull request runs Rust only; the other areas start
when it is marked ready for review. A new push to a pull request cancels its running checks.

`nightly.yml` runs every area on `master` each night, whatever changed, plus the DuckDB
pre-release leg (`next`), which reports and does not block. Run it by hand from the Actions tab
when a change needs the full set before merging.

**A pull request from a fork.** Its checks wait for the maintainer's approval before any of them
runs. No workflow keeps a token on disk: every checkout sets `persist-credentials: false`, and
the docs deploy hands its token to `git push` alone. `.github/scripts/lint.sh` runs
[zizmor](https://docs.zizmor.sh) over the workflows and composite actions, so an unpinned
action, a template injection or a stored credential fails the Workflows area. When a finding
cannot be fixed without changing what the job does, ignore it on the line it names, with the
reason after the rule: `# zizmor: ignore[github-env] why the step is safe`. The only rule
switched off for the whole repository is `self-repository`, in `.github/zizmor.yml`, with its
reason. A change to the
build, CI or dependency files (`Makefile`, `make/`, `.github/`, `tools/`, the `Cargo.toml`,
`Cargo.lock`, `pyproject.toml`, `build.rs` and `rust-toolchain.toml` files and
`docs/requirements.txt`) needs the code owner's review, as `.github/CODEOWNERS` says. Dependabot
refreshes the pinned actions every week and proposes a release only after it has been out for
seven days (a cooldown).

`CI result` is the one check to require: it fails when an area failed or was cancelled and
passes when each one passed or was not needed. Caches are written only from `master`;
pull requests read them. The checks of a push to `master` are never cancelled or replaced by
a later push.

## Profiling

To find where time goes before changing anything, sample it. Unix only; nothing to install beyond
`cargo` and `uv` (`make venv` brings `py-spy`).

```bash
make profile BENCH=process               # a core bench: tokenize, engine, check, process or write
make profile BENCH=engine SECONDS=20     # longer sampling (default 10 seconds per benchmark)
make profile-py SCRIPT=crates/oxedi_py/scripts/profile_parse.py ARGS='FILE 300'
```

- `make profile` runs `cargo bench -p oxedi_core --bench BENCH -- --profile-time SECONDS`. For each
  benchmark it writes `flamegraph.svg` (open it in a browser) and `top.txt` (the 25 functions
  with the most samples of their own, with their share) into
  `target/criterion/<group>/<benchmark>/profile/`, and lists the files this run wrote. Without
  `--profile-time`, the benches run as always and nothing is sampled.
- `make profile-py` builds the extension in release mode into `.venv`, then runs the script
  twice under `py-spy`: `target/profile-py/flamegraph.svg` and `target/profile-py/stacks.txt`
  (one collapsed stack and its sample count per line). It sees Python frames; the time inside
  the Rust extension shows as the call into it, so profile the Rust side with `make profile`. The target fails when the script exits non-zero.
- The sampler is `pprof`, a development dependency of `oxedi_core` that only the benches use
  (`crates/oxedi_core/benches/common/profiler.rs`); `py-spy` is in the `profile` dependency group
  of `crates/oxedi_py/pyproject.toml`. Neither reaches a published artifact: `make deps-check`
  proves it.

## Sandbox

`make sandbox` builds and tests this checkout inside a throwaway Docker container, so a build
script, a test or a dependency cannot read your secrets, reach the network or change what you run
afterwards. Use it for any command you would rather not run on your machine directly.

```bash
make sandbox CMD="make gates"     # any command; it runs with bash -c inside the container
make sandbox CMD="make py-test"
make sandbox CMD="make viewer-test" IMAGE=viewer   # the viewer checks, in the viewer image (amd64 only)
make sandbox-check                # prove the isolation from inside: every line says ok (IMAGE=viewer too)
make sandbox-tools-check          # prove cargo-deny and pip-audit work in the image (needs the network)
make sandbox-pr PR=251            # review a pull request: see "Reviewing a pull request" below
make sandbox-pr-check             # prove sandbox-pr keeps a hostile change from your machine
make sandbox-image                # build the image (make sandbox builds it when it is missing)
make sandbox-image-viewer         # build the viewer image on it (IMAGE=viewer builds it when it is missing)
make sandbox-clean                # remove this checkout's storage; ALL=1 removes all of it
```

`CMD` reaches the container's `bash -c` as written, with no other quoting layer; `$$` is a
literal `$`. `CMD` for `sandbox`, and `PR`, `REF`, `HEAD`, `TARGET` and `CONFIRM` for `sandbox-pr`,
are read from the command line only, like `IMAGE`: one left in your shell is refused with exit 2
and one line before any docker command, for example `sandbox-pr: CONFIRM is read from the command
line only; pass it as make sandbox-pr CONFIRM=yes`. GNU make takes a variable assigned in an
exported `MAKEFLAGS` or `GNUMAKEFLAGS` as one from the command line, so the top-level make refuses
those too, `IMAGE` included, naming the variable and saying to unset the one that holds it. On Linux
the check reads the environment make started with, from `/proc`; on another host with GNU make 4.4
or later, an assignment in an exported `MAKEFLAGS` cannot be told apart from the command line, so
there only the origin check holds.

**What the container is.** A pinned image (`tools/sandbox/Dockerfile`: Ubuntu 24.04, the Rust
version `rust-toolchain.toml` pins, CPython 3.11 and 3.13, uv, cargo-deny and pip-audit;
no `git`), tagged with the hash of its Dockerfile, `pip-audit.txt` and `rust-toolchain.toml`. The container runs as you, with a read-only root, no capabilities, no new
privileges, the default seccomp profile and limits on processes, memory and CPUs. It gets a fixed
environment, never a variable of your shell, and no part of your home other than its own storage:
no `~/.ssh`, `~/.pypirc`, `~/.config/gh`, Docker socket or the private samples.

`IMAGE=viewer` runs in a second image, `tools/sandbox/Dockerfile.viewer`, built `FROM` the base
image's exact tag, which it leaves unchanged: Node 24 by version and SHA-256, Playwright's
`chromium-headless-shell` at the revision the `playwright==` pin of `docs/requirements.txt`
installs (checked by the SHA-256 of the installed tree), and the `wasm32-unknown-unknown` target,
`wasm-bindgen` and `wasm-opt` at the versions and SHA-256s of
`crates/oxedi_wasm/scripts/install_tools.sh`, which `make sandbox-image-viewer` reads on the host
(the image never runs that script). It is tagged `oxedi-sandbox-viewer:<hash>` of the base tag,
`Dockerfile.viewer`, `tools/sandbox/playwright.txt`, `install_tools.sh` and
`tools/sandbox/viewer_image.py`, and it is amd64 only, because the wasm tools are pinned for
x86_64: on another architecture `make sandbox-image-viewer` refuses with one line naming it, and
with no daemon to ask it says the daemon could not be reached.
`IMAGE` is `base` (the default) or `viewer` and is read from the command line only: any other
value, or an `IMAGE` exported in your shell, is refused with one line before any docker command.
A checkout whose `playwright==` pin, or whose `wasm-bindgen` or `binaryen` pin in `install_tools.sh`,
differs from the one the viewer image was built with is refused before any container, with a line
naming the tool and both versions and saying to run `make sandbox-image-viewer`.

Chromium runs without its own sandbox in the viewer image: its sandbox needs user namespaces, which
a container with no capabilities, no new privileges and the default seccomp profile does not grant,
and the container is already the boundary. The image sets `OXEDI_CHROMIUM_NO_SANDBOX=1`, and only
that variable makes the browser tests start Chromium with `--no-sandbox`; neither the base image
nor the host checks set it.

**Two phases.** Each run starts both, each in its own container:

- *fetch*, with the network: the checkout read-only, the repository's `.cargo` masked and no
  project code run. It runs `cargo fetch --locked` and installs wheels only (never an sdist)
  into a fresh `.venv` from the `contributor` group of `crates/oxedi_py/pyproject.toml`. With
  `IMAGE=viewer` it also makes a fresh `.venv-docs` from `docs/requirements.txt`, wheels only and
  with `--require-hashes`, and the stamp `make docs-venv` checks. Without the network it stops
  with one line; it never falls back to the caches offline.
- *run*, with no network: your command, with the cargo registry read-only.

**What stays read-only.** The files your host executes or trusts after a run are bound read-only
over the checkout in both phases: `Makefile`, `make/`, `tools/`, `.cargo/`, `.github/`,
`rust-toolchain.toml`, `Cargo.toml`, `Cargo.lock`, `CLAUDE.md`, `.claude/`,
`crates/oxedi_py/pyproject.toml`, `.git`, `.gitmodules`, every submodule (the DuckDB targets
include `extension-ci-tools`' makefiles), and every tracked `Cargo.toml`, `Cargo.lock`,
`rust-toolchain`, `pyproject.toml`, `uv.toml`, `.python-version` and `.cargo/` anywhere, with every
tracked file a host tool loads by its name alone: `conftest.py`, `pytest.ini`, `setup.cfg`,
`tox.ini`, `noxfile.py`, `sitecustomize.py`, `usercustomize.py` and `.envrc`. The names make and
pytest load from the root, `GNUmakefile`, `makefile` and `conftest.py`, are read-only when present;
when absent, the run phase binds an empty read-only file over each, so the run cannot create one.
While the run lasts, the checkout holds a placeholder of each name, one line that says what it is,
removed when the run ends. A runner killed before then (SIGKILL, an out-of-memory kill, a power
loss) leaves them: a host `make` then stops with the placeholder's own line,
`GNUmakefile:1: *** sandbox: a killed sandbox run left this placeholder GNUmakefile; ...`. Remove
each root `GNUmakefile`, `makefile` or `conftest.py` that holds only such a line; or start the
runner directly (`python3 -I tools/sandbox/sandbox.py`, below), which removes the placeholders
that still hold their line, says so, and refuses when one was changed. Ctrl-C, SIGTERM and SIGHUP
(a closed terminal) stop the runner cleanly and leave none. Every path git ignores is hidden: a directory behind an empty read-only mount, a file behind an empty
read-only file, so a run cannot read a local `.env`. The sandbox's own outputs (`target/`,
`.venv/`, Python caches, the built `_core*.so`) are the exception.

**The audit.** Before the run the sandbox records what git sees and a fingerprint of every
tracked and untracked file; after it, it prints every file that changed while the sandbox ran
(the audit cannot tell whether the run or you wrote it), every new path the run left (and where a
new symbolic link out of the checkout points) and every path it cannot read, and removes the
Python caches and the built extension from the checkout. It exits 3 when a file changed, a new
path appeared outside those outputs, a path could not be read or an output could not be removed,
whatever the command's status: read `git status` and `git diff` before running anything from the
checkout. Otherwise it exits with the command's status (124 past the timeout, 130 on Ctrl-C, 143
on SIGTERM, 129 on SIGHUP, 2 when it refuses to start); Ctrl-C, SIGTERM and SIGHUP kill the container and run the audit
first.

A new path that host tools look up by name comes first in the report, with a line that says not
to run `make` or any tool in the checkout until you remove it: `GNUmakefile` and `makefile` (make
reads them before `Makefile`, so `make sandbox` itself would run them), a `*.py` at the root,
`sitecustomize.py`, `usercustomize.py`, `conftest.py`, `.envrc`, `.cargo/` and `rust-toolchain*`.
Every Python that `make/sandbox.mk` starts runs isolated (`python3 -I`), so a module left in the
checkout never loads. A dirty audit is recorded in the checkout's storage, where no run can
write, and the next run refuses to start (exit 2, naming the paths) while one of them is still as
the run left it: remove or revert it, or add it with `git add` once you have reviewed it.
`make sandbox-clean` forgets the record with the rest of the checkout's storage.

**The guard.** While that record names a path still as the run left it, every `make` in the
checkout refuses too, before any recipe runs, so a `make py-test` never loads a `conftest.py` the
run planted:

```text
make/common.mk:NN: *** make: a sandbox run left crates/oxedi_py/tests/x/conftest.py unreviewed; remove or revert them, or git add them once reviewed (make sandbox-clean forgets the record).  Stop.
```

Clear it the same way: remove or revert each path, or `git add` it once reviewed; or run
`make sandbox-clean`, which forgets the record. `make help` and `make sandbox-clean` still run
while the guard refuses. The guard (`tools/sandbox/guard.py`) runs once, in the top-level make;
with no record it checks that one file is absent and calls no git.

Through `make`, the status shows in make's own line (`make: *** [make/sandbox.mk:...] Error 3`) and
make exits 2. A script that tests the status calls the runner directly:
`SANDBOX_CMD="make gates" python3 -I tools/sandbox/sandbox.py --image oxedi-sandbox:<hash> --checkout .`
returns it as is (`make sandbox` prints the image tag in its first line).

**Knobs.** `SANDBOX_CPUS` (default: the daemon's CPUs minus one), `SANDBOX_MEMORY` (GB; default
60 % of the daemon's memory) and `SANDBOX_TIMEOUT` (seconds; default 3600), for example
`make sandbox CMD="make gates" SANDBOX_MEMORY=8`. A run killed at the memory limit exits 137 and
says how to raise it.

**Storage.** `${XDG_CACHE_HOME:-~/.cache}/oxedi-sandbox/` holds the cargo registry and the uv
cache, shared by every checkout, and one directory per checkout with its `target/`, `.venv` and
(for `IMAGE=viewer` runs) `.venv-docs` and `viewer/pkg`, so the sandbox never touches the ones you
build on the host.
`make sandbox-clean` removes this checkout's directory; `make sandbox-clean ALL=1` removes all of
`oxedi-sandbox/`. Both refuse while a sandbox container that uses the storage runs.

**Hosts.** A rootful Docker Engine on Linux, and Docker Desktop on Linux and macOS. A rootless
daemon is refused. The base image builds for amd64 and arm64, but the arm64 leg is untested; the
viewer image is amd64 only. When the
daemon lists the gVisor runtime (`runsc`), the sandbox uses it; otherwise `runc`. The line
printed before each run names the runtime, the image and its Rust version, the limits, the
storage and the read-only, masked and hidden paths.

**What stays on the host.** Targets that need `git`, the network or tools the image does not
hold: `make complexity`, `make py-test-nopyx12`, `make release-check`, the publishing targets,
and the DuckDB and docs targets. The viewer targets (`make wasm`, `make viewer-test` and the rest)
run in the sandbox with `IMAGE=viewer`.

**Reviewing a pull request.** `make sandbox-pr` tests someone else's change in the sandbox
without running any of it on your machine:

```bash
make sandbox-pr PR=251                         # the merge ref refs/pull/251/merge, as CI tests it
make sandbox-pr PR=251 HEAD=1                  # its head instead, when it has no merge ref
make sandbox-pr REF=feature                    # a branch or any ref of this checkout, as it is
make sandbox-pr PR=251 TARGET="make py-test"   # another command (default: make gates && make py-test)
make sandbox-pr PR=251 CONFIRM=yes             # go on without asking when something is flagged
make sandbox-pr PR=251 IMAGE=viewer TARGET="make viewer-test"   # in the viewer image
```

Those variables are read from the command line only, as the `sandbox` section says. It clones
this checkout into a throwaway directory and fetches the pull request anonymously over HTTPS from
the GitHub repository your `origin` names. With `PR`, it tests the merge ref; when
there is none (the pull request has conflicts or is closed), it refuses and says to set `HEAD=1`.
Every `git` it runs on the clone runs without your global and system git configuration, hooks,
attributes file or file transport, so a filter or hook the change names never runs on your
machine. It needs git 2.40 or later (for `--attr-source`), and refuses with the version it found.

Then, in order:

1. A risk report made from the diff alone (base...head; the base is the merge commit's first
   parent for `PR`, the merge base with `origin/master`, else `master`, otherwise): every path it
   touches, flagged when it is a build script, a manifest or lock file, a file the host executes,
   a script, a test hook such as `conftest.py`, a procedural macro, documentation `make docs-test`
   runs, a binary, a symbolic link, a submodule or a `.gitattributes`; and the packages it adds,
   changes or removes in `Cargo.lock` and in the declared Python dependencies. The Python
   diff covers `pyproject.toml` dependencies and dependency groups, `requirements*.txt`, and the
   uv sources and package indexes (`[tool.uv.sources]`, `index-url` and the like), so a
   dependency moved to a git URL or another index shows as changed.
2. A confirmation before `cargo-deny`, which resolves the change's manifests and so can reach a
   source the change adds: asked when the change adds a git or registry source (a `git` or
   `registry-index` URL, a `[source.*]` registry, or a `[registries.*]` index that a dependency's
   `registry` name leads to) to a `Cargo.toml`, a `Cargo.lock` or a `.cargo/config(.toml)` (each
   URL is listed first), and when a Cargo file it touches cannot be read (not UTF-8 or not
   TOML), because its sources cannot be checked. Same `y/N`, exit 4 and `CONFIRM=yes` as below.
3. `cargo-deny` (advisories, sources, bans) over the change's `Cargo.lock`, in a container with
   the network and the checkout read-only.
4. A confirmation when something is flagged or cargo-deny failed: `y/N` on a terminal; without
   one it stops with exit 4 unless `CONFIRM=yes` is set.
5. The fetch, then `pip-audit` over the wheels the fetch installed in each venv (`.venv`, and
   `.venv-docs` with `IMAGE=viewer`), and a second confirmation, asked the same way, when one of
   them failed.
6. `TARGET` in the run phase, with no network, and the audit of the clone.

Before any container, a change whose `crates/oxedi_py/pyproject.toml` has no `contributor`
dependency group (a change not rebased on `master`) is refused with
`sandbox-pr: the change's crates/oxedi_py/pyproject.toml has no "contributor" dependency group,
which the fetch installs; rebase the change on master`, instead of failing in the fetch.

The rules come from your checkout, never from the change: the scripts in `tools/sandbox/`, the
list of flagged paths and the cargo-deny policy (`tools/sandbox/deny.toml`, mounted read-only into
the audit container). A change that edits them is judged by your copies, and the report flags it.

The clone and both reports, `report.txt` and `report.json`, are in
`${XDG_CACHE_HOME:-~/.cache}/oxedi-sandbox/prs/<id>-<sha12>/` (`<id>` is `pr<N>` or
`ref-<name>`, `<sha12>` the tested commit). A run of the same change and commit removes that
directory and the clone's `target/` first, so no build output survives from one run to the next.
`make sandbox-clean ALL=1` removes them with the rest of the storage. Nothing is published: no
comment, label, push or status reaches GitHub. Reviewed with the base image, a change that touches
`viewer/` or `crates/oxedi_wasm/` gets a last line in `report.txt` (and `image_hint` in
`report.json`) saying to rerun with `IMAGE=viewer`, since the base image cannot run its tests.
`report.json` holds the change, the touched paths with their kinds, the `cargo` and `python`
package diffs, `new_sources` (the sources the change adds) and `unreadable_cargo` (the Cargo files
it touches that could not be read), both lists, then `cargo_deny`, `pip_audit`, `flagged`,
`confirmed`, `run`, `image`, `runtime` and `image_hint`. `cargo_deny` and
`run` are `null` when that step did not run (a refusal before it), and `pip_audit` is `[]`.

The exit status is that of `make sandbox` (the command's, 2 refused, 3 dirty audit, 124 timeout,
129, 130 and 143 on a signal), the fetch's when the fetch failed, and 4 when a confirmation was needed
and not given. Every refusal is one line starting `sandbox-pr:`.

`make sandbox-pr-check` proves it with a hostile change: in a temporary clone of your checkout, on
a local branch that is never pushed, it plants a build script and a test hook that try to list your
home directory, reach the network and write to the cargo cache and to a canary file, recording only
whether each try worked. It reviews that branch with `CONFIRM=yes`, checks that every try was
refused, that the canary, your home and your repository's refs are unchanged, and removes the
clone. One line per check says ok or FAIL.

**Bumping Rust.** `rust-toolchain.toml` is the one place the version is written (an exact
`X.Y.Z`); CI and the image read it from there, and nothing else names it. A bump edits its
`channel` only, in a pull request of its own: run `make sandbox-image` (the file is an input of
the tag, so the version is a new tag), then both gates, `make gates` on the host and
`make sandbox CMD="make gates"`, and fix what the new clippy or rustfmt reports in that same pull
request.

**Bumping the viewer tools.** Each pin lives in one place, and a bump is a new viewer tag:

- Playwright: change `playwright==` in `docs/requirements.in`, regenerate `docs/requirements.txt`
  with the command in its header (it keeps the hashes), and regenerate `tools/sandbox/playwright.txt`
  at the same version as its header says; `make sandbox-image-viewer` refuses when the two differ.
  The new Chromium revision fails the first build with the digest of its installed tree (each
  file's SHA-256 and mode, and each symbolic link's target): check it, then set
  `CHROMIUM_TREE_SHA256_AMD64` in `tools/sandbox/Dockerfile.viewer`. Until the image
  is rebuilt, `IMAGE=viewer` refuses a checkout with the new pin.
- `wasm-bindgen` and `wasm-opt`: change the versions and checksums in
  `crates/oxedi_wasm/scripts/install_tools.sh`; the image reads them from there.
- Node: `NODE_VERSION` and `NODE_SHA256` in `tools/sandbox/Dockerfile.viewer`.

**Bumping the audit tools.** A cargo-deny or pip-audit bump must pass `make sandbox-tools-check`.
cargo-deny 0.19.1 and later need the `git` command to fetch the advisory database, which the image
does not carry, so the image keeps 0.19.0 until that changes.

## Releases

`make release-check TAG=v<version>` checks the tag, a clean tree and the changelog entry before
a release. The Python package and the DuckDB extension are versioned and tagged separately
(`v*` and `duckdb-v*`); see `make help` for the publishing targets.

## Complexity

**Targets.** Per function: cyclomatic complexity (CCN) of 6–7, nesting no deeper than 3, at
most 8 branches. Flat per-variant dispatch (a big `Display` or kind `match`) is the expected
exception. No pinned tool counts branches apart from CCN, so the 8-branch limit is checked in
review.

**The ratchet.** Today's code is the baseline: nothing may get worse, and a function over a
target may only shrink. Two checks hold that line:

- Clippy (`make clippy`) caps cognitive complexity, nesting, function length and argument count
  at today's largest values, set in `clippy.toml`. Lower a ceiling when a refactor brings the
  largest function down. A Rust toolchain bump may need them measured again.
- `make complexity` (needs uv) measures CCN and nesting of every Rust, Python and JS function
  outside tests, benches, examples and tooling, with a pinned lizard (Python nesting with the
  `ast` module). It compares the functions over a target (CCN above 7, nesting above 3) with
  `.github/complexity-baseline.txt`. CI runs it in the Workflows area (see [CI](#ci)). The
  tool is in `tools/complexity/` (see [tools/README.md](tools/README.md)); its tests run with
  `.github/scripts/lint.sh`.

**What it reports.** Every line says how to resolve it:

- `FAIL`: a function is over a target and not in the baseline, or worse than its entry.
  Simplify it.
- `STALE`: an entry is better than recorded, or its function is within the targets or gone.
  Run `make complexity-update` and commit the baseline with the refactor, so the baseline
  shrinks.
- `TOUCHED`: this change modifies a function that is in the baseline and leaves it over a
  target. See [Touch it, fix it](#touch-it-fix-it).
- `MOVED`: a file was moved or renamed, and a function in it matches its old entry, no worse.
  Run `make complexity-update` to record the new path.

**Touch it, fix it.** A change that modifies a function listed in the baseline must leave it
within the targets (CCN 7 or less, nesting 3 or less). The check diffs your change against the
base (`origin/master` locally, the base of the push or pull request in CI; set `COMPLEXITY_BASE`
to use another ref) and reports a `TOUCHED` line for each such function that is still over a
target. A function is modified when a changed line, committed or not, falls inside it, so
moving or re-indenting a function inside its file counts, and so does editing one in a file you
rename. Moving a file without editing its functions modifies nothing. To fix it, find the cause
and remove it with the fitting pattern or data structure: a table instead of a chain of `if`s,
an enum with a method instead of repeated `match`es, a parser that reads one step per function. Splitting the function into pieces that
call each other only moves the complexity, so it does not count. Flat per-variant dispatch is
the one exception: record it with `make complexity-update ACCEPT='path::function'`, which marks
its baseline line `flat`, and explain it in the pull request's **Intent**. When the check
cannot find the base (no `origin/master`, a shallow clone), it prints one line saying it skipped
the touched check and does not fail.

**Reviewed exceptions.** Adding a variant to a flat dispatch, such as one more arm in an
error's `Display`, raises its CCN by one and is acceptable. Record it with
`make complexity-update ACCEPT='path::function'` (several separated by spaces) and say why in
the pull request's **Intent**. `ACCEPT` also marks the entry `flat`, which exempts it from the
touch rule from then on, so use it only for flat per-variant dispatch (an error's `Display`, a
kind `match`). Nothing else is accepted this way: fix the cause instead.
On a pull request, CI lists every added or raised entry, and every entry newly marked `flat`, in
the job summary, and changes to the baseline or `clippy.toml` need the maintainer's review
(`.github/CODEOWNERS`).
