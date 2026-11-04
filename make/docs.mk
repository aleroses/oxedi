# Documentation site (mkdocs.yml, docs/, docs/tests/). Included by the root Makefile.
.PHONY: docs-venv docs-dev docs docs-serve docs-test

##@ Docs site
# The docs environment holds the site tools from docs/requirements.txt and the package itself
# (debug build, editable), so the API reference and the examples run against the current code.
$(DOCS_VENV)/.ready: docs/requirements.txt
	uv venv --clear $(DOCS_VENV) --python 3.13
	uv pip install --python $(DOCS_PYTHON) --require-hashes -r docs/requirements.txt
	touch $@

docs-venv: $(DOCS_VENV)/.ready ## create .venv-docs with the site tools (locked in docs/requirements.txt)

docs-dev: docs-venv ## build the package into .venv-docs (debug), so the site and examples see current code
	VIRTUAL_ENV=$(abspath $(DOCS_VENV)) $(DOCS_VENV)/bin/maturin develop --uv --manifest-path $(PY_MANIFEST)

docs: docs-dev ## build the site into site/ (strict: any warning fails)
	NO_MKDOCS_2_WARNING=true $(DOCS_VENV)/bin/mkdocs build --strict

docs-serve: docs-dev ## preview the site at http://127.0.0.1:8000
	NO_MKDOCS_2_WARNING=true $(DOCS_VENV)/bin/mkdocs serve

docs-test: docs-dev ## run every python and sql block of the docs and README (SQL needs `make release` first)
	$(DOCS_PYTHON) -m pytest docs/tests -q
