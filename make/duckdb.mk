# DuckDB extension (crates/oxedi_duckdb). Included by the root Makefile.
.PHONY: configure_ci set_duckdb_version set_duckdb_tag set_duckdb_repository debug release test_debug test_release duckdb-oracle duckdb-version-check

##@ DuckDB extension (names fixed by DuckDB CI)
# DuckDB's community CI runs set_duckdb_version, configure_ci, release (or debug) and
# test_release (or test_debug) from the repository root and uploads
# build/<type>/extension/oxedi/oxedi.duckdb_extension, so each target delegates to the crate's
# extension-ci-tools Makefile and the build copies its artifact to that path here.
# These names are fixed by that CI: release, debug, test_release and test_debug build and test the
# DuckDB extension only; Python releases go through dist, publish and tag.
configure_ci: ## DuckDB extension: create the test venv and record platform and version
	$(MAKE) -C $(EXT_DIR) configure_ci

set_duckdb_version: ## DuckDB extension: no-op for C API extensions (called by DuckDB's CI)
	$(MAKE) -C $(EXT_DIR) set_duckdb_version

set_duckdb_tag: ## DuckDB extension: no-op for C API extensions (called by DuckDB's CI)
	$(MAKE) -C $(EXT_DIR) set_duckdb_tag

set_duckdb_repository: ## DuckDB extension: no-op for C API extensions (called by DuckDB's CI)
	$(MAKE) -C $(EXT_DIR) set_duckdb_repository

release: ## DuckDB extension (not a Python release; see dist/publish/tag): build oxedi.duckdb_extension (release) into build/release/extension/oxedi
	$(MAKE) -C $(EXT_DIR) release
	@$(PYTHON_BIN) -c "import pathlib, shutil; d = pathlib.Path('build/release/extension/$(EXT_NAME)'); d.mkdir(parents=True, exist_ok=True); shutil.copyfile('$(EXT_DIR)/build/release/extension/$(EXT_NAME)/$(EXT_NAME).duckdb_extension', d / '$(EXT_NAME).duckdb_extension')"

debug: ## DuckDB extension: build oxedi.duckdb_extension (debug) into build/debug/extension/oxedi
	$(MAKE) -C $(EXT_DIR) debug
	@$(PYTHON_BIN) -c "import pathlib, shutil; d = pathlib.Path('build/debug/extension/$(EXT_NAME)'); d.mkdir(parents=True, exist_ok=True); shutil.copyfile('$(EXT_DIR)/build/debug/extension/$(EXT_NAME)/$(EXT_NAME).duckdb_extension', d / '$(EXT_NAME).duckdb_extension')"

test_release: ## DuckDB extension: run its SQLLogicTests against the release build
	$(MAKE) -C $(EXT_DIR) test_release

test_debug: ## DuckDB extension: run its SQLLogicTests against the debug build
	$(MAKE) -C $(EXT_DIR) test_debug

duckdb-version-check: ## fail unless the community descriptor's version equals the extension crate's version
	@d=$$(sed -n 's/^  version: *\(.*\)$$/\1/p' $(EXT_DIR)/description.yml | head -n 1); \
	test "$$d" = "$(DUCKDB_VERSION)" || { echo "duckdb-version-check: $(EXT_DIR)/description.yml has version $$d but $(EXT_DIR)/Cargo.toml has $(DUCKDB_VERSION)"; exit 1; }

duckdb-oracle: py-dev ## DuckDB extension: compare read_835 with oxedi.parse_file on every sample and fixture
	$(MAKE) -C $(EXT_DIR) test_oracle ORACLE_PYTHON=$(abspath $(PYTHON))
