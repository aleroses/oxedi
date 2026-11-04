# Developer entry points. Every target runs from the repository root.
# The targets live by area in make/*.mk; this file keeps the shared setup, `help` and `gates`.
# Rust gates mirror CI; Python targets use the checkout's own .venv (create it with `make venv`).

.DEFAULT_GOAL := help

# The order is fixed: common.mk holds the variables and functions the others use.
include make/common.mk
include make/rust.mk
include make/python.mk
include make/duckdb.mk
include make/docs.mk
include make/viewer.mk
include make/quality.mk
include make/sandbox.mk
include make/release.mk

.PHONY: help gates

##@ Overview
# Lists every target that has a `##` comment, grouped by the `##@` heading of its file.
help: ## list targets
	@awk 'BEGIN { FS = ":.*## " } \
	  /^##@ / { printf "\n%s\n", substr($$0, 5); next } \
	  /^[a-z][a-z0-9_-]*:.*##/ { printf "  %-24s %s\n", $$1, $$2 }' $(MAKEFILE_LIST)
	@echo
	@echo "Python releases use release-check and tag (tags v*, which start the PyPI workflow)."
	@echo "The DuckDB extension uses duckdb-release-check and duckdb-tag (tags duckdb-v*, which do not)."

gates: fmt-check clippy test bench-check doc readme-py-check deps-check ## run every commit gate
