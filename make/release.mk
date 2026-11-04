# Releases and publishing. Included by the root Makefile.
.PHONY: version release-check duckdb-release-check duckdb-tag publish-test publish-test-verify publish tag

##@ Releases
# Python releases use release-check and tag (tags v*, which start the PyPI workflow).
# The DuckDB extension uses duckdb-release-check and duckdb-tag (tags duckdb-v*, which do not).
version: ## print the PEP 440 version published by maturin
	@echo $(VERSION)

release-check: ## fail unless TAG is v<version>, the tree is clean and CHANGELOG.md has the version
	@test -n "$(TAG)" || { echo "usage: make release-check TAG=v<version>"; exit 1; }
	@test -n "$(VERSION)" || { echo "release-check: no version in Cargo.toml [workspace.package]"; exit 1; }
	@test "$(TAG)" = "v$(VERSION)" || { echo "release-check: tag $(TAG) differs from v$(VERSION) (Cargo.toml version $(CARGO_VERSION))"; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "release-check: the working tree is not clean"; git status --short; exit 1; }
	@grep -qE '^## \[$(subst .,\.,$(VERSION))\]' CHANGELOG.md || { echo "release-check: CHANGELOG.md has no '## [$(VERSION)]' section"; exit 1; }
	@echo "release-check: ok ($(TAG))"

duckdb-release-check: ## fail unless TAG is duckdb-v<extension version>, the tree is clean, HEAD is on origin/master, the descriptor matches and the extension CHANGELOG.md has the version
	@test -n "$(TAG)" || { echo "usage: make duckdb-release-check TAG=duckdb-v<version>"; exit 1; }
	@test -n "$(DUCKDB_VERSION)" || { echo "duckdb-release-check: no version in $(EXT_DIR)/Cargo.toml"; exit 1; }
	@test "$(TAG)" = "duckdb-v$(DUCKDB_VERSION)" || { echo "duckdb-release-check: tag $(TAG) differs from duckdb-v$(DUCKDB_VERSION) ($(EXT_DIR)/Cargo.toml version $(DUCKDB_VERSION))"; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "duckdb-release-check: the working tree is not clean"; git status --short; exit 1; }
	@git merge-base --is-ancestor HEAD origin/master || { echo "duckdb-release-check: HEAD is not on origin/master (run git fetch first; the check uses the local origin/master)"; exit 1; }
	@$(MAKE) --no-print-directory duckdb-version-check
	@grep -qE '^## \[$(subst .,\.,$(DUCKDB_VERSION))\]' $(EXT_DIR)/CHANGELOG.md || { echo "duckdb-release-check: $(EXT_DIR)/CHANGELOG.md has no '## [$(DUCKDB_VERSION)]' section"; exit 1; }
	@echo "duckdb-release-check: ok ($(TAG))"

# ---- Publishing (credentials come from ~/.pypirc; never from the repo) ----
publish-test: sdist-check ## upload the built artifacts to TestPyPI
	$(MATURIN) upload -r testpypi $(WHEELS)/oxedi-$(VERSION)*

publish-test-verify: ## install the TestPyPI pre-release into .venv and import it
	uv pip install --python $(PYTHON) --index-url https://test.pypi.org/simple/ --pre --no-deps --reinstall oxedi==$(VERSION)
	$(PYTHON) -c "import oxedi, importlib.metadata as m; print(m.version('oxedi'), oxedi.Spec.builtin())"

publish: sdist-check ## upload the built artifacts to PyPI (irreversible)
	@test -n "$(VERSION)" || (echo "no version in Cargo.toml" && exit 1)
	@echo "about to publish oxedi $(VERSION) to PyPI"; read -p "type the version to confirm: " v && test "$$v" = "$(VERSION)"
	$(MATURIN) upload -r pypi $(WHEELS)/oxedi-$(VERSION)*

tag: ## tag the current commit as v<version> and push the tag
	git tag -a v$(VERSION) -m "oxedi $(VERSION)"
	git push origin v$(VERSION)

duckdb-tag: ## tag the current commit as duckdb-v<extension version> and push the tag (does not start the PyPI workflow)
	git tag -a duckdb-v$(DUCKDB_VERSION) -m "oxedi DuckDB extension $(DUCKDB_VERSION)"
	git push origin duckdb-v$(DUCKDB_VERSION)
