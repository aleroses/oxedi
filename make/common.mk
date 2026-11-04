# Variables and functions shared by the area files. No target is defined here. Included first by
# the root Makefile.

# Paths and tools

PY_MANIFEST := crates/oxedi_py/Cargo.toml
PY_TESTS    := crates/oxedi_py/tests
# The test and profile tool groups live in the package's pyproject (never in its published metadata).
PY_PYPROJECT := crates/oxedi_py/pyproject.toml
# stubtest reads an allowlist only when one exists; each entry carries its reason.
STUBTEST_ALLOWLIST := crates/oxedi_py/stubtest-allowlist.txt
# The Python environment is always the checkout's own .venv, never the VIRTUAL_ENV of the shell
# that runs make: two checkouts or agents with different active environments must not share one.
# CI creates .venv in the workspace; set OXEDI_VENV to point make at another environment.
OXEDI_VENV  ?= $(CURDIR)/.venv
VENV        := $(abspath $(OXEDI_VENV))
# Same tools without pyx12, to run the suite as a user without it would.
NOPYX12_VENV := .venv-nopyx12
# The documentation site has its own environment; the package never depends on it.
DOCS_VENV   := .venv-docs
DOCS_PYTHON := $(DOCS_VENV)/bin/python
PYTHON      := $(VENV)/bin/python
# Prefer the venv's tools; fall back to whatever is on PATH (asdf shims, uv tool installs).
MATURIN     := $(if $(wildcard $(VENV)/bin/maturin),$(VENV)/bin/maturin,maturin)
PYTEST      := $(if $(wildcard $(VENV)/bin/pytest),$(VENV)/bin/pytest,$(PYTHON) -m pytest)
WHEELS      := target/wheels
# The interpreter that runs small scripts outside the venv (DuckDB CI and the viewer build).
PYTHON_BIN  ?= python3

# The DuckDB extension: its crate and the name it is built under.
EXT_DIR     := crates/oxedi_duckdb
EXT_NAME    := oxedi

# Versions

# The workspace version is Cargo's form (0.1.0-rc.1); maturin publishes it as PEP 440 (0.1.0rc1).
# Only the -a.N, -b.N, -rc.N and -dev.N pre-release forms are mapped; any other suffix
# (-alpha, -beta) leaves a version that release-check rejects against the tag.
CARGO_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
# The DuckDB extension has its own version, in its crate's Cargo.toml.
DUCKDB_VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' crates/oxedi_duckdb/Cargo.toml | head -n 1)
VERSION     := $(shell echo '$(CARGO_VERSION)' | sed -E 's/-(a|b|rc)\.?/\1/; s/-dev\.?/.dev/')

# Functions

# $(call check-generated,TARGET,GENERATOR,FILE,FIX): a recipe line that fails, printing a message
# that names TARGET, unless FILE is exactly what the shell command GENERATOR writes to stdout.
# FIX is the make target that regenerates FILE.
define check-generated
@$(2) \
  | diff -u $(3) - \
  || { echo "$(1): $(3) is stale; run make $(4)"; exit 1; }
endef

# The sandbox guard

# While the last sandbox run's leftovers are unreviewed (tools/sandbox/guard.py says which), the
# top-level make stops with the guard's line before any recipe runs, whatever the goal, except help
# and sandbox-clean, which inspect or forget the record. A sub-make was started by a make that
# passed the guard. A host without $(PYTHON_BIN) never ran the sandbox (its runner needs it), so it
# holds no record to check.
ifeq ($(filter-out 0,$(MAKELEVEL)),)
ifneq ($(filter-out help sandbox-clean,$(or $(MAKECMDGOALS),$(.DEFAULT_GOAL),help)),)
SANDBOX_GUARD := $(shell if command -v $(PYTHON_BIN) > /dev/null 2>&1; then $(PYTHON_BIN) -I '$(CURDIR)/tools/sandbox/guard.py' --checkout '$(CURDIR)' 2>&1; fi)
ifneq ($(SANDBOX_GUARD),)
$(error $(SANDBOX_GUARD))
endif
endif
endif
