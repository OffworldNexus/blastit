# Blastit root Makefile: unified quality gates for Rust crates, Python tooling, docs, and the plan tree.
#
#   make help           # list targets
#   make check          # fast unprivileged repo gate: guard + critical-guard + rust + python tools + docs + plan
#   make clean          # format Rust and Python harness tools, then run lints
#   make docs-check     # build Zensical documentation site with --strict (zero broken links/anchors)
#   make docs-unit      # run the runnable-docs gate's own tests (offline, no wheel build)
#   make test-critical  # run protected critical tests only (Rust + Python)
#   make e2e            # build wheel and run Docker + rootless Docker e2e scenarios (gates)
#   make mutants-rust   # run cargo-mutants on critical crates (PKG=blastit-sqlpolicy), disk scratch + memory cap
#   make mutants-clean  # delete mutants.out and every cargo-mutants scratch copy, report bytes freed

UV ?= uv
UVX ?= uvx --offline
CARGO ?= cargo
# The one pinned dependency the runnable-docs gate needs (`docs-unit`, and the
# `blastit check` fences through the harness). Read from the uv cache, offline.
DOCS_PYTEST ?= 9.2.0

# Resolve real CPython >= 3.13 binary via uv (following symlinks so ../lib holds libpython3.X.so).
PYO3_BIN := $(shell readlink -f "$$($(UV) python find --no-project '>=3.13' 2>/dev/null || which python3)")
PYO3_LIBDIR := $(abspath $(dir $(PYO3_BIN))/../lib)
# Install prefix (parent of bin/): embedded interpreters (PyO3 test binaries) need it as PYTHONHOME
# at runtime, otherwise they cannot find the uv-managed stdlib ("No module named 'encodings'").
PYO3_PREFIX := $(if $(PYO3_BIN),$(abspath $(dir $(PYO3_BIN))/..),)
RUST_ENV = $(if $(PYO3_PREFIX),PYTHONHOME="$(PYO3_PREFIX)") PYO3_PYTHON="$(PYO3_BIN)" LD_LIBRARY_PATH="$(PYO3_LIBDIR)$${LD_LIBRARY_PATH:+:$$LD_LIBRARY_PATH}"

# `cargo-mutants` copies the whole workspace into "$TMPDIR/cargo-mutants-<crate>-XXXX.tmp"
# and builds every mutant inside that copy, so TMPDIR holds multi-GiB build trees per
# job. On this host /tmp is tmpfs (32 GiB of RAM), so the old /tmp default spent RAM
# instead of disk; default to the user cache on disk (override with e.g.
# MUTANTS_TMP=/var/tmp/blastit-mutants). `make -s print-mutants-tmp` prints it.
MUTANTS_TMP ?= $(or $(XDG_CACHE_HOME),$(HOME)/.cache)/blastit-mutants
MUTANTS_JOBS ?= 2
MUTANTS_TIMEOUT ?= 90
# Hard cgroup v2 cap for one campaign on this 48 GiB host. A 2-job campaign keeps a
# workspace copy + rustc/linker + the crate's test binaries per job: a measured 2-job
# `blastit-env` campaign (174 mutants) peaked at 2.9 GiB in its scope with a 2.2 GB
# scratch, and the policy crates are heavier, so 12 GiB is ~4x that while still leaving
# >30 GiB for the tmpfs /tmp (RAM), the 48 G repo `target/` page cache and a concurrent
# `make check`. MemorySwapMax=0 makes an overrun a clean OOM death, not thrashing.
MUTANTS_MEMORY_MAX ?= 12G
# Refuse to start below this much free space on the scratch filesystem: an interrupted
# campaign leaves multi-GiB copies behind (a measured 2-job `blastit-env` campaign held
# 2.2 GB), so 5 GiB is ~2x one campaign — the check exists to stop a start on a nearly
# full filesystem, not to size the campaign (`scripts/mutants-scratch.sh preflight`).
MUTANTS_MIN_FREE_GB ?= 5
# Retention for the start-up prune: keep the newest copies, and never prune a copy
# touched within MUTANTS_STALE_MIN minutes so a concurrent campaign keeps its scratch.
MUTANTS_KEEP_SCRATCH ?= 2
MUTANTS_STALE_MIN ?= 240

# Where `test-critical` stages the built package and extension, so `unittest`
# imports the real `blastit._blastit` without an install (R42, §7.1).
CRITICAL_STAGE ?= $(CURDIR)/target/python-critical
# The attribute/call a protected critical test may never contain. Comments never
# match because each alternative names the construct itself, not a bare word.
CRITICAL_SKIP_PATTERN := \#\[[[:space:]]*ignore|\bneeds_root[[:space:]]*\(|pytest\.mark\.skip|pytest\.skip[[:space:]]*\(|importorskip[[:space:]]*\(|unittest\.skip

.PHONY: help check clean guard critical-guard critical-skip-check plan-validate \
        rs-format rs-fmt rs-lint rs-doc rs-test rs-test-critical \
        py-format py-lint test-critical docs-check docs-unit docs-test e2e mutants-rust \
        mutants-clean mutants-scope-probe print-mutants-tmp

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

# `CARGO_NET_OFFLINE` has to be the string `true`, not `1`: cargo accepts both,
# but maturin >= 1.16 parses the variable as a strict bool when it shells out to
# cargo, and `make check` reaches it through `docs-test` -> `scripts/build-wheel.sh`
# (`docs/knowledge/LESSONS.md`).
test-critical: critical-skip-check rs-test-critical ## Run protected critical tier (Rust + Python, zero skips, offline)
	@set -e; \
	stage="$(CRITICAL_STAGE)"; \
	ext="$$($(PYO3_BIN) -c 'import sysconfig; print(sysconfig.get_config_var("EXT_SUFFIX"))')"; \
	rm -rf "$$stage"; \
	mkdir -p "$$stage/blastit/_bin"; \
	cp python/blastit/*.py "$$stage/blastit/"; \
	echo "cargo build --locked -p blastit-py -p blastit-dbproxy"; \
	$(RUST_ENV) CARGO_NET_OFFLINE=true $(CARGO) build --locked -p blastit-py -p blastit-dbproxy; \
	cp target/debug/lib_blastit.so "$$stage/blastit/_blastit$${ext}"; \
	cp target/debug/blastit-dbproxy "$$stage/blastit/_bin/blastit-dbproxy"; \
	echo "python -m unittest discover -s python/tests/critical"; \
	CARGO_NET_OFFLINE=true UV_OFFLINE=1 PYTHONPATH="$${stage}:$(CURDIR)/python" \
		$(RUST_ENV) $(UV) run --no-project python -m unittest discover \
		-s python/tests/critical -p 'test_*.py'

docs-check: ## Build Zensical documentation site with --strict
	$(UVX) zensical build --strict

docs-unit: ## Run the runnable-docs and release-hygiene tests (offline, no wheel build)
	UV_OFFLINE=1 $(UV) run --no-project --with pytest==$(DOCS_PYTEST) python -m pytest \
		python/tests/test_runnable_docs.py \
		python/tests/runnable_docs/test_harness.py \
		python/tests/test_docs_assets.py \
		python/tests/test_release_scripts.py -q

docs-test: docs-unit ## Execute every console fence in README.md and the public docs pages, then check the built wheel
	scripts/build-wheel.sh
	UV_OFFLINE=1 BLASTIT_REQUIRE_WHEEL=1 $(UV) run --no-project --with pytest==$(DOCS_PYTEST) python -m pytest -q \
		python/tests/test_release_scripts.py::test_built_wheel_long_description_has_absolute_links
	$(UV) run --no-project python python/tests/runnable_docs/run.py

e2e: ## Build wheel and run Docker + rootless capability e2e tests
	scripts/build-wheel.sh
	E2E_DOCKERFILE=Dockerfile.prebuilt $(UV) run --no-project e2e/run.py
	$(UV) run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt

mutants-rust: ## Run cargo-mutants with disk scratch + cgroup memory cap (PKG=<crate> to scope)
	@mkdir -p "$(MUTANTS_TMP)"
	MUTANTS_KEEP_SCRATCH="$(MUTANTS_KEEP_SCRATCH)" MUTANTS_STALE_MIN="$(MUTANTS_STALE_MIN)" scripts/mutants-scratch.sh prune "$(MUTANTS_TMP)"
	MUTANTS_MIN_FREE_GB="$(MUTANTS_MIN_FREE_GB)" scripts/mutants-scratch.sh preflight "$(MUTANTS_TMP)"
	$(RUST_ENV) TMPDIR="$(MUTANTS_TMP)" MUTANTS_JOBS="$(MUTANTS_JOBS)" MUTANTS_TIMEOUT="$(MUTANTS_TIMEOUT)" MUTANTS_MEMORY_MAX="$(MUTANTS_MEMORY_MAX)" CARGO="$(CARGO)" scripts/mutants-run.sh $(if $(PKG),-p "$(PKG)",--workspace)

mutants-clean: ## Delete mutants.out and every cargo-mutants scratch copy (report bytes freed)
	scripts/mutants-scratch.sh clean "$(MUTANTS_TMP)"

mutants-scope-probe: ## Show the cgroup and memory.max a cargo-mutants campaign would run under
	TMPDIR="$(MUTANTS_TMP)" scripts/mutants-run.sh --probe --memory-max "$(MUTANTS_MEMORY_MAX)"

print-mutants-tmp: ## Print the mutants scratch directory (disk, never /tmp)
	@echo "$(MUTANTS_TMP)"

check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test plan-validate ## Full fast unprivileged check
