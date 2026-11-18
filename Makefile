# Blastit root Makefile: unified quality gates for Rust crates, Python tooling, docs, and the plan tree.
#
#   make help           # list targets
#   make check          # fast unprivileged repo gate: guard + critical-guard + rust + python tools + docs + plan
#   make clean          # format Rust and Python harness tools, then run lints
#   make docs-check     # build Zensical documentation site with --strict (zero broken links/anchors)
#   make docs-unit      # run the runnable-docs gate's own tests (offline, no wheel build)
#   make docs-serve     # build the site and serve it through a freshly built `blastit dev` (PORT=8080)
#   make docs-serve-check # bounded smoke test of that serve stack (binds a port; not part of `check`)
#   make test-critical  # run protected critical tests only (Rust + Python)
#   make e2e            # build wheel and run Docker + rootless Docker e2e scenarios (gates)
#   make php-sapi       # build the per-minor C SAPI shims this host has headers for
#   make php-sapi-e2e   # Docker: build one shim and prove it with the real-load probe
#   make rs-test-php    # real-pair PHP legs: critical_php_backend + critical_php_embed
#                       # + critical_parallel_views
#   make rs-test-php-image # those legs inside the php-sapi-e2e image (what CI runs)
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
# A **standalone** build links no `libpython`, so it is not handed an interpreter
# either: `crates/blastit/build.rs` now gates its `-Wl,-rpath,<prefix>/lib` on the
# `python` feature (`MB.17`), and this is belt and braces — a `PYO3_PYTHON`
# exported in a developer's shell cannot take part in the artifact's build.
STANDALONE_ENV = env -u PYO3_PYTHON -u PYTHONHOME

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
# `blastit`'s default build is the standalone artifact (`MB.17`): with `python` off,
# every decorated role — the WSGI views, the immortalisation pass, the `learn` path —
# is compiled out of the mutant build, so its mutants are generated but no test ever
# exercises them, and the campaign reports them as survivors: a green-looking number
# that covers half the crate. The capability therefore follows `PKG`.
#
# `--features python`, not `--all-features`: that also turns on
# `blastit-py/extension-module`, whose test binaries cannot link (the interpreter
# supplies those symbols). Not `blastit/python` either: cargo refuses a
# package-qualified feature for a graph that does not contain the package, which is
# why a workspace campaign is split below instead.
MUTANTS_FEATURE_ARGS := $(if $(filter blastit,$(PKG)),--features python)

# Dogfood the static backend: our own documentation site, built by Zensical and
# served by a freshly built `blastit dev` through the real `[route.static]` path
# (resolver, ETag/304, refusal matrix) — not a special-case file server. The
# stage lives under target/ (gitignored), so the manifest sits beside the files
# it serves and `root` never needs a `..`. `docs-serve-check` binds a TCP port
# and builds the standalone binary, so it is a developer target, not part of
# `check`.
PORT ?= 8080
DOCS_SERVE_STAGE ?= $(CURDIR)/target/docs-serve
DOCS_SERVE_MANIFEST := $(DOCS_SERVE_STAGE)/Blastfile
DOCS_SERVE_BIN := $(CURDIR)/target/debug/blastit
# `docs-serve-check` never assumes 8080 is free: it probes upwards from here.
DOCS_SERVE_CHECK_PORT_BASE ?= 8765

# `docs-serve` binds PORT, so validate it at parse time — but only when that goal
# is actually requested, and before any prerequisite runs: a bad value must not
# cost a zensical build and a cargo build first. (`docs-serve-check` picks its own
# port, so a stray PORT in the environment cannot break it.) No `case` here: a
# bare `)` inside `$(shell …)` closes the call at parse time.
ifeq ($(filter docs-serve,$(MAKECMDGOALS)),docs-serve)
DOCS_SERVE_PORT_PROBLEM := $(shell p='$(PORT)'; \
	if ! printf '%s' "$$p" | grep -qE '^[0-9]+$$'; then echo 'is not a port number'; \
	elif [ "$$p" -lt 1 ] || [ "$$p" -gt 65535 ]; then echo 'must be within 1..65535'; \
	fi)
ifneq ($(DOCS_SERVE_PORT_PROBLEM),)
$(error PORT $(DOCS_SERVE_PORT_PROBLEM): e.g. `make docs-serve PORT=8080` (got PORT='$(PORT)'))
endif
endif

# Where `test-critical` stages the built package and extension, so `unittest`
# imports the real `blastit._blastit` without an install (R42, §7.1).
CRITICAL_STAGE ?= $(CURDIR)/target/python-critical
# The attribute/call a protected critical test may never contain. Comments never
# match because each alternative names the construct itself, not a bare word.
CRITICAL_SKIP_PATTERN := \#\[[[:space:]]*ignore|\bneeds_root[[:space:]]*\(|pytest\.mark\.skip|pytest\.skip[[:space:]]*\(|importorskip[[:space:]]*\(|unittest\.skip

.PHONY: help check clean guard critical-guard critical-skip-check plan-validate \
        rs-format rs-fmt rs-lint rs-doc rs-test rs-test-critical rs-no-python seam-check \
        no-inotify \
        py-format py-lint test-critical docs-check docs-unit docs-test e2e mutants-rust \
        docs-serve docs-serve-stage docs-serve-check php-sapi php-sapi-e2e \
        rs-test-php rs-test-php-image platform-matrix-check standalone standalone-glibc workflow-check \
        packaging-gates-test doc-tone manifest-purity zed-check lsp-portable mutants-clean \
        mutants-scope-probe print-mutants-tmp

help: ## Show available Make targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-18s\033[0m %s\n", $$1, $$2}'

clean: rs-format py-format rs-lint py-lint ## Format Rust + Python tools, then lint

guard: ## Refuse >5 MiB files, .env secrets, or stray binary artefacts
	$(UV) run --no-project tools/guard.py

critical-guard: ## Protect critical tests (report by default; pass BASE=<rev> NODE=<id> to enforce)
	$(UV) run --no-project tools/critical_guard.py --base $(or $(BASE),HEAD) $(if $(NODE),--node $(NODE))

plan-validate: ## Validate plan/nodes consistency
	$(UV) run --no-project tools/plan.py validate

# The packaging matrix (`MB.3.18`): the one place a wheel/archive/shim leg is
# declared, and the guard on what a published page may claim. `ADR-0010` §4 is
# the table of record for what an artifact *contains*; `scripts/platform_matrix.py`
# is the table of record for which targets are attempted, and for how many shims
# are built *and* probed — the proven-count rule (4 until 8.2/8.3 existed, now
# whatever `PROVEN` names) is enforced here rather than in prose, so a page
# cannot promise a leg the matrix does not build.
platform-matrix-check: ## Refuse a packaging matrix a published page over-claims
	$(UV) run --no-project scripts/platform_matrix.py --check

# The workflows as a graph (`MB.3.18`): `scripts/check_workflows.py` reads the
# YAML (`pyyaml==6.0.3` pinned and hash-locked in `scripts/check_workflows.py.lock`,
# executed offline with `UV_OFFLINE=1 --locked`) and `scripts/test_packaging_gates.py`
# runs mutated negative self-tests for the workflow, matrix, and wheel-tag guards.
workflow-check: ## Refuse a workflow whose job graph cannot run (offline, locked)
	UV_OFFLINE=1 $(UV) run --no-project --locked scripts/check_workflows.py

packaging-gates-test: ## Run positive + negative self-tests for workflow, matrix, and wheel-tag guards
	UV_OFFLINE=1 $(UV) run --no-project scripts/test_packaging_gates.py

# The tone rule (`MB.3.65` part B): no apology language on a page a user reads.
# The list is narrow and literal (`scripts/check_doc_tone.py`), and its negative
# self-tests run with the rest of `docs-unit`'s pytest files.
doc-tone: ## Refuse apology language in the user-facing docs
	$(UV) run --no-project scripts/check_doc_tone.py

# The manifest crate never reads the environment (`MB.21.3.1`, R20/R24): a
# `Blastfile` names variables and the value is resolved by `blastit-env` at
# boot. The guard is literal and its negative self-tests run with it, so a moved
# directory or an emptied needle fails here instead of passing silently.
manifest-purity: ## Refuse std::env under crates/blastit-manifest/src (R20/R24)
	$(UV) run --no-project scripts/check_manifest_purity.py
	$(UV) run --no-project scripts/test_manifest_purity.py

# `editors/**` is the one tree `tools/guard.py` does not model (it bans wheels,
# binaries and >5 MiB files): an extension that is present but cannot install —
# a branch-shaped grammar `rev`, a capture outside §10.1's fifteen names, a
# committed `extension.wasm` — is a registry-wide failure. The guard is static,
# stdlib and offline, and its negative self-tests run beside it, so a property
# silently deleted from the guard fails here (design §12.4, MB.22.5).
zed-check: ## Static checks for editors/** (Zed extension + tree-sitter grammar)
	$(UV) run --no-project scripts/check_zed_extension.py
	UV_OFFLINE=1 $(UV) run --no-project scripts/test_check_zed_extension.py

# The portable server's one invariant (`MB.35.6`, design §1.3). `blastit-lsp` is
# the only artifact shipped for macOS and Windows, so its closure is
# `blastit-manifest` + `serde_json` and its own code is `std` plus those two. The
# guard reads the crate's manifest (every dependency table, not `[dependencies]`
# alone), every file under its `src/`, `cargo metadata`'s unfiltered normal
# closure and this Makefile; its negative twins mutate throwaway copies, so a
# promise deleted from the guard fails here instead of passing silently.
lsp-portable: ## Refuse a blastit-lsp dependency, source line or wiring that is not portable
	$(UV) run --no-project scripts/check_lsp_portable.py
	UV_OFFLINE=1 $(UV) run --no-project scripts/test_check_lsp_portable.py

rs-format: ## Format Rust workspace
	$(CARGO) fmt --all

rs-fmt: ## Check Rust workspace formatting
	$(CARGO) fmt --all -- --check

rs-lint: ## Clippy workspace with warnings denied (python capability on, like the wheel)
	$(RUST_ENV) $(CARGO) clippy --workspace --all-targets --locked --features python -- -D warnings

rs-doc: ## Check rustdoc across workspace with warnings denied
	$(RUST_ENV) RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --locked --features python

rs-test: ## Run all unprivileged Rust unit, integration, and doc tests (python capability on)
	$(RUST_ENV) $(CARGO) test --workspace --all-targets --locked --features python
	$(RUST_ENV) $(CARGO) test --workspace --doc --locked --features python

# The default `cargo build -p blastit` **is** the standalone artifact (`MB.17`,
# ADR-0010 §4): `blastit`'s default features are `php`, which needs nothing at
# link time, while `python` links `libpython` and a `RUNPATH` into the uv
# interpreter cache of whoever built it. This target is that build's guard — it
# compiles the default, inspects the ELF (`readelf`: no `libpython`, no
# `RPATH`/`RUNPATH`), and runs the suites that need no interpreter. The
# interpreter-bound suites belong to the python configuration, which `rs-test`
# builds; `make check` runs both, so neither can silently disappear.
# `cargo tree -e normal` excludes dev- and build-dependencies, which is exactly
# "what the binary links".
rs-no-python: ## Build + inspect the default (standalone) binary: no libpython, no RUNPATH
	@echo "cargo build -p blastit --locked            # the default build is the standalone artifact"
	$(STANDALONE_ENV) $(CARGO) build -p blastit --locked
	scripts/check-portable-elf.sh "$(CURDIR)/target/debug/blastit"
	@echo "scripts/check-no-inotify.sh                # the only watcher is fanotify (ADR-0014)"
	scripts/check-no-inotify.sh "$(CURDIR)/target/debug/blastit"
	scripts/check-no-inotify.sh --self-test
	@echo "cargo check -p blastit-view --no-default-features"
	$(CARGO) check -p blastit-view --no-default-features --locked
	@echo "cargo test  -p blastit --lib --locked      # the default configuration"
	$(STANDALONE_ENV) $(CARGO) test -p blastit --locked --lib
	@echo "cargo test  -p blastit --test no_python_build"
	$(STANDALONE_ENV) $(CARGO) test -p blastit --locked --test no_python_build
	$(CARGO) test -p blastit-view --no-default-features --locked --lib
	@echo "clippy      -p blastit{,-view} (standalone)"
	$(STANDALONE_ENV) $(CARGO) clippy -p blastit --all-targets --locked -- -D warnings
	$(CARGO) clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
	@count=$$($(CARGO) tree -p blastit -e normal \
		| grep -c pyo3 || true); \
	if [ "$$count" -ne 0 ]; then \
		echo "error: the default build depends on pyo3 ($$count): it would link libpython" >&2; \
		exit 1; \
	fi; \
	echo "default build: 0 pyo3 dependencies (no libpython linked)"

# The one watcher (`MB.23.3`, ADR-0014): `inotify` is banned outright — not as a
# fallback, not behind a flag, not in a dependency — and `scripts/check-no-inotify.sh`
# is the mechanical proof, in three checks (dynamic symbols in a named ELF, the
# default build's dependency graph, and every tracked **and untracked** source
# file, including the group syscall and the watcher constructor outside their one
# module). It builds the binary first so `make check` never scans a missing file,
# and its `--self-test` plants a call in a scratch copy and in an untracked file,
# a second constructor call, a symbol in a fake `readelf` dump and a crate in a
# fake `cargo tree` dump, so a guard that cannot fail is itself a red gate.
# `make check` runs both lines through `rs-no-python`, which builds the binary.
no-inotify: ## Refuse an inotify symbol, crate or call site; prove the guard can fail
	@if [ ! -x "$(CURDIR)/target/debug/blastit" ]; then \
		echo "cargo build -p blastit --locked                    # the binary the symbol check reads"; \
		$(STANDALONE_ENV) $(CARGO) build -p blastit --locked; \
	fi
	scripts/check-no-inotify.sh "$(CURDIR)/target/debug/blastit"
	scripts/check-no-inotify.sh --self-test

# The test-only boot seam (`MB.3.64`) is a debug build's: `#[cfg(debug_assertions)]`
# compiles the rendezvous — and the variable's name — out of a release build, and
# `BLASTIT_TEST_SWAP_POINT` in a shipped binary would pause a boot before the
# listener binds, which is a denial-of-service surface. `scripts/check-no-test-seam.sh`
# measures both directions on real files, because either alone can pass vacuously:
# `--present` on the debug standalone binary `rs-no-python` just built (so a renamed
# variable, or a seam deleted from the source, cannot look green), and the absent
# direction on a release artifact when this checkout has one. The absent direction
# that governs a *shipped* file is unconditional in `scripts/build-standalone.sh`,
# which every archive goes through.
seam-check: ## Prove the test-only boot seam is in the debug build and not in a release one
	@if [ ! -x "$(CURDIR)/target/debug/blastit" ]; then \
		echo "cargo build -p blastit --locked                    # the debug binary to compare"; \
		$(STANDALONE_ENV) $(CARGO) build -p blastit --locked; \
	fi
	scripts/check-no-test-seam.sh --present-all "$(CURDIR)/target/debug/blastit"
	@if [ -x "$(CURDIR)/target/release/blastit" ]; then \
		scripts/check-no-test-seam.sh "$(CURDIR)/target/release/blastit"; \
	else \
		echo "note: no target/release/blastit in this checkout; the shipped artifact's absence"; \
		echo "      is checked by scripts/build-standalone.sh, which packages it"; \
	fi

# The standalone archive (`ADR-0010` §4, `MB.3.18`): the release binary with
# `python` off, the shims this host has built for its own tuple, a LICENSE and a
# checksum file — `scripts/build-standalone.sh`. Locally it packages whatever
# `make php-sapi` left behind, so the archive is inspectable before a tag; the
# release pipeline builds it in an image instead (see `standalone-glibc`), and it
# reads that host's glibc floor from the finished binary either way.
standalone: ## Package the standalone archive (release binary + proven shims + checksum)
	scripts/build-standalone.sh

# The shipped way: the archive is built inside the image whose glibc it claims,
# so the floor is a property of the build and not of this machine. `make
# standalone` above keeps the host build for a quick local look — its README
# says which host it came from, and `--max-glibc` is what makes the shipped one
# refuse a binary that outgrew the claim.
STANDALONE_ARCH ?= $(shell uname -m)
STANDALONE_TARGET ?= $(STANDALONE_ARCH)-unknown-linux-gnu
STANDALONE_BUILDER ?= quay.io/pypa/manylinux_2_28_$(STANDALONE_ARCH)

standalone-glibc: ## Build the gnu archive inside the manylinux_2_28 image (as CI ships it)
	scripts/build-standalone-in-image.sh \
		--image "$(STANDALONE_BUILDER)" \
		--target "$(STANDALONE_TARGET)" \
		--max-glibc 2_28

rs-test-critical: ## Run every crates/*/tests/critical_*.rs suite
	@set -e; found=0; for file in crates/*/tests/critical_*.rs; do \
		[ -e "$$file" ] || continue; found=1; \
		crate=$$(basename $$(dirname $$(dirname "$$file"))); \
		name=$$(basename "$$file" .rs); \
		features=""; if [ "$$crate" = "blastit" ]; then features="--features python"; fi; \
		echo "cargo test -p $$crate $$features --test $$name"; \
		$(RUST_ENV) $(CARGO) test -p "$$crate" $$features --test "$$name" --locked; \
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
	echo "cargo build --locked -p blastit-py -p blastit-dbproxy --features blastit/python"; \
	$(RUST_ENV) CARGO_NET_OFFLINE=true $(CARGO) build --locked -p blastit-py -p blastit-dbproxy --features blastit/python; \
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
		python/tests/test_release_scripts.py \
		scripts/test_doc_tone.py -q

# `docs-test` builds its host-native wheel into a scratch directory under
# `target/docs-wheel` (`-o`), stashes any pre-existing release `dist/` aside for
# the two readers that hardcode `REPO / "dist"`, and restores `dist/` on every
# exit path — so `make check` never deletes a `manylinux_2_28` wheel in `dist/`
# or leaves a host `manylinux_2_39` wheel behind.
DOCS_WHEEL_STAGE ?= $(CURDIR)/target/docs-wheel
docs-test: SHELL := /bin/bash
docs-test: docs-unit ## Execute every console fence in README.md and the public docs pages, then check the built wheel
	scripts/build-wheel.sh -o "$(DOCS_WHEEL_STAGE)"
	@set -euo pipefail; \
	saved=""; had_dist=0; \
	if [ -d "$(CURDIR)/dist" ]; then \
		had_dist=1; \
		saved=$$(mktemp -d "$${TMPDIR:-/var/tmp}/blastit-saved-dist.XXXXXX"); \
		mv "$(CURDIR)/dist" "$$saved/dist"; \
	fi; \
	restore_dist() { \
		rm -rf "$(CURDIR)/dist"; \
		if [ "$$had_dist" -eq 1 ] && [ -d "$$saved/dist" ]; then \
			mv "$$saved/dist" "$(CURDIR)/dist"; \
		fi; \
		[ -n "$$saved" ] && rm -rf "$$saved" || true; \
	}; \
	trap restore_dist EXIT; \
	mkdir -p "$(CURDIR)/dist"; \
	cp "$(DOCS_WHEEL_STAGE)"/blastit-*.whl "$(CURDIR)/dist/"; \
	UV_OFFLINE=1 BLASTIT_REQUIRE_WHEEL=1 $(UV) run --no-project --with pytest==$(DOCS_PYTEST) python -m pytest -q \
		python/tests/test_release_scripts.py::test_built_wheel_long_description_has_absolute_links; \
	$(UV) run --no-project python python/tests/runnable_docs/run.py

# Build the site, then stage exactly what blastit serves: the files under
# `site/` and a generated manifest beside them. Non-strict on purpose —
# `docs-check` owns `--strict`; a preview should still show a page whose
# neighbour has a broken link.
# Dogfooding builds the **standalone** artifact (`MB.17`): a plain
# `cargo build -p blastit` carries `php` and links no interpreter, and the
# `readelf` check below keeps this preview from reintroducing a dependency on a
# dev machine's libpython.
docs-serve-stage:
	@set -eu; \
	echo "zensical build   -> site/"; \
	$(UVX) zensical build; \
	rm -rf "$(DOCS_SERVE_STAGE)"; \
	mkdir -p "$(DOCS_SERVE_STAGE)/site"; \
	cp -R site/. "$(DOCS_SERVE_STAGE)/site/"; \
	printf '%s\n' \
		'# Generated by `make docs-serve`; blastit serves the built site from here.' \
		'static "/" {' \
		'    root "site"' \
		'}' \
		> "$(DOCS_SERVE_MANIFEST)"; \
	echo "cargo build      -> $(DOCS_SERVE_BIN)"; \
	$(STANDALONE_ENV) $(CARGO) build -p blastit --locked; \
	echo "portable check   -> $(DOCS_SERVE_BIN)"; \
	scripts/check-portable-elf.sh "$(DOCS_SERVE_BIN)"

docs-serve: docs-serve-stage ## Build the docs site and serve it through `blastit dev` (PORT=8080)
	@echo ""
	@echo "  Docs served by blastit itself (single-uid dev, 127.0.0.1:$(PORT))."
	@echo "  URL:  http://127.0.0.1:$(PORT)/"
	@echo "  Stop: Ctrl-C."
	@echo ""
	BLASTIT_MANIFEST="$(DOCS_SERVE_MANIFEST)" "$(DOCS_SERVE_BIN)" dev --port "$(PORT)"

docs-serve-check: SHELL := /bin/bash
docs-serve-check: docs-serve-stage ## Bounded smoke test of the docs serve stack (binds a port; not part of `check`)
	@set -eu; \
	log="$(DOCS_SERVE_STAGE)/check.log"; \
	port=""; p="$(DOCS_SERVE_CHECK_PORT_BASE)"; end=$$((p + 50)); \
	while [ "$$p" -le "$$end" ]; do \
		if ! (exec 3<>"/dev/tcp/127.0.0.1/$$p") 2>/dev/null; then port="$$p"; break; fi; \
		p=$$((p + 1)); \
	done; \
	[ -n "$$port" ] || { echo "error: no free port in $(DOCS_SERVE_CHECK_PORT_BASE)..$$end" >&2; exit 1; }; \
	pid=""; \
	cleanup() { \
		rc=$$?; set +eu; \
		if [ -n "$$pid" ]; then \
			kill -TERM "$$pid" 2>/dev/null || true; \
			i=0; \
			while [ "$$i" -lt 50 ] && kill -0 "$$pid" 2>/dev/null; do sleep 0.1; i=$$((i + 1)); done; \
			if kill -0 "$$pid" 2>/dev/null; then \
				echo "warning: blastit dev ignored SIGTERM for 5 s; sending SIGKILL" >&2; \
				kill -KILL "$$pid" 2>/dev/null || true; \
				sleep 0.2; \
			fi; \
			wait "$$pid" 2>/dev/null || true; \
			i=0; \
			while [ "$$i" -lt 20 ]; do \
				if (exec 3<>"/dev/tcp/127.0.0.1/$$port") 2>/dev/null; then sleep 0.1; i=$$((i + 1)); else break; fi; \
			done; \
			if (exec 3<>"/dev/tcp/127.0.0.1/$$port") 2>/dev/null; then \
				echo "error: 127.0.0.1:$$port still answers after teardown" >&2; rc=1; \
			fi; \
			leftovers="$$(pgrep -f -- "blastit-dev-[0-9]*-$$pid-" 2>/dev/null | tr '\n' ' ' || true)"; \
			if [ -n "$$leftovers" ]; then \
				echo "error: this run left processes behind:$$leftovers" >&2; \
				ps -o pid=,ppid=,args= -p "$$(echo $$leftovers | tr ' ' ',')" >&2 || true; \
				rc=1; \
			fi; \
		fi; \
		exit "$$rc"; \
	}; \
	trap cleanup EXIT; \
	trap 'exit 130' INT; \
	trap 'exit 143' TERM; \
	echo "docs-serve-check: serving $(DOCS_SERVE_STAGE)/site on 127.0.0.1:$$port"; \
	env -u PORT BLASTIT_MANIFEST="$(DOCS_SERVE_MANIFEST)" "$(DOCS_SERVE_BIN)" dev --port "$$port" >"$$log" 2>&1 & \
	pid=$$!; \
	cmd="[exe]"; i=0; \
	while [ "$$cmd" = "[exe]" ] && [ "$$i" -lt 20 ]; do \
		cmd="$$(ps -o args= -p "$$pid" 2>/dev/null | cut -c1-96)"; \
		if [ "$$cmd" = "[exe]" ]; then sleep 0.05; fi; \
		i=$$((i + 1)); \
	done; \
	echo "docs-serve-check: pid $$pid ($$cmd)"; \
	waited=0; \
	while ! (exec 3<>"/dev/tcp/127.0.0.1/$$port") 2>/dev/null; do \
		if ! kill -0 "$$pid" 2>/dev/null; then \
			echo "error: blastit dev exited before the port answered" >&2; cat "$$log" >&2; exit 1; \
		fi; \
		waited=$$((waited + 1)); \
		if [ "$$waited" -ge 60 ]; then \
			echo "error: 127.0.0.1:$$port did not answer within 15 s" >&2; cat "$$log" >&2; exit 1; \
		fi; \
		sleep 0.25; \
	done; \
	request() { \
		exec 3<>"/dev/tcp/127.0.0.1/$$port" || { echo "error: cannot reach 127.0.0.1:$$port for $$1 $$2" >&2; return 1; }; \
		printf '%s %s HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n' "$$1" "$$2" >&3; \
		rc=0; \
		timeout 10 cat <&3 || rc=$$?; \
		exec 3<&-; exec 3>&-; \
		return "$$rc"; \
	}; \
	status_of() { printf '%s' "$$1" | tr -d '\r' | awk 'NR == 1 { print $$2 }'; }; \
	header_of() { printf '%s' "$$2" | tr -d '\r' | tr 'A-Z' 'a-z' | awk -v n="$$1" 'index($$0, n ":") == 1 { sub(/^[^:]*:[[:space:]]*/, ""); print; exit }'; }; \
	fail=0; \
	check() { \
		out=""; rc=0; \
		out="$$(request "$$2" "$$3")" || rc=$$?; \
		if [ "$$rc" -eq 124 ]; then \
			printf 'FAIL %-30s no complete response within 10 s\n' "$$1" >&2; \
			printf '%s\n' "$$out" >&2; \
			fail=1; \
		elif [ "$$rc" -ne 0 ]; then \
			printf 'FAIL %-30s request failed (rc=%s)\n' "$$1" "$$rc" >&2; \
			printf '%s\n' "$$out" >&2; \
			fail=1; \
		else \
			st="$$(status_of "$$out")"; \
			ct="$$(header_of content-type "$$out")"; \
			cc="$$(header_of cache-control "$$out")"; \
			ok=1; \
			[ "$$st" = "$$4" ] || ok=0; \
			if [ -n "$${5:-}" ]; then case "$$ct" in *"$${5:-}"*) ;; *) ok=0;; esac; fi; \
			if [ -n "$${6:-}" ]; then [ "$$cc" = "$${6:-}" ] || ok=0; fi; \
			if [ "$$ok" -eq 1 ]; then \
				printf 'ok   %-30s %s  %s  %s\n' "$$1" "$$st" "$$ct" "$$cc"; \
			else \
				printf 'FAIL %-30s wanted %s %s%s\n' "$$1" "$$4" "$${5:-}" "$${6:-}" >&2; \
				printf '%s\n' "$$out" >&2; \
				fail=1; \
			fi; \
		fi; \
	}; \
	asset="$$(cd "$(DOCS_SERVE_STAGE)/site" && ls assets/javascripts/bundle.*.min.js 2>/dev/null | head -n 1 || true)"; \
	[ -n "$$asset" ] || { echo "error: no fingerprinted asset under site/assets/javascripts (did zensical build?)" >&2; exit 1; }; \
	check "/ (index)" GET / 200 text/html no-cache; \
	check "/reference/cli/" GET /reference/cli/ 200 text/html no-cache; \
	check "missing path (not the index)" GET /mb14-does-not-exist 404 text/plain; \
	check "asset /$$asset" HEAD "/$$asset" 200 text/javascript no-cache; \
	if [ "$$fail" -ne 0 ]; then \
		echo "" >&2; echo "--- $$log (tail) ---" >&2; tail -n 40 "$$log" >&2; exit 1; \
	fi; \
	echo "docs-serve-check: 4/4 checks passed (server log: $$log)"

e2e: ## Build manylinux_2_28 wheel and run Docker + rootless capability e2e tests
	scripts/build-wheel.sh --manylinux 2_28
	E2E_DOCKERFILE=Dockerfile.prebuilt $(UV) run --no-project e2e/run.py
	$(UV) run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt

# Per-minor PHP SAPI shims (MB.3.12, `crates/blastit-php-sapi`). A host without
# PHP headers skips a minor with one line naming the package, so `make check`
# stays green on a PHP-free machine: the shim is *built* here, and only
# `php-sapi-e2e` (Docker + Debian's `libphp8.4-embed`) *proves* that it loads,
# serves a request, contains an `exit()`, and refuses the four reserved CGI
# parameter names.
#
# One artifact, one name (ADR-0010 §4.1): the file is
# `libblastit_php<major><minor>-<arch>-<libc>.so` and its `DT_SONAME` is that
# same string, because the boot selects exactly that name from its compile-time
# `(target_arch, target_env)` — `Minor::shim_file()` in
# `crates/blastit-php-sapi/src/toolchain.rs` and `stage0`'s `shim_name()`.
#
# `<arch>`/`<libc>` are read from the compiler that builds the shim
# (`$(PHP_SAPI_CC) -dumpmachine`, e.g. `x86_64-linux-gnu`,
# `aarch64-alpine-linux-musl`), never from the filesystem: a glibc host that has
# Debian's musl package installed has a musl *loader* while `cc`/`php-config`
# still build glibc, and the gnu boot could never select the `-musl` name such a
# probe would produce. `PHP_SAPI_ARCH`/`PHP_SAPI_LIBC` override the tuple for a
# cross build; an overlay of the wrong libc is refused rather than shipped.
#
# The PHP headers are passed as `-isystem`, not `-I`: PHP 8.5's
# `Zend/zend_operators.h` declares a `static inline` with an unused parameter,
# which `-Wextra -Werror` (kept on for *our* file) would reject as a build error
# in a header we do not own. System-header treatment suppresses their warnings
# without lowering the bar for `c/blastit_php_sapi.c`.
PHP_MINORS ?= 8.5 8.4 8.3
PHP_SAPI_CRATE := $(CURDIR)/crates/blastit-php-sapi
PHP_SAPI_OUT ?= $(CURDIR)/target/php-sapi
PHP_SAPI_CC ?= $(CC)
# Empty means "ask `$(PHP_SAPI_CC) -dumpmachine`". Set both to cross-build.
# `PHP_SAPI_LIBC` doubles as the *leg selector* for `php-sapi-e2e`: the matrix has
# one shim leg per (minor, libc), and the probe image has to be the distro that
# carries that libc (`debian:trixie` vs `alpine:3.21`), so
# `make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_LIBC=musl` is the Alpine leg.
PHP_SAPI_ARCH ?=
PHP_SAPI_LIBC ?=
# Keep the e2e image after the probe (`php_sapi_probe.py --keep`), because
# `rs-test-php-image` runs the protected PHP suites against its pair (`MB.3.39`):
# it is the one environment with both the NTS `libphp8.4.so` and the shim.
PHP_SAPI_KEEP ?=

php-sapi: ## Build the C SAPI shims whose php-config exists; name the absent ones
	@set -eu; \
	cc="$(PHP_SAPI_CC)"; \
	machine="$$("$$cc" -dumpmachine 2>/dev/null || true)"; \
	arch="$(PHP_SAPI_ARCH)"; \
	libc="$(PHP_SAPI_LIBC)"; \
	if [ -z "$$machine" ] && [ -z "$$arch" ]; then \
		echo "php-sapi: '$$cc' does not answer -dumpmachine; set PHP_SAPI_ARCH and PHP_SAPI_LIBC" >&2; exit 1; \
	fi; \
	if [ -z "$$arch" ]; then arch="$${machine%%-*}"; fi; \
	if [ -z "$$libc" ]; then \
		case "$$machine" in \
			*musl*) libc=musl ;; \
			*gnu*) libc=gnu ;; \
			*) echo "php-sapi: cannot tell the libc of '$$cc' ($$machine); set PHP_SAPI_LIBC=gnu|musl" >&2; exit 1 ;; \
		esac; \
	fi; \
	case "$$arch" in \
		x86_64|aarch64) ;; \
		*) echo "php-sapi: unsupported architecture '$$arch' (supported: x86_64, aarch64)" >&2; exit 1 ;; \
	esac; \
	case "$$libc" in \
		gnu|musl) ;; \
		*) echo "php-sapi: unsupported libc '$$libc' (supported: gnu, musl)" >&2; exit 1 ;; \
	esac; \
	if [ "$$libc" = musl ] && [ -z "$(PHP_SAPI_LIBC)" ]; then \
		musl_loader=0; \
		for loader in /lib/ld-musl-*.so.1; do [ -e "$$loader" ] && musl_loader=1; done; \
		if [ "$$musl_loader" -eq 0 ]; then \
			echo "php-sapi: '$$cc' targets musl but no /lib/ld-musl-*.so.1 is installed, so this host could not load the shim; set PHP_SAPI_LIBC=musl only for a cross artifact" >&2; exit 1; \
		fi; \
	fi; \
	mkdir -p "$(PHP_SAPI_OUT)"; \
	built=0; skipped=""; \
	explicit="$(PHP_CONFIG)"; \
	for minor in $(PHP_MINORS); do \
		digits="$$(printf '%s' "$$minor" | tr -d '.')"; \
		cfg=""; \
		if [ -n "$$explicit" ] && [ "$$("$$explicit" --version 2>/dev/null | cut -d. -f1-2)" = "$$minor" ]; then \
			cfg="$$explicit"; \
		fi; \
		if [ -z "$$cfg" ]; then \
			for candidate in "php-config$$minor" "php-config$$digits" php-config; do \
				path="$$(command -v "$$candidate" 2>/dev/null || true)"; \
				[ -n "$$path" ] || continue; \
				if [ "$$("$$path" --version 2>/dev/null | cut -d. -f1-2)" = "$$minor" ]; then cfg="$$path"; break; fi; \
			done; \
		fi; \
		soname="libblastit_php$$digits-$$arch-$$libc.so"; \
		out="$(PHP_SAPI_OUT)/$$soname"; \
		if [ -z "$$cfg" ]; then \
			echo "php-sapi: skipped $$minor: no php-config for PHP $$minor (Debian/Ubuntu: apt install php$$minor-dev libphp$$minor-embed; Alpine: php$$digits-dev php$$digits-embed)"; \
			skipped="$$skipped $$minor"; \
			continue; \
		fi; \
		echo "php-sapi: cc -> $$out ($$cfg reports $$("$$cfg" --version))"; \
		"$(PHP_SAPI_CC)" -O2 -D_GNU_SOURCE -fPIC -shared -Wall -Wextra -Werror \
			-Wl,-soname,"$$soname" \
			-I"$(PHP_SAPI_CRATE)/c/include" \
			$$("$$cfg" --includes | sed 's/-I/-isystem /g') \
			-o "$$out" "$(PHP_SAPI_CRATE)/c/blastit_php_sapi.c"; \
		built=$$((built + 1)); \
	done; \
	echo "php-sapi: $$built shim(s) for $$arch-$$libc in $(PHP_SAPI_OUT); skipped:$${skipped:- none}"

php-sapi-e2e: ## Docker: build one shim and prove it with the real-load probe (not in `check`)
	$(UV) run --no-project e2e/php_sapi_probe.py \
		$(if $(PHP_MINOR),--minor $(PHP_MINOR)) \
		$(if $(PHP_SAPI_LIBC),--libc $(PHP_SAPI_LIBC)) \
		--image "$(PHP_SAPI_IMAGE)" \
		$(if $(PHP_SAPI_KEEP),--keep)

# The real-pair half of the protected PHP suites (nodes MB.3.39, MB.24.4,
# MB.23.4.2).
#
# `make check` runs all of them on a host with **no** PHP, so every CI run of it
# exercises their refusal branch only (`refused … install the PHP embed
# package`). `rs-test-php` runs the same suites against a real NTS `libphp`
# plus the shim built for it — the `[route.php]` page answering `200` through
# `view.chan` with `cpu_ns`, the `exit()`/fatal containment and the slot
# respawn, the maps leg, and `critical_parallel_views`' wall-clock promise (four
# concurrent 200 ms requests to one PHP route under 450 ms against a serialized
# control of at least 800 ms) — which is what `MB.3.13`'s and `MB.24.4`'s
# acceptance claims.
#
# `rs-test-php-image` still runs only the **three** its image compiles
# (`critical_php_backend`, `critical_php_embed` and `critical_file_sink`:
# `e2e/php-sapi/Dockerfile` hardcodes them and this target requires exactly three
# recorded executables), so `critical_parallel_views` does not reach that leg yet
# — a filed plan follow-up (`MB.24.4`), not a claim of this variable.
#
# `--nocapture` is load-bearing, not decoration: the evidence lines
# (`served: HTTP/1.1 200 OK  cpu_ns=…`, `retired pid … respawned pid …`) are
# `println!`s of **passing** tests, which cargo captures and drops. Every suite
# chooses its branch from an independent host probe, so on a host without the
# pair the same command is green and says `refused`: that is why
# `.github/workflows/php-sapi.yml` greps the log for the refusal markers and for
# the served ones instead of trusting the exit status.
#
# `BLASTIT_PHP_SHIM_DIR` is set here (not left to the suites' `target/php-sapi`
# fallback) so the shim this host just built is the one the boot loads.
#
# `PHP_SINK_SUITE_ARGS` is separate only because it lives in another crate: the
# protected loaded-file sink suite (`MB.23.4.2`) asserts the shim's
# `blastit_php_file_sink_install` contract directly (`blastit_php_sapi::Loaded`),
# so it is `-p blastit-php-sapi`, and `cargo` resolves `--test` per package.
PHP_SUITE_ARGS := --test critical_php_backend --test critical_php_embed \
	--test critical_parallel_views
PHP_SINK_SUITE_ARGS := --test critical_file_sink
# `php-sapi-e2e` builds this tag (forwarded to `php_sapi_probe.py --image`, so
# the variable is the truth for both targets); `rs-test-php-image` runs the legs
# against the pair inside it. Debian's `libphp8.4-embed` puts the library at
# `/usr/lib/libphp8.4.so` (no multi-arch directory — checked in
# `debian:trixie-slim`), and the image's own `make php-sapi` leaves the shim at
# `/src/target/php-sapi`, which is the path the suites are pointed at below.
#
# **The suites are built in the image, not here** (`MB.3.58`). They used to be
# compiled on the runner and executed inside the leg image, which only works
# while the runner's glibc is no newer than the leg's: on the `debian:bookworm`
# leg (glibc 2.36) an Ubuntu 24.04 runner (2.39) produced a binary that could not
# start —
#
#   /lib/x86_64-linux-gnu/libc.so.6: weak version 'GLIBC_2.39' not found
#   (required by target/debug/blastit)
#
# — and raising the leg's base would have proved nothing about the older distros
# the shim is shipped for. `e2e/php-sapi/Dockerfile` now installs libclang and
# compiles the suites, so the image executes what it built; it records the three
# executables in `/src/target/php-suite-executables` and this target runs exactly
# those (a musl leg records none, and is refused by name instead of passing).
PHP_SAPI_IMAGE ?= blastit-php-sapi-e2e:local
PHP_SAPI_IMAGE_MINOR ?= 8.4
PHP_SAPI_IMAGE_LIBPHP ?= /usr/lib/libphp$(PHP_SAPI_IMAGE_MINOR).so
# The shim directory **inside** the image: the one its own `make php-sapi` wrote
# and the probe loaded. Nothing is copied out to a host path any more, so the
# suites cannot be pointed at a shim from a different build.
PHP_SAPI_IMAGE_SHIM_DIR ?= /src/target/php-sapi

rs-test-php: php-sapi ## Real-pair PHP legs: critical_php_backend + critical_php_embed + critical_parallel_views + critical_file_sink
	@echo "BLASTIT_PHP_SHIM_DIR=$(PHP_SAPI_OUT) cargo test -p blastit $(PHP_SUITE_ARGS) -- --nocapture"
	BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_OUT)" \
		$(RUST_ENV) $(CARGO) test -p blastit --locked $(PHP_SUITE_ARGS) -- --nocapture
	@echo "BLASTIT_PHP_SHIM_DIR=$(PHP_SAPI_OUT) cargo test -p blastit-php-sapi $(PHP_SINK_SUITE_ARGS) -- --nocapture"
	BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_OUT)" \
		$(RUST_ENV) $(CARGO) test -p blastit-php-sapi --locked $(PHP_SINK_SUITE_ARGS) -- --nocapture

rs-test-php-image: ## Docker: all three PHP suites, in the image that built them (what CI runs)
	@docker image inspect "$(PHP_SAPI_IMAGE)" >/dev/null 2>&1 || { \
		echo "rs-test-php-image: no image '$(PHP_SAPI_IMAGE)'; build it first:" >&2; \
		echo "  make php-sapi-e2e PHP_SAPI_KEEP=1" >&2; \
		echo "  (the image is where the suites are compiled: MB.3.58)" >&2; \
		exit 1; \
	}
	# Two runs, and the second one is not optional (`MB.3.58` review).
	#
	# The first runs the whole suite as the image's own root: the tree the suites
	# were built in is root-owned `0755`, and `critical_php_embed`'s provenance
	# legs need a fixture parent they can create and whose ancestors no other uid
	# can write. `--cap-add SYS_PTRACE` lets root read the maps of the
	# `PR_SET_DUMPABLE(0)` roles (Docker's default root set has no such
	# capability), so `no_libphp_is_mapped_above_the_view_worker` can *content*-check
	# each role above the view instead of skipping them.
	#
	# **That is not the witness for `PR_SET_DUMPABLE(0)`.** Under root the suite
	# takes its `euid == 0` branch, which never asserts `denied > 0`; the only
	# assertion of that invariant is the unprivileged branch, and `ci.yml`'s root
	# run never reaches it (no `libphp` on the runner). So the second run executes
	# the same test as the caller's uid, where the roles answer `EACCES`, and this
	# target refuses to pass unless the suite reports at least one denied read:
	# dropping `set_dumpable(false)` in `php_view.rs` must fail here.
	@set -eu; \
	echo "docker run $(PHP_SAPI_IMAGE)  # the suites this image built (root)"; \
	docker run --rm --cap-add SYS_PTRACE \
		-e BLASTIT_LIBPHP="$(PHP_SAPI_IMAGE_LIBPHP)" \
		-e BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_IMAGE_SHIM_DIR)" \
		"$(PHP_SAPI_IMAGE)" \
		sh -eu -c ' \
			set --; \
			while IFS= read -r binary; do set -- "$$@" "$$binary"; done \
				< /src/target/php-suite-executables; \
			if [ "$$#" -ne 3 ]; then \
				echo "rs-test-php-image: the image records $$# suite executable(s), expected 3" >&2; \
				echo "  a musl leg or a leg with suites:false builds none (MB.3.39/MB.3.58)" >&2; \
				exit 1; \
			fi; \
			for binary in "$$@"; do \
				echo "$$binary --nocapture"; \
				"$$binary" --nocapture; \
			done'
	@set -eu; \
	echo "docker run --user $$(id -u):$$(id -g) $(PHP_SAPI_IMAGE)  # the PR_SET_DUMPABLE(0) witness"; \
	out=$$(docker run --rm --user "$$(id -u):$$(id -g)" \
		-e BLASTIT_LIBPHP="$(PHP_SAPI_IMAGE_LIBPHP)" \
		-e BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_IMAGE_SHIM_DIR)" \
		"$(PHP_SAPI_IMAGE)" \
		sh -eu -c ' \
			binary=$$(grep -m1 critical_php_embed /src/target/php-suite-executables); \
			[ -n "$$binary" ] || { \
				echo "rs-test-php-image: the image records no critical_php_embed executable" >&2; \
				exit 1; \
			}; \
			exec "$$binary" no_libphp_is_mapped_above_the_view_worker --exact --nocapture' \
		2>&1) && rc=0 || rc=$$?; \
	printf '%s\n' "$$out"; \
	[ "$$rc" -eq 0 ] || { echo "rs-test-php-image: the unprivileged maps leg failed (exit $$rc)" >&2; exit 1; }; \
	denied=$$(printf '%s\n' "$$out" | sed -n 's/.* \([0-9][0-9]*\) denied (EACCES).*/\1/p' | tail -n 1); \
	if [ -z "$$denied" ] || [ "$$denied" -lt 1 ]; then \
		echo "rs-test-php-image: NO role was denied its maps to an unprivileged caller ($${denied:-none} denied):" >&2; \
		echo "  that is the assertion of PR_SET_DUMPABLE(0) (ADR-0010 §2) and it did not fire" >&2; \
		exit 1; \
	fi; \
	echo "rs-test-php-image: PR_SET_DUMPABLE(0) witness — $$denied role(s) denied (EACCES) to uid $$(id -u)"

mutants-rust: ## Run cargo-mutants with disk scratch + cgroup memory cap (PKG=<crate> to scope)
	@mkdir -p "$(MUTANTS_TMP)"
	MUTANTS_KEEP_SCRATCH="$(MUTANTS_KEEP_SCRATCH)" MUTANTS_STALE_MIN="$(MUTANTS_STALE_MIN)" scripts/mutants-scratch.sh prune "$(MUTANTS_TMP)"
	MUTANTS_MIN_FREE_GB="$(MUTANTS_MIN_FREE_GB)" scripts/mutants-scratch.sh preflight "$(MUTANTS_TMP)"
	@set -eu; \
	run() { \
		$(RUST_ENV) TMPDIR="$(MUTANTS_TMP)" MUTANTS_JOBS="$(MUTANTS_JOBS)" \
			MUTANTS_TIMEOUT="$(MUTANTS_TIMEOUT)" MUTANTS_MEMORY_MAX="$(MUTANTS_MEMORY_MAX)" \
			CARGO="$(CARGO)" scripts/mutants-run.sh "$$@"; \
	}; \
	if [ -n "$(PKG)" ]; then \
		run -p "$(PKG)" $(MUTANTS_FEATURE_ARGS); \
	else \
		others="$$($(CARGO) metadata --no-deps --format-version 1 \
			| $(UV) run --no-project python -c 'import json, sys; print(" ".join(sorted(p["name"] for p in json.load(sys.stdin)["packages"] if p["name"] != "blastit")))')"; \
		set --; \
		for package in $$others; do set -- "$$@" -p "$$package"; done; \
		run "$$@"; \
		run -p blastit --features python; \
	fi

mutants-clean: ## Delete mutants.out and every cargo-mutants scratch copy (report bytes freed)
	scripts/mutants-scratch.sh clean "$(MUTANTS_TMP)"

mutants-scope-probe: ## Show the cgroup and memory.max a cargo-mutants campaign would run under
	TMPDIR="$(MUTANTS_TMP)" scripts/mutants-run.sh --probe --memory-max "$(MUTANTS_MEMORY_MAX)"

print-mutants-tmp: ## Print the mutants scratch directory (disk, never /tmp)
	@echo "$(MUTANTS_TMP)"

check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test rs-no-python seam-check platform-matrix-check workflow-check packaging-gates-test doc-tone manifest-purity zed-check lsp-portable plan-validate ## Full fast unprivileged check
