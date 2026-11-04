# Complexity ratchet, dependency leaks and profiling. Included by the root Makefile.
.PHONY: complexity complexity-update deps-check profile profile-py

##@ Code quality

# Needs uv; CI runs it in the Workflows job. The targets and the baseline are explained in
# CONTRIBUTING.md (Complexity).
complexity: ## fail if a Rust, Python or JS function is over the complexity targets and not in the baseline, worse than it, or modified while still over a target
	tools/complexity/complexity.sh

complexity-update: ## rewrite .github/complexity-baseline.txt after a refactor or move; ACCEPT='path::function ...' records reviewed exceptions
	tools/complexity/complexity.sh --update $(foreach a,$(ACCEPT),--accept '$(a)')

# The allowed lists live in tools/deps/deps.py. The crate check needs cargo only; the wheel check
# runs when `make dist` has left a wheel in target/wheels (CI runs both).
deps-check: ## fail if a published crate has an unlisted normal dependency or the wheel an unconditional Requires-Dist
	python3 tools/deps/deps.py crates
	@if ls $(WHEELS)/*.whl >/dev/null 2>&1; then python3 tools/deps/deps.py wheel; else echo "deps-check: no wheel in $(WHEELS), wheel check skipped (make dist builds one)"; fi

##@ Profiling
# Unix only. Outputs: see CONTRIBUTING.md (Profiling).
SECONDS ?= 10
PY_PROFILE_DIR := target/profile-py
# tools/profile/py_spy_record.sh fails on a script error, a py-spy error or a missing output.
define py-spy-record
tools/profile/py_spy_record.sh $(VENV)/bin/py-spy $(1) $(PY_PROFILE_DIR)/$(2) $(PYTHON) "$(SCRIPT)" $(ARGS)
endef

profile: ## profile a core bench under a sampler: BENCH=process [SECONDS=10]; writes flamegraph.svg and top.txt under target/criterion/
	@test -n "$(BENCH)" || { echo "usage: make profile BENCH=<tokenize|engine|check|process|write> [SECONDS=10]"; exit 1; }
	@stamp=$$(mktemp) && trap 'rm -f "$$stamp"' EXIT && \
	  cargo bench --locked -p oxedi_core --bench $(BENCH) -- --profile-time $(SECONDS) && \
	  written=$$(find target/criterion -path '*/profile/*' -newer "$$stamp" -type f | sort) && \
	  { test -n "$$written" || { echo "profile: the run of bench $(BENCH) wrote no profile file (the sampler reports why on stderr)"; exit 1; }; } && \
	  echo "profile: written by this run:" && echo "$$written"

profile-py: ## profile a Python script with py-spy: SCRIPT=path [ARGS='...']; writes flamegraph.svg and stacks.txt under target/profile-py/
	@test -n "$(SCRIPT)" || { echo "usage: make profile-py SCRIPT=path/to/script.py [ARGS='...']"; exit 1; }
	@test -x $(VENV)/bin/py-spy || { echo "profile-py: $(VENV)/bin/py-spy is missing; run make venv"; exit 1; }
	VIRTUAL_ENV=$(VENV) $(MATURIN) develop --uv --release --manifest-path $(PY_MANIFEST)
	mkdir -p $(PY_PROFILE_DIR) && rm -f $(PY_PROFILE_DIR)/flamegraph.svg $(PY_PROFILE_DIR)/stacks.txt
	$(call py-spy-record,flamegraph,flamegraph.svg)
	$(call py-spy-record,raw,stacks.txt)
	@echo "profile-py: wrote $(PY_PROFILE_DIR)/flamegraph.svg and $(PY_PROFILE_DIR)/stacks.txt"
