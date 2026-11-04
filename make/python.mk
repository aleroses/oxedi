# Python binding: the venv, the extension build, type checks, the distribution files and the PyPI
# README. Python targets use the local .venv (create it with `make venv`). Included by the root
# Makefile.
.PHONY: venv py-dev py-test py-test-nopyx12 stubs stubtest compat-oracle dist smoke readme-py readme-py-check sdist-check wheel-check clean-dist

##@ Python package
# The sed program that turns README.md's relative links into absolute ones.
README_PY_RENDER := sed -E 's\#\]\(([^):\\\#][^):]*)\)\#](https://github.com/javillegasna/oxedi/blob/master/\1)\#g' README.md
PY_TOOLS := maturin --group $(PY_PYPROJECT):test
venv: ## create .venv with uv and the dev tools
	uv venv $(VENV) --python 3.13
	uv pip install --python $(PYTHON) --group $(PY_PYPROJECT):contributor

py-dev: ## build the extension into .venv (debug)
	VIRTUAL_ENV=$(VENV) $(MATURIN) develop --uv --manifest-path $(PY_MANIFEST)

py-test: py-dev ## build and run the Python suite
	$(PYTEST) -q $(PY_TESTS)

py-test-nopyx12: ## build into a venv without pyx12 and run the Python suite (pyx12 tests skip)
	uv venv --clear $(NOPYX12_VENV) --python 3.13
	uv pip install --python $(NOPYX12_VENV)/bin/python $(PY_TOOLS)
	VIRTUAL_ENV=$(CURDIR)/$(NOPYX12_VENV) $(NOPYX12_VENV)/bin/maturin develop --uv --manifest-path $(PY_MANIFEST)
	$(NOPYX12_VENV)/bin/python -c "import importlib.util, sys; sys.exit(1 if importlib.util.find_spec('pyx12') else 0)"
	$(NOPYX12_VENV)/bin/python -m pytest -q -rs $(PY_TESTS)

# The stub generator formats defaults through the interpreter, so it links libpython, which the
# extension build leaves to the interpreter that loads it. It builds in its own target directory.
STUB_PYTHON := $(if $(wildcard $(PYTHON)),$(PYTHON),python3)
stubs: ## regenerate the native module's type stub from the binding (links libpython of .venv)
	libdir=$$($(STUB_PYTHON) -c "import sysconfig; print(sysconfig.get_config_var('LIBDIR'))") && \
	ldver=$$($(STUB_PYTHON) -c "import sysconfig; print(sysconfig.get_config_var('LDVERSION'))") && \
	PYO3_PYTHON=$(STUB_PYTHON) CARGO_TARGET_DIR=target/stub-gen \
	RUSTFLAGS="-C link-arg=-L$$libdir -C link-arg=-lpython$$ldver -C link-arg=-Wl,-rpath,$$libdir" \
	cargo run --locked -p oxedi_py --bin stub_gen --features stub-gen

stubtest: py-dev ## check the stub against the built module, the public API with mypy --strict, and the package with mypy
	$(PYTHON) -m mypy.stubtest oxedi._core $(if $(wildcard $(STUBTEST_ALLOWLIST)),--allowlist $(STUBTEST_ALLOWLIST))
	$(PYTHON) -m mypy --strict $(PY_TESTS)/typing/usage.py
	$(PYTHON) -m mypy --config-file crates/oxedi_py/pyproject.toml crates/oxedi_py/python/oxedi

compat-oracle: ## compare with edi-835-parser on DIR (outside the repo); prints counts and verdicts only
	@test -n "$(DIR)" || (echo "usage: make compat-oracle DIR=/path/outside/the/repo [OUT=report.txt]" && exit 1)
	$(PYTHON) crates/oxedi_py/scripts/compat_oracle.py "$(DIR)" $(if $(OUT),--out "$(OUT)")

dist: clean-dist ## build the sdist and the release wheel into target/wheels
	$(MATURIN) sdist --manifest-path $(PY_MANIFEST) -o $(WHEELS)
	@$(MAKE) --no-print-directory sdist-check
	$(MATURIN) build --release --manifest-path $(PY_MANIFEST) -o $(WHEELS)
	@$(MAKE) --no-print-directory wheel-check
	@ls -l $(WHEELS)

# Paths are matched exactly against the archive's listing, under its single top directory.
SDIST_REQUIRED := crates/oxedi_core/src/lib.rs crates/oxedi_core/specs/835.json crates/oxedi_core/specs/835.4010.json crates/oxedi_core/specs/edi_835_parser.json crates/oxedi_core/Cargo.toml crates/oxedi_py/src/lib.rs pyproject.toml LICENSE THIRD_PARTY_NOTICES

sdist-check: ## fail if the sdist holds the core's test trees or the maintainers' scripts, or lacks what the build needs
	@sdist=$$(ls $(WHEELS)/oxedi-*.tar.gz 2>/dev/null | head -n 1); \
	  test -n "$$sdist" || { echo "sdist-check: no oxedi-*.tar.gz in $(WHEELS); run make dist first"; exit 1; }; \
	  listing=$$(tar tzf "$$sdist" | sed 's|^[^/]*/||') || exit 1; \
	  if echo "$$listing" | grep -E '(^|/)oxedi_core/tests/'; then echo "sdist-check: $$sdist holds the core's tests/ tree (samples, golden, fixtures)"; exit 1; fi; \
	  if echo "$$listing" | grep -E '(^|/)oxedi_(core|py)/scripts/'; then echo "sdist-check: $$sdist holds the maintainers' scripts"; exit 1; fi; \
	  if echo "$$listing" | grep -E '(^|/)__pycache__(/|$$)|\.pyc$$'; then echo "sdist-check: $$sdist holds compiled bytecode"; exit 1; fi; \
	  for need in $(SDIST_REQUIRED); do \
	    echo "$$listing" | grep -qxF "$$need" || { echo "sdist-check: $$sdist lacks $$need"; exit 1; }; \
	  done; \
	  top=$$(tar tzf "$$sdist" | head -n 1 | cut -d/ -f1); \
	  for f in LICENSE THIRD_PARTY_NOTICES; do \
	    tar xzOf "$$sdist" "$$top/$$f" | cmp -s - "$$f" || { echo "sdist-check: $$f in $$sdist differs from the repository's $$f"; exit 1; }; \
	  done; echo "sdist-check: ok ($$(echo "$$listing" | wc -l) entries)"

wheel-check: ## fail if the built wheel lacks the type stub, py.typed or the licenses, or holds bytecode
	@wheel=$$(ls $(WHEELS)/oxedi-*.whl 2>/dev/null | head -n 1); \
	  test -n "$$wheel" || { echo "wheel-check: no oxedi-*.whl in $(WHEELS); run make dist first"; exit 1; }; \
	  $(PYTHON) crates/oxedi_py/scripts/check_wheel.py "$$wheel" .

readme-py: ## regenerate the PyPI README from README.md (relative links become absolute)
	$(README_PY_RENDER) > crates/oxedi_py/README.md

readme-py-check: ## fail if crates/oxedi_py/README.md is not what make readme-py generates
	$(call check-generated,readme-py-check,$(README_PY_RENDER),crates/oxedi_py/README.md,readme-py)

smoke: ## install the built wheel in a clean venv outside the repo and run the suite
	crates/oxedi_py/scripts/smoke_wheel.sh

clean-dist:
	rm -rf $(WHEELS)
