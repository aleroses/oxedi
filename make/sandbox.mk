# The sandbox: a pinned image and a runner that builds and tests this checkout inside a hardened,
# throwaway container (tools/sandbox/sandbox.py says how).
# Included by the root Makefile.
.PHONY: sandbox-image sandbox-image-viewer sandbox sandbox-pr sandbox-check sandbox-pr-check sandbox-tools-check sandbox-clean

##@ Sandbox
SANDBOX_DIR   := tools/sandbox
# Every Python this file starts runs isolated (-I): a module a run left in the checkout, such as a
# root hashlib.py, or a PYTHON* variable of the shell is never loaded.
SANDBOX_PY    := $(PYTHON_BIN) -I
# The tag is the first 12 hex digits of the SHA-256 of the Dockerfile, pip-audit.txt and
# rust-toolchain.toml, the three files the build reads (the Dockerfile COPYs pip-audit.txt and takes
# the Rust version from the last as a build argument), so a change to any of them is a new tag.
SANDBOX_HASH   = $(shell $(SANDBOX_PY) -c "import hashlib, pathlib; print(hashlib.sha256(b''.join(pathlib.Path(n).read_bytes() for n in ('$(SANDBOX_DIR)/Dockerfile', '$(SANDBOX_DIR)/pip-audit.txt', 'rust-toolchain.toml'))).hexdigest()[:12])")
SANDBOX_IMAGE  = oxedi-sandbox:$(SANDBOX_HASH)
# The viewer image is built on the base one: its tag follows the base tag, its Dockerfile, playwright.txt,
# install_tools.sh and viewer_image.py (that file derives the tag and the wasm and Playwright pins).
SANDBOX_VIEWER_IMAGE = $(shell $(SANDBOX_PY) $(SANDBOX_DIR)/viewer_image.py tag --root $(CURDIR) --base $(SANDBOX_IMAGE))
SANDBOX_DOCKER_ARCH  = $(shell docker version --format '{{.Server.Arch}}' 2> /dev/null)

# The image sandbox and sandbox-pr run in: IMAGE=base (the default) or IMAGE=viewer, from the command
# line only, so a variable left in the shell never picks the image. Anything else is refused by
# SANDBOX_REFUSE before any docker command; the value reaches that shell as SANDBOX_IMAGE_VALUE,
# never spliced into the recipe.
ifeq ($(origin IMAGE),undefined)
SANDBOX_KIND := base
else ifneq ($(origin IMAGE),command line)
SANDBOX_KIND := environment
else ifeq ($(IMAGE),base)
SANDBOX_KIND := base
else ifeq ($(IMAGE),viewer)
SANDBOX_KIND := viewer
else
SANDBOX_KIND := invalid
endif
SANDBOX_RUN_IMAGE   = $(if $(filter viewer,$(SANDBOX_KIND)),$(SANDBOX_VIEWER_IMAGE),$(SANDBOX_IMAGE))
SANDBOX_IMAGE_TARGET = $(if $(filter viewer,$(SANDBOX_KIND)),sandbox-image-viewer,sandbox-image)
SANDBOX_REFUSAL_environment = printf '%s\n' "$(1): IMAGE is read from the command line only; pass it as make $(1) IMAGE=viewer" >&2; exit 2
SANDBOX_REFUSAL_invalid = printf "%s: IMAGE must be base or viewer, not '%s'\n" $(1) "$$SANDBOX_IMAGE_VALUE" >&2; exit 2
# $(1) is the target: the refusal line starts with its name.
define SANDBOX_REFUSE
$(SANDBOX_REFUSAL_$(SANDBOX_KIND)) $(if $(filter base viewer,$(SANDBOX_KIND)),true)
endef
# Refuses, with exit 2 and before any docker command, the first variable of $(2) that is set
# but not on the command line, so a variable left in the shell never reaches the runner. $(1) is
# the target; each variable shows its own example.
SANDBOX_EXAMPLE_CMD     = CMD="make gates"
SANDBOX_EXAMPLE_PR      = PR=251
SANDBOX_EXAMPLE_REF     = REF=feature
SANDBOX_EXAMPLE_HEAD    = HEAD=1
SANDBOX_EXAMPLE_TARGET  = TARGET="make py-test"
SANDBOX_EXAMPLE_CONFIRM = CONFIRM=yes
SANDBOX_EXAMPLE_IMAGE   = IMAGE=viewer
define SANDBOX_REFUSE_VARS
$(foreach v,$(2),$(if $(findstring command line,$(origin $(v))),,$(if $(filter undefined,$(origin $(v))),,printf '%s\n' '$(1): $(v) is read from the command line only; pass it as make $(1) $(SANDBOX_EXAMPLE_$(v))' >&2; exit 2;)))
endef
# GNU make takes a variable defined in an exported MAKEFLAGS or GNUMAKEFLAGS as one from the
# command line, so the top-level make (MAKELEVEL empty or 0) also refuses, with exit 2, the first
# variable of $(2) that the environment's MAKEFLAGS or GNUMAKEFLAGS assigns. The environment make
# started with is read from /proc (make 4.4 gives $(shell) its own MAKEFLAGS, with the command
# line's variables), else from the shell when that sees make's starting environment (MAKELEVEL unset).
# On Linux the check reads the environment make started with, from `/proc`; on another host with GNU
# make 4.4 or later, an assignment in an exported `MAKEFLAGS` cannot be told apart from the command
# line, so there only the origin check holds.
# The value only feeds $(filter), never the recipe.
define SANDBOX_STARTING_ENV
$(shell { if [ -r /proc/$$PPID/environ ]; then tr '\0' '\n' < /proc/$$PPID/environ; elif [ "$${MAKELEVEL:-0}" = 0 ]; then env; fi; } | sed -n 's/^$(1)=//p')
endef
SANDBOX_ASSIGNS = $(filter $(1)=% $(1):=% $(1)::=% $(1):::=% $(1)+=% $(1)?=% $(1)!=%,$(2))
SANDBOX_FLAGS_REFUSAL = $(foreach v,$(2),$(if $(call SANDBOX_ASSIGNS,$(v),$(4)),printf '%s\n' '$(1): $(v) comes from $(3) in the environment; unset $(3) and pass it as make $(1) $(SANDBOX_EXAMPLE_$(v))' >&2; exit 2;))
define SANDBOX_REFUSE_FLAGS
$(if $(filter-out 0,$(MAKELEVEL)),,$(foreach f,MAKEFLAGS GNUMAKEFLAGS,$(call SANDBOX_FLAGS_REFUSAL,$(1),$(2),$(f),$(call SANDBOX_STARTING_ENV,$(f)))))
endef
# The image of the run, built first when missing (sandbox-image or sandbox-image-viewer).
define SANDBOX_ENSURE_IMAGE
test -n "$(SANDBOX_RUN_IMAGE)" || exit 1; \
	if command -v docker > /dev/null && ! docker image inspect $(SANDBOX_RUN_IMAGE) > /dev/null 2>&1; then \
	  $(MAKE) --no-print-directory $(SANDBOX_IMAGE_TARGET); \
	fi
endef

# Two builds: the second reuses every layer and only adds the apt set, read from the first, as a label.
sandbox-image: ## build the sandbox image from tools/sandbox (tag oxedi-sandbox:<hash of the Dockerfile, pip-audit.txt and rust-toolchain.toml>)
	rust=$$($(SANDBOX_PY) $(SANDBOX_DIR)/rust_version.py --root $(CURDIR)) && \
	  docker build --build-arg RUST_VERSION=$$rust --tag $(SANDBOX_IMAGE) $(SANDBOX_DIR) && \
	  apt=$$(docker run --rm --pull never --network none --read-only --cap-drop ALL --user 65534:65534 --security-opt no-new-privileges $(SANDBOX_IMAGE) cat /usr/share/oxedi-sandbox.manifest | tr '\t\n' '= ' | sed 's/ $$//') && \
	  docker build --build-arg RUST_VERSION=$$rust --tag $(SANDBOX_IMAGE) --label "oxedi.sandbox.apt=$$apt" $(SANDBOX_DIR)
	@echo "sandbox-image: $(SANDBOX_IMAGE), $$(docker image inspect --format '{{.Size}}' $(SANDBOX_IMAGE) | awk '{ printf "%.0f MB", $$1 / 1000000 }')"

# Two builds, like sandbox-image. The base image is built first when missing; the wasm tools are
# pinned for x86_64, so another docker server architecture is refused.
sandbox-image-viewer: ## build the viewer sandbox image (Node, Chromium, wasm tools) on the base image (amd64 only)
	@arch='$(SANDBOX_DOCKER_ARCH)'; \
	  test -n "$$arch" || { echo "sandbox-image-viewer: the docker daemon could not be reached (docker version names no server); start it and retry" >&2; exit 1; }; \
	  test "$$arch" = amd64 || { echo "sandbox-image-viewer: the docker server is '$$arch' and the wasm tools are pinned for amd64" >&2; exit 1; }
	@test -n "$(SANDBOX_VIEWER_IMAGE)" || exit 1
	@docker image inspect $(SANDBOX_IMAGE) > /dev/null 2>&1 || $(MAKE) --no-print-directory sandbox-image
	args=$$($(SANDBOX_PY) $(SANDBOX_DIR)/viewer_image.py build-args --root $(CURDIR)) && \
	  docker build -f $(SANDBOX_DIR)/Dockerfile.viewer --build-arg BASE_IMAGE=$(SANDBOX_IMAGE) $$args --tag $(SANDBOX_VIEWER_IMAGE) $(SANDBOX_DIR) && \
	  apt=$$(docker run --rm --pull never --network none --read-only --cap-drop ALL --user 65534:65534 --security-opt no-new-privileges $(SANDBOX_VIEWER_IMAGE) cat /usr/share/oxedi-sandbox.manifest | tr '\t\n' '= ' | sed 's/ $$//') && \
	  docker build -f $(SANDBOX_DIR)/Dockerfile.viewer --build-arg BASE_IMAGE=$(SANDBOX_IMAGE) $$args --tag $(SANDBOX_VIEWER_IMAGE) --label "oxedi.sandbox.apt=$$apt" $(SANDBOX_DIR)
	@echo "sandbox-image-viewer: $(SANDBOX_VIEWER_IMAGE), $$(docker image inspect --format '{{.Size}}' $(SANDBOX_VIEWER_IMAGE) | awk '{ printf "%.0f MB", $$1 / 1000000 }')"

# CMD reaches the container's `bash -c` as SANDBOX_CMD, expanded once by make ($$ is a literal $)
# and with no other quoting layer. A missing image is built first.
sandbox: export SANDBOX_CMD := $(CMD)
sandbox sandbox-pr sandbox-check: export SANDBOX_IMAGE_VALUE := $(IMAGE)
sandbox: ## run CMD over this checkout in a hardened container with no network (make sandbox CMD="make gates" [IMAGE=viewer])
	@$(call SANDBOX_REFUSE_VARS,sandbox,CMD) true
	@$(call SANDBOX_REFUSE_FLAGS,sandbox,CMD IMAGE) true
	@$(call SANDBOX_REFUSE,sandbox)
	@$(SANDBOX_ENSURE_IMAGE)
	@$(SANDBOX_PY) $(SANDBOX_DIR)/sandbox.py --image $(SANDBOX_RUN_IMAGE) --kind $(SANDBOX_KIND) --checkout $(CURDIR)

# The change, the target and the confirmation reach sandbox_pr.py as SANDBOX_* variables, like CMD
# for sandbox. The rules (sandbox_pr.py, risk.py, deps.py, report.py, deny.toml) are this checkout's, never the change's.
sandbox-pr: export SANDBOX_PR := $(PR)
sandbox-pr: export SANDBOX_REF := $(REF)
sandbox-pr: export SANDBOX_HEAD := $(HEAD)
sandbox-pr: export SANDBOX_TARGET := $(TARGET)
sandbox-pr: export SANDBOX_CONFIRM := $(CONFIRM)
sandbox-pr: ## test a pull request (PR=N [HEAD=1]) or a ref (REF=<name>) in the sandbox, after a risk report [TARGET="..."] [CONFIRM=yes] [IMAGE=viewer]
	@$(call SANDBOX_REFUSE_VARS,sandbox-pr,PR REF HEAD TARGET CONFIRM) true
	@$(call SANDBOX_REFUSE_FLAGS,sandbox-pr,PR REF HEAD TARGET CONFIRM IMAGE) true
	@$(call SANDBOX_REFUSE,sandbox-pr)
	@$(SANDBOX_ENSURE_IMAGE)
	@$(SANDBOX_PY) $(SANDBOX_DIR)/sandbox_pr.py --image $(SANDBOX_RUN_IMAGE) --kind $(SANDBOX_KIND) --checkout $(CURDIR)

# The host home path is an argument, so the check proves that path is not mounted. IMAGE=viewer
# checks the viewer image; IMAGE reaches the inner make as a command-line variable, through MAKEFLAGS.
sandbox-check: ## prove the sandbox's isolation from inside it: every line of tools/sandbox/isolation-check.sh says ok [IMAGE=viewer]
	@$(call SANDBOX_REFUSE_FLAGS,sandbox-check,IMAGE) true
	@$(call SANDBOX_REFUSE,sandbox-check)
	@$(MAKE) --no-print-directory sandbox CMD='bash $(SANDBOX_DIR)/isolation-check.sh "$(HOME)"'

# The planted change lives only on a local branch of a temporary clone, removed at the end.
sandbox-pr-check: ## prove sandbox-pr keeps a hostile change from the host, the network and the caches: every line of tools/sandbox/malicious-pr.sh says ok
	@if command -v docker > /dev/null && ! docker image inspect $(SANDBOX_IMAGE) > /dev/null 2>&1; then \
	  $(MAKE) --no-print-directory sandbox-image; \
	fi
	@bash $(SANDBOX_DIR)/malicious-pr.sh $(SANDBOX_IMAGE) $(CURDIR)

# Needs the network once: it fetches the advisory database into a temporary directory.
sandbox-tools-check: ## prove cargo-deny and pip-audit work in the image: fetch the advisory database, then check a crate offline
	@if command -v docker > /dev/null && ! docker image inspect $(SANDBOX_IMAGE) > /dev/null 2>&1; then \
	  $(MAKE) --no-print-directory sandbox-image; \
	fi
	@bash $(SANDBOX_DIR)/audit-tools-check.sh $(SANDBOX_IMAGE)

sandbox-clean: ## remove this checkout's sandbox storage (ALL=1: every checkout's and the shared caches)
	@$(SANDBOX_PY) $(SANDBOX_DIR)/sandbox.py --clean $(if $(ALL),all,checkout) --checkout $(CURDIR)
