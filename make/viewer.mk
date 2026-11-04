# Browser viewer (crates/oxedi_wasm, viewer/). Included by the root Makefile.
.PHONY: wasm-bindgen-version wasm wasm-size-check viewer-serve viewer-units viewer-oracle viewer-test

##@ Viewer
# The module is built for the browser with size-first release settings passed as environment
# variables, so native release builds (wheels, the DuckDB extension) keep the workspace profile.
# The wasm-bindgen CLI must be the exact version Cargo.lock resolves for the crate; CI reads it
# from wasm-bindgen-version too. Expanded only by the recipes that use it. The wasm recipe empties
# viewer/pkg instead of removing it, since the sandbox mounts its own directory there.
WASM_TARGET     := wasm32-unknown-unknown
WASM_BUILT      := target/$(WASM_TARGET)/release/oxedi_wasm.wasm
VIEWER_PKG      := viewer/pkg
WASM_BINDGEN_VERSION = $(shell cargo tree -p oxedi_wasm --locked --depth 1 -e normal --prefix none | sed -n 's/^wasm-bindgen v\([^ ]*\).*/\1/p')
# 250 KB, gzipped.
WASM_SIZE_LIMIT := 256000

wasm-bindgen-version: ## print the wasm-bindgen version Cargo.lock resolves for oxedi_wasm
	@echo $(WASM_BINDGEN_VERSION)

wasm: ## build the viewer's WebAssembly module into viewer/pkg
	@rustup target list --installed | grep -qx '$(WASM_TARGET)' || rustup target add $(WASM_TARGET)
	@v=$$(wasm-bindgen --version 2>/dev/null | cut -d' ' -f2); \
	  test "$$v" = "$(WASM_BINDGEN_VERSION)" || { echo "wasm: wasm-bindgen $(WASM_BINDGEN_VERSION) is required (Cargo.lock) but found '$${v:-none}'; install it with: cargo install wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION) --locked"; exit 1; }
	@command -v wasm-opt >/dev/null || { echo "wasm: wasm-opt not found; install binaryen (https://github.com/WebAssembly/binaryen/releases) and put wasm-opt on PATH"; exit 1; }
	CARGO_PROFILE_RELEASE_OPT_LEVEL=s CARGO_PROFILE_RELEASE_LTO=true \
	  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_PANIC=abort \
	  cargo build -p oxedi_wasm --release --locked --target $(WASM_TARGET)
	mkdir -p $(VIEWER_PKG) && find $(VIEWER_PKG) -mindepth 1 -delete
	wasm-bindgen --target web --out-dir $(VIEWER_PKG) --out-name oxedi --no-typescript $(WASM_BUILT)
	wasm-opt -Os $(VIEWER_PKG)/oxedi_bg.wasm -o $(VIEWER_PKG)/oxedi_bg.wasm
	$(PYTHON_BIN) crates/oxedi_wasm/scripts/embed_wasm.py

# The page ships module.js (the module as base64); its gzipped size is printed, not gated.
wasm-size-check: ## fail if the gzipped module is over 250 KB
	@for f in oxedi_bg.wasm module.js; do test -f $(VIEWER_PKG)/$$f || { echo "wasm-size-check: no $(VIEWER_PKG)/$$f; run make wasm first"; exit 1; }; done
	@raw=$$(wc -c < $(VIEWER_PKG)/oxedi_bg.wasm); gz=$$(gzip -9 -c $(VIEWER_PKG)/oxedi_bg.wasm | wc -c); \
	  js=$$(gzip -9 -c $(VIEWER_PKG)/module.js | wc -c); \
	  echo "wasm-size-check: oxedi_bg.wasm $$raw bytes, $$gz gzipped (limit $(WASM_SIZE_LIMIT)); module.js $$js gzipped (shipped)"; \
	  test $$gz -le $(WASM_SIZE_LIMIT) || { echo "wasm-size-check: the gzipped module is $$gz bytes, over $(WASM_SIZE_LIMIT)"; exit 1; }

viewer-serve: wasm ## serve the viewer at http://127.0.0.1:8001
	python3 -m http.server 8001 --directory viewer

viewer-units: ## run the page's pure-function checks in node
	node viewer/tests/units.mjs

viewer-oracle: docs-dev wasm ## compare the WebAssembly module with oxedi.parse on every sample and fixture
	@out=$$(mktemp -d); trap 'rm -rf "$$out"' EXIT; \
	  $(DOCS_PYTHON) viewer/tests/expected.py "$$out" && node viewer/tests/oracle.mjs "$$out"

viewer-test: viewer-units viewer-oracle ## run the page's unit checks, the oracle and the browser tests (needs: .venv-docs/bin/playwright install chromium)
	$(DOCS_PYTHON) -m pytest viewer/tests -q
