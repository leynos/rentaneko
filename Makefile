.PHONY: help all clean test build release coverage lint fmt check-fmt markdownlint nixie audit rust-audit spelling

SHELL := bash


TARGET ?= librentaneko.rlib

CARGO ?= cargo
CARGO_AUDIT_DEFAULT_IGNORES ?= RUSTSEC-2023-0071 RUSTSEC-2024-0370
CARGO_AUDIT_DEFAULT_IGNORE_PACKAGES ?= rsa proc-macro-error
BUILD_JOBS ?=
RUST_FLAGS ?=
RUST_FLAGS := -D warnings $(RUST_FLAGS)
# The build standard: every `rustflags` source in `.cargo/config.toml` carries
# the parallel frontend, and the Linux source adds mold. Assigning `RUSTFLAGS`
# replaces those sources outright, so the gate targets restate the flags here.
# The recipes add them to any inherited `RUSTFLAGS` (setup-rust exports one
# in CI) instead of replacing it.
# Coverage and release builds deliberately take neither, and coverage builds on
# LLVM because `-Cinstrument-coverage` is LLVM-only.
STANDARD_THREADS_FLAG ?= -Zthreads=8
STANDARD_MOLD_FLAG ?= -Clink-arg=-fuse-ld=mold
BUILD_HOST_OS := $(shell uname -s)
# mold is added only when the machine doing the build is Linux (only Make can
# tell whether it has mold) and the compilation target is Linux too, which is
# the host unless `CARGO_BUILD_TARGET` names another triple. Make reads the
# target from that variable alone, so a `--target` in the flag variables, which
# it cannot see, is an error rather than a silent assumption of a Linux target.
STANDARD_CLI_TARGETS = $(filter --target --target=%,$(CARGO_FLAGS) $(TEST_FLAGS) $(CLIPPY_FLAGS))
STANDARD_TARGET_IS_LINUX = $(if $(STANDARD_CLI_TARGETS),$(error `--target` in CARGO_FLAGS, TEST_FLAGS or CLIPPY_FLAGS is not read by Make: set CARGO_BUILD_TARGET instead),$(if $(CARGO_BUILD_TARGET),$(or $(findstring -linux-,$(CARGO_BUILD_TARGET)),$(filter host-tuple,$(CARGO_BUILD_TARGET))),yes))
STANDARD_RUSTFLAGS = $(STANDARD_THREADS_FLAG)$(if $(filter Linux,$(BUILD_HOST_OS)),$(if $(STANDARD_TARGET_IS_LINUX), $(STANDARD_MOLD_FLAG)))
# Release builds take neither flag: assigning `RUSTFLAGS`, even to an empty
# inherited value, displaces every `rustflags` source in the configuration.
RELEASE_RUSTFLAGS = RUSTFLAGS="$${RUSTFLAGS-}"
# Debug builds keep a caller's exported flags and add the standard ones,
# since an inherited `RUSTFLAGS` would otherwise displace the configuration.
DEBUG_RUSTFLAGS = RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$(STANDARD_RUSTFLAGS)"
RUSTDOC_FLAGS ?=
RUSTDOC_FLAGS := -D warnings $(RUSTDOC_FLAGS)
CARGO_FLAGS ?= --all-targets --all-features
CLIPPY_FLAGS ?= $(CARGO_FLAGS) -- $(RUST_FLAGS)
TEST_FLAGS ?= $(CARGO_FLAGS)
TEST_CMD := $(if $(shell $(CARGO) nextest --version 2>/dev/null),nextest run,test)
COVERAGE_LINKER_FLAGS ?= -fuse-ld=lld
COVERAGE_RUST_FLAGS ?= $(RUST_FLAGS) -C link-arg=$(COVERAGE_LINKER_FLAGS)
MDLINT ?= markdownlint-cli2
# `make fmt` and `make check-fmt` call mdtablefix directly. `--git` selects the
# Markdown files Git tracks and `--include-untracked` adds the untracked files
# Git does not ignore, so a new document is formatted before it is staged.
# Both modes need mdtablefix 0.6.0 or later; CI pins the version at the
# install-mdtablefix step.
MDTABLEFIX ?= mdtablefix
MDTABLEFIX_SELECT = --git --include-untracked
MDTABLEFIX_RULES = --wrap --renumber --breaks --ellipsis --fences
NIXIE ?= nixie
WHITAKER ?= whitaker
UV ?= uv
UV_ENV = UV_CACHE_DIR=.uv-cache UV_TOOL_DIR=.uv-tools
TYPOS_CONFIG_BUILDER_VERSION ?= v0.1.1
TYPOS_CONFIG_BUILDER = $(UV_ENV) $(UV) tool run --python 3.14 --from \
	"git+https://github.com/leynos/typos-config-builder.git@$(TYPOS_CONFIG_BUILDER_VERSION)" \
	typos-config-builder

build: target/debug/$(TARGET) ## Build debug binary
release: target/release/$(TARGET) ## Build release binary

all: check-fmt lint test spelling ## Perform a comprehensive check of code and prose

clean: ## Remove build artefacts
	$(CARGO) clean

test: ## Run tests with warnings treated as errors
	RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$(RUST_FLAGS) $(STANDARD_RUSTFLAGS)" $(CARGO) $(TEST_CMD) $(TEST_FLAGS) $(BUILD_JOBS)
	RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$(RUST_FLAGS) $(STANDARD_RUSTFLAGS)" $(CARGO) test --doc --workspace --all-features

target/%/$(TARGET): ## Build binary in debug or release mode
	$(if $(findstring release,$(@)),$(RELEASE_RUSTFLAGS),$(DEBUG_RUSTFLAGS)) $(CARGO) build $(BUILD_JOBS) $(if $(findstring release,$(@)),--release)

coverage: ## Generate lcov coverage with lld for llvm-tools compatibility
	@echo "coverage linker flags: $(COVERAGE_LINKER_FLAGS)"
	CARGO_PROFILE_DEV_CODEGEN_BACKEND=llvm CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=clang RUSTFLAGS="$(COVERAGE_RUST_FLAGS)" \
		CFLAGS="$(COVERAGE_LINKER_FLAGS)" LDFLAGS="$(COVERAGE_LINKER_FLAGS)" \
		$(CARGO) llvm-cov --lcov --output-path lcov.info $(TEST_FLAGS)

lint: ## Run Clippy and the Whitaker Dylint suite with warnings denied
	RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$(RUST_FLAGS) $(STANDARD_RUSTFLAGS)" RUSTDOCFLAGS="$(RUSTDOC_FLAGS)" $(CARGO) doc --no-deps
	RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$(RUST_FLAGS) $(STANDARD_RUSTFLAGS)" $(CARGO) clippy $(CLIPPY_FLAGS)
	RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$(RUST_FLAGS) $(STANDARD_RUSTFLAGS)" $(WHITAKER) --all -- $(CARGO_FLAGS)

typecheck: ## Type-check without building
	RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$(RUST_FLAGS) $(STANDARD_RUSTFLAGS)" $(CARGO) check $(CARGO_FLAGS)

fmt: ## Format Rust and Markdown sources
	$(CARGO) +nightly fmt --all
	$(MDTABLEFIX) --in-place $(MDTABLEFIX_SELECT) $(MDTABLEFIX_RULES)
	$(MDLINT) --fix "**/*.md"

check-fmt: ## Verify formatting
	$(CARGO) fmt --all -- --check
	$(MDTABLEFIX) --check $(MDTABLEFIX_SELECT) $(MDTABLEFIX_RULES)

markdownlint: spelling ## Lint Markdown files and enforce spelling
	find . -type f -name '*.md' -not -path './target/*' \
		-not -path './.uv-cache/*' -not -path './.uv-tools/*' -print0 | \
		xargs -0 $(MDLINT)

spelling: ## Enforce en-GB-oxendict spelling
	$(TYPOS_CONFIG_BUILDER) gate --repository .

nixie: ## Validate Mermaid diagrams
	$(NIXIE) --no-sandbox

audit: rust-audit ## Audit dependencies for known vulnerabilities

rust-audit: ## Audit the Rust workspace for known vulnerabilities
	set -eo pipefail; \
	manifest_list=$$(mktemp); \
	trap 'rm -f "$$manifest_list"' EXIT; \
	printf "Audit metadata phase: deriving workspace manifests\n"; \
	$(CARGO) metadata --no-deps --format-version 1 | python3 -c 'import json, sys; metadata = json.load(sys.stdin); members = set(metadata["workspace_members"]); print(metadata["workspace_root"]); [print(package["manifest_path"]) for package in metadata["packages"] if package["id"] in members]' > "$$manifest_list"; \
	workspace_root=$$(sed -n '1p' "$$manifest_list"); \
	printf "Audit policy phase: checking default ignores are development-only\n"; \
	production_tree=$$(cd "$$workspace_root" && $(CARGO) tree --workspace --all-features --edges normal,build --prefix none); \
	for package in $(CARGO_AUDIT_DEFAULT_IGNORE_PACKAGES); do \
		if printf '%s\n' "$$production_tree" | awk -v package="$$package" '$$1 == package { found = 1 } END { exit !found }'; then \
			printf "Refusing default RustSec ignore: package %s is reachable through a normal or build dependency.\n" "$$package"; \
			exit 1; \
		fi; \
	done; \
	audit_flags=(); \
	for advisory in $(CARGO_AUDIT_DEFAULT_IGNORES) $$CARGO_AUDIT_IGNORES; do \
		audit_flags+=(--ignore "$$advisory"); \
	done; \
	printf "Auditing Rust workspace %s\n" "$$workspace_root"; \
	sed -n '2,$$p' "$$manifest_list" | while IFS= read -r manifest; do \
		manifest_dir=$$(dirname "$$manifest"); \
		printf "Workspace Rust manifest %s\n" "$$manifest_dir/Cargo.toml"; \
	done; \
	printf "Audit execution phase: running cargo audit\n"; \
	printf "Audit failures may indicate RustSec advisories, cargo metadata errors, or documented ignores that need CARGO_AUDIT_IGNORES entries.\n"; \
	(cd "$$workspace_root" && $(CARGO) audit "$${audit_flags[@]}")

help: ## Show available targets
	@grep -E '^[a-zA-Z_-]+:.*?##' $(MAKEFILE_LIST) | \
	awk 'BEGIN {FS=":"; printf "Available targets:\n"} {printf "  %-20s %s\n", $$1, $$2}'
