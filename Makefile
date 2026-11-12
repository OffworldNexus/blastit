# Blastit root Makefile: unified quality gates for Rust crates, Python tooling, docs, and the plan tree.
#
#   make help           # list targets
#   make check          # fast unprivileged repo gate: guard + critical-guard + rust + python tools + docs + plan
#   make clean          # format Rust and Python harness tools, then run lints
#   make docs-check     # build Zensical documentation site with --strict (zero broken links/anchors)
#   make test-critical  # run protected critical tests only (Rust + Python)
#   make e2e            # build wheel and run Docker + rootless Docker e2e scenarios (gates)
#   make mutants-rust   # run cargo-mutants on critical crates (PKG=blastit-sqlpolicy)

UV ?= uv
UVX ?= uvx --offline
CARGO ?= cargo

# Resolve real CPython >= 3.13 binary via uv (following symlinks so ../lib holds libpython3.X.so).
PYO3_BIN := $(shell readlink -f "$$($(UV) python find --no-project '>=3.13' 2>/dev/null || which python3)")
PYO3_LIBDIR := $(abspath $(dir $(PYO3_BIN))/../lib)
# Install prefix (parent of bin/): embedded interpreters (PyO3 test binaries) need it as PYTHONHOME
# at runtime, otherwise they cannot find the uv-managed stdlib ("No module named 'encodings'").
PYO3_PREFIX := $(if $(PYO3_BIN),$(abspath $(dir $(PYO3_BIN))/..),)
RUST_ENV = $(if $(PYO3_PREFIX),PYTHONHOME="$(PYO3_PREFIX)") PYO3_PYTHON="$(PYO3_BIN)" LD_LIBRARY_PATH="$(PYO3_LIBDIR)$${LD_LIBRARY_PATH:+:$$LD_LIBRARY_PATH}"

MUTANTS_TMP ?= /tmp/opencode/blastit-mutants
MUTANTS_JOBS ?= 2
MUTANTS_TIMEOUT ?= 90

# Where `test-critical` stages the built package and extension, so `unittest`
# imports the real `blastit._blastit` without an install (R42, §7.1).
CRITICAL_STAGE ?= $(CURDIR)/target/python-critical
# The attribute/call a protected critical test may never contain. Comments never
# match because each alternative names the construct itself, not a bare word.
CRITICAL_SKIP_PATTERN := \#\[[[:space:]]*ignore|\bneeds_root[[:space:]]*\(|pytest\.mark\.skip|pytest\.skip[[:space:]]*\(|importorskip[[:space:]]*\(|unittest\.skip

.PHONY: help check clean guard critical-guard critical-skip-check plan-validate \
        rs-format rs-fmt rs-lint rs-doc rs-test rs-test-critical \
        py-format py-lint test-critical docs-check docs-test e2e mutants-rust

help: ## Show available Make targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-18s\033[0m %s\n", $$1, $$2}'

clean: rs-format py-format rs-lint py-lint ## Format Rust + Python tools, then lint

guard: ## Refuse >5 MiB files, .env secrets, or stray binary artefacts
	$(UV) run --no-project tools/guard.py

critical-guard: ## Protect critical tests (report by default; pass BASE=<rev> NODE=<id> to enforce)
	$(UV) run --no-project tools/critical_guard.py --base $(or $(BASE),HEAD) $(if $(NODE),--node $(NODE))

plan-validate: ## Validate plan/nodes consistency
	$(UV) run --no-project tools/plan.py validate

rs-format: ## Format Rust workspace
	$(CARGO) fmt --all

rs-fmt: ## Check Rust workspace formatting
	$(CARGO) fmt --all -- --check

rs-lint: ## Clippy workspace with warnings denied
	$(RUST_ENV) $(CARGO) clippy --workspace --all-targets --locked -- -D warnings

rs-doc: ## Check rustdoc across workspace with warnings denied
	$(RUST_ENV) RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --locked

rs-test: ## Run all unprivileged Rust unit, integration, and doc tests
	$(RUST_ENV) $(CARGO) test --workspace --all-targets --locked
	$(RUST_ENV) $(CARGO) test --workspace --doc --locked

rs-test-critical: ## Run every crates/*/tests/critical_*.rs suite
	@set -e; found=0; for file in crates/*/tests/critical_*.rs; do \
		[ -e "$$file" ] || continue; found=1; \
		crate=$$(basename $$(dirname $$(dirname "$$file"))); \
		name=$$(basename "$$file" .rs); \
		echo "cargo test -p $$crate --test $$name"; \
		$(RUST_ENV) $(CARGO) test -p "$$crate" --test "$$name" --locked; \
	done; [ "$$found" -eq 1 ] || echo "no Rust critical tests yet (crates/*/tests/critical_*.rs)"

py-format: ## Format Python harness tools and the published package
	$(UVX) ruff check --fix --select I python tools
	$(UVX) ruff format python tools

py-lint: ## Lint and format-check the Python package and harness tools
	$(UVX) ruff check python tools
	$(UVX) ruff format --check python tools

critical-skip-check: ## Refuse a skip construct in any protected critical test (R42)
	@set -e; \
	files=$$(ls crates/*/tests/critical_*.rs python/tests/critical/test_*.py 2>/dev/null || true); \
	[ -n "$$files" ] || { echo "no protected critical tests to check"; exit 0; }; \
	if found=$$(grep -nEH "$(CRITICAL_SKIP_PATTERN)" $$files); then \
		echo "error: a protected critical test may never skip (R42):"; \
		echo "$$found"; \
		exit 1; \
	fi; \
	echo "protected critical tests hold no skip construct"

test-critical: critical-skip-check rs-test-critical ## Run protected critical tier (Rust + Python, zero skips, offline)
	@set -e; \
	stage="$(CRITICAL_STAGE)"; \
	ext="$$($(PYO3_BIN) -c 'import sysconfig; print(sysconfig.get_config_var("EXT_SUFFIX"))')"; \
	rm -rf "$$stage"; \
	mkdir -p "$$stage/blastit/_bin"; \
	cp python/blastit/*.py "$$stage/blastit/"; \
	echo "cargo build --locked -p blastit-py -p blastit-dbproxy"; \
	$(RUST_ENV) CARGO_NET_OFFLINE=1 $(CARGO) build --locked -p blastit-py -p blastit-dbproxy; \
	cp target/debug/lib_blastit.so "$$stage/blastit/_blastit$${ext}"; \
	cp target/debug/blastit-dbproxy "$$stage/blastit/_bin/blastit-dbproxy"; \
	echo "python -m unittest discover -s python/tests/critical"; \
	CARGO_NET_OFFLINE=1 UV_OFFLINE=1 PYTHONPATH="$${stage}:$(CURDIR)/python" \
		$(RUST_ENV) $(UV) run --no-project python -m unittest discover \
		-s python/tests/critical -p 'test_*.py'

docs-check: ## Build Zensical documentation site with --strict
	$(UVX) zensical build --strict

docs-test: ## Execute every console fence in README.md and the public docs pages
	scripts/build-wheel.sh
	$(UV) run --no-project python python/tests/runnable_docs/run.py

e2e: ## Build wheel and run Docker + rootless capability e2e tests
	scripts/build-wheel.sh
	E2E_DOCKERFILE=Dockerfile.prebuilt $(UV) run --no-project e2e/run.py
	$(UV) run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt

mutants-rust: ## Run cargo-mutants on Rust crates (pass PKG=<crate> to scope)
	@mkdir -p "$(MUTANTS_TMP)"
	@rm -rf mutants.out
	TMPDIR="$(MUTANTS_TMP)" $(RUST_ENV) $(CARGO) mutants $(if $(PKG),-p "$(PKG)",--workspace) -j $(MUTANTS_JOBS) --timeout $(MUTANTS_TIMEOUT)

check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-check docs-test plan-validate ## Full fast unprivileged check
