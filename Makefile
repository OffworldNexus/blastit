# Blastit root Makefile: unified quality gates for Rust crates, Python tooling, docs, and the plan tree.
#
#   make help             # list targets
#   make fast-check       # fast per-slice check: guard + critical-guard + rs-fmt + rs-lint + rs-doc + rs-test + py-lint + plan-validate
#   make check            # full unprivileged epic/release gate (fast-check + docs + wheel + standalone ELF + packaging gates)
#   make clean            # format Rust and Python harness tools, then run lints
#   make docs-check       # build Zensical documentation site with --strict (zero broken links/anchors)
#   make docs-unit        # run the runnable-docs gate's own tests (offline, no wheel build)
#   make docs-serve       # build the site and serve it through a freshly built `blastit dev` (PORT=8080)
#   make docs-serve-check # bounded smoke test of the docs serve stack (binds a port; not in `check`)
#   make test-critical    # run critical skip check + staged Python critical tests (offline)
#   make rs-test-critical # run every crates/*/tests/critical_*.rs suite standalone
#   make e2e              # build manylinux_2_28 wheel and run Docker + rootless capability e2e tests
#   make install          # stage a self-contained release bundle under PREFIX (default ~/.local)
#   make uninstall        # remove the bundle `make install` staged under PREFIX
#   make php-sapi         # build the per-minor C SAPI shims this host has headers for
#   make php-sapi-e2e     # Docker: build one shim and prove it with the real-load probe
#   make rs-test-php      # real-pair PHP suites against host libphp + shim
#   make rs-test-php-image # real-pair PHP suites inside the php-sapi-e2e image
#   make mutants-rust     # run cargo-mutants on critical crates (PKG=blastit-sqlpolicy), disk scratch + memory cap
#   make mutants-clean    # delete mutants.out and cargo-mutants scratch copies

UV ?= uv
UVX ?= uvx --offline
CARGO ?= cargo
DOCS_PYTEST ?= 9.2.0

# `make install`: one self-contained bundle under PREFIX (`MB.40.8`).
# `$PREFIX/bin/{blastit,blastit-lsp}` are symlinks into `$PREFIX/share/blastit`,
# so `--version`, `dev` and `check` work with no `uv`, no wheel and no `PATH`.
PREFIX ?= $(HOME)/.local
INSTALL_BINDIR := $(PREFIX)/bin
INSTALL_LIBDIR := $(PREFIX)/share/blastit/lib
INSTALL_PYDIR  := $(PREFIX)/share/blastit/python
# `make PREFIX= install` would stage into `/bin` and `/share`; refuse at parse
# time instead of writing outside the prefix the caller meant.
ifneq ($(filter install uninstall,$(MAKECMDGOALS)),)
$(if $(strip $(PREFIX)),,$(error PREFIX must not be empty: e.g. make install PREFIX=/some/dir))
endif

# Resolve CPython >= 3.13 via uv and export PYTHONHOME + LD_LIBRARY_PATH for embedded PyO3 test binaries.
PYO3_BIN := $(shell readlink -f "$$($(UV) python find --no-project '>=3.13' 2>/dev/null || which python3)")
PYO3_LIBDIR := $(abspath $(dir $(PYO3_BIN))/../lib)
PYO3_PREFIX := $(if $(PYO3_BIN),$(abspath $(dir $(PYO3_BIN))/..),)
RUST_ENV = $(if $(PYO3_PREFIX),PYTHONHOME="$(PYO3_PREFIX)") PYO3_PYTHON="$(PYO3_BIN)" LD_LIBRARY_PATH="$(PYO3_LIBDIR)$${LD_LIBRARY_PATH:+:$$LD_LIBRARY_PATH}"
# Standalone builds link zero libpython and strip PYO3_PYTHON/PYTHONHOME from the build environment.
STANDALONE_ENV = env -u PYO3_PYTHON -u PYTHONHOME

# cargo-mutants scratch on disk (never tmpfs /tmp) with a 12 GiB cgroup v2 cap and 5 GiB free-space preflight.
MUTANTS_TMP ?= $(or $(XDG_CACHE_HOME),$(HOME)/.cache)/blastit-mutants
MUTANTS_JOBS ?= 2
MUTANTS_TIMEOUT ?= 90
MUTANTS_MEMORY_MAX ?= 12G
MUTANTS_MIN_FREE_GB ?= 5
MUTANTS_KEEP_SCRATCH ?= 2
MUTANTS_STALE_MIN ?= 240
# Enable `--features python` when mutating `blastit` so WSGI/learn paths are compiled and exercised.
MUTANTS_FEATURE_ARGS := $(if $(filter blastit,$(PKG)),--features python)

# Dogfood static serving of the built Zensical site via standalone `blastit dev`.
PORT ?= 8080
DOCS_SERVE_STAGE ?= $(CURDIR)/target/docs-serve
DOCS_SERVE_MANIFEST := $(DOCS_SERVE_STAGE)/Blastfile
DOCS_SERVE_BIN := $(CURDIR)/target/debug/blastit
DOCS_SERVE_CHECK_PORT_BASE ?= 8765

# Validate PORT at parse time only when `docs-serve` is requested.
ifeq ($(filter docs-serve,$(MAKECMDGOALS)),docs-serve)
DOCS_SERVE_PORT_PROBLEM := $(shell p='$(PORT)'; \
	if ! printf '%s' "$$p" | grep -qE '^[0-9]+$$'; then echo 'is not a port number'; \
	elif [ "$$p" -lt 1 ] || [ "$$p" -gt 65535 ]; then echo 'must be within 1..65535'; \
	fi)
ifneq ($(DOCS_SERVE_PORT_PROBLEM),)
$(error PORT $(DOCS_SERVE_PORT_PROBLEM): e.g. `make docs-serve PORT=8080` (got PORT='$(PORT)'))
endif
endif

# Staging directory for `test-critical` and banned skip constructs in protected critical tests.
CRITICAL_STAGE ?= $(CURDIR)/target/python-critical
CRITICAL_SKIP_PATTERN := \#\[[[:space:]]*ignore|\bneeds_root[[:space:]]*\(|pytest\.mark\.skip|pytest\.skip[[:space:]]*\(|importorskip[[:space:]]*\(|unittest\.skip

.PHONY: help fast-check check clean guard critical-guard critical-skip-check plan-validate \
        rs-format rs-fmt rs-lint rs-doc rs-test rs-test-critical rs-no-python rs-no-default seam-check \
        py-format py-lint test-critical docs-check docs-unit docs-test e2e mutants-rust \
        docs-serve docs-serve-stage docs-serve-check install uninstall php-sapi php-sapi-e2e py-sapi \
        rs-test-php rs-test-php-image platform-matrix-check standalone standalone-glibc workflow-check \
        packaging-gates-test zed-check mutants-clean mutants-scope-probe print-mutants-tmp

help: ## Show available Make targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

clean: rs-format py-format rs-lint py-lint ## Format Rust + Python tools, then lint

guard: ## Refuse >5 MiB files, .env secrets, or stray binary artefacts
	$(UV) run --no-project tools/guard.py

critical-guard: ## Protect critical tests (report by default; pass BASE=<rev> NODE=<id> to enforce)
	$(UV) run --no-project tools/critical_guard.py --base $(or $(BASE),HEAD) $(if $(NODE),--node $(NODE))

plan-validate: ## Validate plan/nodes consistency
	$(UV) run --no-project tools/plan.py validate

platform-matrix-check: ## Verify published docs match scripts/platform_matrix.py claims
	$(UV) run --no-project scripts/platform_matrix.py --check

workflow-check: ## Audit GitHub Actions workflow graphs and matrices (offline, locked)
	UV_OFFLINE=1 $(UV) run --no-project --locked scripts/check_workflows.py

packaging-gates-test: ## Run positive + negative self-tests for workflow, matrix, wheel-tag, and exec-env gates
	UV_OFFLINE=1 $(UV) run --no-project scripts/test_packaging_gates.py

zed-check: ## Static checks + negative self-tests for editors/** (Zed extension + tree-sitter grammar)
	$(UV) run --no-project scripts/check_zed_extension.py
	UV_OFFLINE=1 $(UV) run --no-project scripts/test_check_zed_extension.py

rs-format: ## Format Rust workspace
	$(CARGO) fmt --all

rs-fmt: ## Check Rust workspace formatting
	$(CARGO) fmt --all -- --check

rs-lint: ## Clippy workspace with warnings denied (python feature enabled)
	$(RUST_ENV) $(CARGO) clippy --workspace --all-targets --locked --features python -- -D warnings

rs-doc: ## Check rustdoc across workspace with warnings denied
	$(RUST_ENV) RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --locked --features python

rs-test: ## Run all unprivileged Rust unit, integration (including critical_*), and doc tests
	$(RUST_ENV) $(CARGO) test --workspace --all-targets --locked --features python
	$(RUST_ENV) $(CARGO) test --workspace --doc --locked --features python

# Verify default standalone `blastit` links zero libpython/RUNPATH and `blastit-view` stays runtime-agnostic.
rs-no-python: ## Build + inspect default standalone binary: zero libpython, zero RUNPATH
	@echo "cargo build -p blastit --locked            # default standalone artifact"
	$(STANDALONE_ENV) $(CARGO) build -p blastit --locked
	scripts/check-portable-elf.sh "$(CURDIR)/target/debug/blastit"
	@set -e; count=$$(grep -c -e pyo3 -e blastit-php-sapi crates/blastit-view/Cargo.toml || true); \
	if [ "$$count" -ne 0 ]; then \
		echo "error: crates/blastit-view names an interpreter ($$count line(s)): the transport crate must stay plain std" >&2; \
		exit 1; \
	fi; \
	echo "blastit-view: 0 pyo3 / blastit-php-sapi lines"
	$(CARGO) test -p blastit-view --locked
	$(STANDALONE_ENV) $(CARGO) test -p blastit --locked --lib
	$(STANDALONE_ENV) $(CARGO) test -p blastit --locked --test no_python_build
	$(STANDALONE_ENV) $(CARGO) clippy -p blastit --all-targets --locked -- -D warnings
	$(CARGO) clippy -p blastit-view --lib --locked -- -D warnings
	@count=$$($(CARGO) tree -p blastit -e normal \
		| grep -c pyo3 || true); \
	if [ "$$count" -ne 0 ]; then \
		echo "error: the default build depends on pyo3 ($$count): it would link libpython" >&2; \
		exit 1; \
	fi; \
	echo "default build: 0 pyo3 dependencies (no libpython linked)"

# Verify `--no-default-features` pulls no view/pyo3 crates and exercises `static_feature_off`.
rs-no-default: ## Build feature-less binary and prove static mount refusal when disabled
	$(STANDALONE_ENV) $(CARGO) build -p blastit --locked --no-default-features --all-targets
	$(STANDALONE_ENV) $(CARGO) clippy -p blastit --locked --no-default-features --all-targets -- -D warnings
	@set -e; tree=$$($(CARGO) tree -p blastit --no-default-features --locked -e normal --prefix none); \
	for crate in blastit-view-php blastit-view-static pyo3; do \
		count=$$(printf '%s\n' "$$tree" | grep -c "^$$crate v" || true); \
		if [ "$$count" -ne 0 ]; then \
			echo "error: --no-default-features still pulls $$crate ($$count): a feature edge is inverted" >&2; \
			exit 1; \
		fi; \
		echo "no-default build: 0 $$crate dependencies"; \
	done
	$(CARGO) test -p blastit-scheduler --locked --no-default-features --test static_feature_off

# Verify debug-only boot swap seam is present in debug builds and compiled out of release builds.
seam-check: ## Verify debug-only test seam presence in debug and absence in release
	@if [ ! -x "$(CURDIR)/target/debug/blastit" ]; then \
		$(STANDALONE_ENV) $(CARGO) build -p blastit --locked; \
	fi
	scripts/check-no-test-seam.sh --present-all "$(CURDIR)/target/debug/blastit"
	@if [ -x "$(CURDIR)/target/release/blastit" ]; then \
		scripts/check-no-test-seam.sh "$(CURDIR)/target/release/blastit"; \
	else \
		echo "note: no target/release/blastit; scripts/build-standalone.sh checks release artifacts"; \
	fi

standalone: ## Package host standalone archive (release binary + proven shims + checksum)
	scripts/build-standalone.sh

STANDALONE_ARCH ?= $(shell uname -m)
STANDALONE_TARGET ?= $(STANDALONE_ARCH)-unknown-linux-gnu
STANDALONE_BUILDER ?= quay.io/pypa/manylinux_2_28_$(STANDALONE_ARCH)

standalone-glibc: ## Build gnu standalone archive inside manylinux_2_28 image (as CI ships it)
	scripts/build-standalone-in-image.sh \
		--image "$(STANDALONE_BUILDER)" \
		--target "$(STANDALONE_TARGET)" \
		--max-glibc 2_28

rs-test-critical: ## Run every crates/*/tests/critical_*.rs suite standalone
	@set -e; found=0; for file in crates/*/tests/critical_*.rs; do \
		[ -e "$$file" ] || continue; found=1; \
		crate=$$(basename $$(dirname $$(dirname "$$file"))); \
		name=$$(basename "$$file" .rs); \
		features=""; if [ "$$crate" = "blastit" ]; then features="--features python"; fi; \
		echo "cargo test -p $$crate $$features --test $$name"; \
		$(RUST_ENV) $(CARGO) test -p "$$crate" $$features --test "$$name" --locked; \
	done; [ "$$found" -eq 1 ] || echo "no Rust critical tests yet (crates/*/tests/critical_*.rs)"

py-format: ## Format Python harness tools and published package
	$(UVX) ruff check --fix --select I python tools
	$(UVX) ruff format python tools

py-lint: ## Lint and format-check Python package and harness tools
	$(UVX) ruff check python tools
	$(UVX) ruff format --check python tools

critical-skip-check: ## Refuse skip constructs in protected critical tests (R42)
	@set -e; \
	files=$$(ls crates/*/tests/critical_*.rs python/tests/critical/test_*.py 2>/dev/null || true); \
	[ -n "$$files" ] || { echo "no protected critical tests to check"; exit 0; }; \
	if found=$$(grep -nEH "$(CRITICAL_SKIP_PATTERN)" $$files); then \
		echo "error: a protected critical test may never skip (R42):"; \
		echo "$$found"; \
		exit 1; \
	fi; \
	echo "protected critical tests hold no skip construct"

# Stage `_blastit` + `blastit-dbproxy` and run protected Python critical tests (`CARGO_NET_OFFLINE=true` for maturin compatibility).
# Rust `critical_*.rs` binaries run once inside `rs-test`; invoke `make rs-test-critical` for standalone Rust critical runs.
test-critical: critical-skip-check ## Run critical skip check + staged Python critical tests (offline)
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

docs-unit: ## Run runnable-docs and release-hygiene unit tests (offline, no wheel build)
	UV_OFFLINE=1 $(UV) run --no-project --with pytest==$(DOCS_PYTEST) python -m pytest \
		python/tests/test_runnable_docs.py \
		python/tests/runnable_docs/test_harness.py \
		python/tests/test_docs_assets.py \
		python/tests/test_release_scripts.py -q

# Build host wheel into `target/docs-wheel`, preserving any pre-existing `dist/` across the run.
DOCS_WHEEL_STAGE ?= $(CURDIR)/target/docs-wheel
docs-test: SHELL := /bin/bash
docs-test: docs-unit ## Execute runnable console fences across README.md and docs/, then check wheel links
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

# Stage Zensical `site/` + minimal static `Blastfile` and verify portable standalone `blastit`.
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

# Build per-minor C SAPI shims (`libblastit_php<mm>-<arch>-<libc>.so`) using `-isystem` for PHP headers.
PHP_MINORS ?= 8.5 8.4 8.3
PHP_SAPI_CRATE := $(CURDIR)/crates/blastit-php-sapi
PHP_SAPI_OUT ?= $(CURDIR)/target/php-sapi
PHP_SAPI_CC ?= $(CC)
PHP_SAPI_ARCH ?=
PHP_SAPI_LIBC ?=
PHP_SAPI_KEEP ?=

php-sapi: ## Build C SAPI shims whose php-config exists; skip absent minors
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

# Build per-minor CPython shims (`libblastit_py3{13,14}-<arch>-<libc>.so`): each carries
# `UND Py_*` symbols (resolved by the host's libpython at stage-1 `dlopen`) and its own
# `DT_SONAME`, and links no libpython of its own (design §1.1/§1.2). Not part of `check`:
# it needs interpreters, and `rs-no-python` already proves the binary links none.
# Interpreter resolution per minor: `PY_SAPI_PYTHON_<digits>` -> manylinux
# `/opt/python/cp<digits>-cp<digits>/bin/python3` -> `uv python find --no-project <minor>`
# -> `python<minor>` on `PATH`. A minor with no interpreter, or a free-threaded one, is
# skipped; `PY_SAPI_REQUIRE_ALL=1` fails closed instead.
PY_MINORS ?= 3.14 3.13
PY_SAPI_OUT ?= $(CURDIR)/target/py-sapi
PY_SAPI_PROFILE ?= release
PY_SAPI_ARCH ?=
PY_SAPI_LIBC ?=
PY_SAPI_REQUIRE_ALL ?=
PY_SAPI_PYTHON_313 ?=
PY_SAPI_PYTHON_314 ?=

py-sapi: ## Build CPython shims per minor (PY_MINORS="3.14 3.13"; PY_SAPI_REQUIRE_ALL=1 fails closed)
	@set -eu; \
	for tool in readelf nm; do \
		command -v "$$tool" >/dev/null 2>&1 || { \
			echo "py-sapi: $$tool is required to assert the shim's ELF contract (install binutils: apt install binutils, apk add binutils, dnf install binutils)" >&2; \
			exit 2; \
		}; \
	done; \
	arch="$(PY_SAPI_ARCH)"; \
	libc="$(PY_SAPI_LIBC)"; \
	if [ -z "$$arch" ]; then arch="$$(uname -m)"; fi; \
	if [ -z "$$libc" ]; then \
		if [ -e "/lib/ld-musl-$$arch.so.1" ]; then libc=musl; else libc=gnu; fi; \
	fi; \
	case "$$arch" in \
		x86_64|aarch64) ;; \
		*) echo "py-sapi: unsupported architecture '$$arch' (supported: x86_64, aarch64)" >&2; exit 1 ;; \
	esac; \
	case "$$libc" in \
		gnu|musl) ;; \
		*) echo "py-sapi: unsupported libc '$$libc' (supported: gnu, musl)" >&2; exit 1 ;; \
	esac; \
	mkdir -p "$(PY_SAPI_OUT)"; \
	built=0; skipped=""; reasons=""; \
	for minor in $(PY_MINORS); do \
		digits="$$(printf '%s' "$$minor" | tr -d '.')"; \
		interp=""; \
		eval "explicit=\$${PY_SAPI_PYTHON_$$digits:-}"; \
		if [ -n "$$explicit" ]; then interp="$$explicit"; fi; \
		if [ -z "$$interp" ] && [ -x "/opt/python/cp$$digits-cp$$digits/bin/python3" ]; then \
			interp="/opt/python/cp$$digits-cp$$digits/bin/python3"; \
		fi; \
		if [ -z "$$interp" ]; then \
			interp="$$($(UV) python find --no-project "$$minor" 2>/dev/null || true)"; \
		fi; \
		if [ -z "$$interp" ]; then \
			interp="$$(command -v "python$$minor" 2>/dev/null || true)"; \
		fi; \
		if [ -z "$$interp" ] || [ ! -x "$$interp" ]; then \
			echo "py-sapi: skipped $$minor: no interpreter (set PY_SAPI_PYTHON_$$digits, or uv python install $$minor)"; \
			skipped="$$skipped $$minor"; reasons="$$reasons $$minor (no interpreter)"; continue; \
		fi; \
		found="$$("$$interp" -c 'import sys; print("%d.%d" % sys.version_info[:2])' 2>/dev/null || true)"; \
		if [ "$$found" != "$$minor" ]; then \
			echo "py-sapi: refused $$minor: $$interp is CPython $${found:-unknown} (set PY_SAPI_PYTHON_$$digits)" >&2; \
			exit 1; \
		fi; \
		if [ "$$("$$interp" -c 'import sysconfig; print(int(sysconfig.get_config_var("Py_GIL_DISABLED") or 0))')" = 1 ]; then \
			echo "py-sapi: skipped $$minor: $$interp is free-threaded, and a WSGI view needs the GIL (uv python install cpython-$$minor)"; \
			skipped="$$skipped $$minor"; reasons="$$reasons $$minor (free-threaded)"; continue; \
		fi; \
		soname="libblastit_py$$digits-$$arch-$$libc.so"; \
		out="$(PY_SAPI_OUT)/$$soname"; \
		target="$(PY_SAPI_OUT)/build-$$digits"; \
		profile="$(PY_SAPI_PROFILE)"; \
		extra=""; if [ "$$profile" = release ]; then extra="--release"; fi; \
		rustflags="-C link-arg=-Wl,-soname,$$soname"; \
		if [ "$$libc" = musl ]; then rustflags="$$rustflags -C target-feature=-crt-static"; fi; \
		echo "py-sapi: cpython $$minor -> $$out ($$interp)"; \
		CARGO_TARGET_DIR="$$target" PYO3_PYTHON="$$interp" \
			RUSTFLAGS="$${RUSTFLAGS:+$$RUSTFLAGS }$$rustflags" \
			$(CARGO) build -p blastit-py-shim --locked --features extension-module $$extra; \
		built_so="$$target/$$profile/libblastit_py_shim.so"; \
		if [ ! -f "$$built_so" ]; then \
			echo "py-sapi: cargo produced no $$built_so" >&2; exit 1; \
		fi; \
		cp "$$built_so" "$$out"; \
		scripts/check-portable-elf.sh "$$out"; \
		readelf -d "$$out" | sed -n 's/.*(SONAME).*\[\(.*\)\]/\1/p' | grep -qx "$$soname" \
			|| { echo "py-sapi: $$out does not carry DT_SONAME $$soname" >&2; exit 1; }; \
		nm -uD "$$out" | grep -q ' Py_InitializeEx$$' \
			|| { echo "py-sapi: $$out carries no undefined dynamic Py_InitializeEx" >&2; exit 1; }; \
		for symbol in blastit_py_abi_v1 blastit_py_abi_version blastit_py_minor; do \
			nm -D --defined-only "$$out" | awk '{print $$2" "$$3}' | grep -qx "T $$symbol" \
				|| { echo "py-sapi: $$out does not export $$symbol" >&2; exit 1; }; \
		done; \
		echo "py-sapi: proving $$out with the self-contained dlopen probe"; \
		libdir="$$("$$interp" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR") or "")')"; \
		libpython="$$(ls "$$libdir/libpython$${minor}"*.so* 2>/dev/null | head -n 1 || true)"; \
		if [ -z "$$libpython" ]; then \
			echo "py-sapi: CPython $$minor has no shared libpython under $$libdir, so this shim cannot be proven on this host; install it — Debian/Ubuntu: apt install libpython$$minor, Alpine: apk add python3, Fedora/RHEL: dnf install python3-libs" >&2; \
			exit 1; \
		fi; \
		BLASTIT_PY_SAPI_SHIM="$$out" BLASTIT_PY_SAPI_LIBPYTHON="$$libpython" \
			PYO3_PYTHON="$$interp" \
			LD_LIBRARY_PATH="$$libdir$${LD_LIBRARY_PATH:+:$$LD_LIBRARY_PATH}" \
			$(CARGO) test -p blastit-py-shim --locked --features extension-module \
				--test abi_load -- --ignored --nocapture; \
		built=$$((built + 1)); \
	done; \
	if [ -n "$$skipped" ] && [ "$(PY_SAPI_REQUIRE_ALL)" = 1 ]; then \
		echo "py-sapi: PY_SAPI_REQUIRE_ALL=1 and these minors were not built:$$reasons; run uv python install$$skipped (drop --free-threaded where that is the reason) and rerun" >&2; \
		exit 1; \
	fi; \
	echo "py-sapi: $$built shim(s) for $$arch-$$libc in $(PY_SAPI_OUT); skipped:$${skipped:- none}"

php-sapi-e2e: ## Docker: build one shim and prove it with the real-load probe (not in `check`)
	$(UV) run --no-project e2e/php_sapi_probe.py \
		$(if $(PHP_MINOR),--minor $(PHP_MINOR)) \
		$(if $(PHP_SAPI_LIBC),--libc $(PHP_SAPI_LIBC)) \
		--image "$(PHP_SAPI_IMAGE)" \
		$(if $(PHP_SAPI_KEEP),--keep)

# Real-pair PHP suites (`--nocapture` preserves served/respawn evidence lines checked by CI).
PHP_SUITE_ARGS := --test critical_php_backend --test critical_php_embed \
	--test critical_parallel_views
PHP_SINK_SUITE_ARGS := --test critical_file_sink
PHP_SAPI_IMAGE ?= blastit-php-sapi-e2e:local
PHP_SAPI_IMAGE_MINOR ?= 8.4
PHP_SAPI_IMAGE_LIBPHP ?= /usr/lib/libphp$(PHP_SAPI_IMAGE_MINOR).so
PHP_SAPI_IMAGE_SHIM_DIR ?= /src/target/php-sapi

rs-test-php: php-sapi ## Real-pair PHP legs: critical_php_backend + critical_php_embed + critical_parallel_views + critical_file_sink
	@echo "BLASTIT_PHP_SHIM_DIR=$(PHP_SAPI_OUT) cargo test -p blastit $(PHP_SUITE_ARGS) -- --nocapture"
	BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_OUT)" \
		$(RUST_ENV) $(CARGO) test -p blastit --locked $(PHP_SUITE_ARGS) -- --nocapture
	@echo "BLASTIT_PHP_SHIM_DIR=$(PHP_SAPI_OUT) cargo test -p blastit-php-sapi $(PHP_SINK_SUITE_ARGS) -- --nocapture"
	BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_OUT)" \
		$(RUST_ENV) $(CARGO) test -p blastit-php-sapi --locked $(PHP_SINK_SUITE_ARGS) -- --nocapture

# Run image-built PHP suites first as root (`--cap-add SYS_PTRACE` for maps content check), then unprivileged (`PR_SET_DUMPABLE(0)` EACCES witness).
rs-test-php-image: ## Docker: run image-built PHP suites as root + unprivileged dumpable witness
	@docker image inspect "$(PHP_SAPI_IMAGE)" >/dev/null 2>&1 || { \
		echo "rs-test-php-image: no image '$(PHP_SAPI_IMAGE)'; build it first:" >&2; \
		echo "  make php-sapi-e2e PHP_SAPI_KEEP=1" >&2; \
		exit 1; \
	}
# Docker's default outer seccomp profile (`moby/profiles/seccomp/default.json`) answers `fanotify_init`
# with EPERM, and `blastit dev`'s reload needs it; `seccomp=unconfined` on the outer container leaves
# blastit's own inner `seccomp(SECCOMP_SET_MODE_FILTER)` enforcing every view/probe sandbox inside.
	@set -eu; \
	echo "docker run $(PHP_SAPI_IMAGE)  # the suites this image built (root)"; \
	docker run --rm --cap-add SYS_PTRACE --security-opt seccomp=unconfined \
		-e BLASTIT_LIBPHP="$(PHP_SAPI_IMAGE_LIBPHP)" \
		-e BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_IMAGE_SHIM_DIR)" \
		"$(PHP_SAPI_IMAGE)" \
		sh -eu -c ' \
			set --; \
			while IFS= read -r binary; do set -- "$$@" "$$binary"; done \
				< /src/target/php-suite-executables; \
			if [ "$$#" -ne 3 ]; then \
				echo "rs-test-php-image: the image records $$# suite executable(s), expected 3" >&2; \
				exit 1; \
			fi; \
			for binary in "$$@"; do \
				echo "$$binary --nocapture"; \
				"$$binary" --nocapture; \
			done'
	@set -eu; \
	echo "docker run --user $$(id -u):$$(id -g) $(PHP_SAPI_IMAGE)  # the PR_SET_DUMPABLE(0) witness"; \
	out=$$(docker run --rm --user "$$(id -u):$$(id -g)" --security-opt seccomp=unconfined \
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
		echo "rs-test-php-image: NO role was denied its maps to an unprivileged caller ($${denied:-none} denied)" >&2; \
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

fast-check: guard critical-guard rs-fmt rs-lint rs-doc rs-test py-lint plan-validate ## Fast per-slice Rust + Python + plan check

check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test rs-no-python rs-no-default seam-check platform-matrix-check workflow-check packaging-gates-test zed-check plan-validate ## Full unprivileged epic/release gate

install: ## Install a self-contained release bundle under PREFIX (default ~/.local)
	@set -eu; \
	if [ "$(PREFIX)" = "$(HOME)/.local" ] && [ -L "$(INSTALL_BINDIR)/blastit" ]; then \
		target=$$(readlink -f "$(INSTALL_BINDIR)/blastit" 2>/dev/null || true); \
		case "$$target" in \
			*/uv/tools/blastit/*) \
				echo "install: removing the stale 'uv tool' blastit ($$target)"; \
				$(UV) tool uninstall blastit >/dev/null 2>&1 || true ;; \
		esac; \
	fi
	@echo "cargo build --release --locked -> blastit (python,php,static)"
	$(RUST_ENV) $(CARGO) build --release --locked -p blastit --features python,php,static
	@echo "cargo build --release --locked -> blastit-lsp"
	$(RUST_ENV) $(CARGO) build --release --locked -p blastit-lsp
	@echo "cargo build --release --locked -> blastit-py, blastit-dbproxy"
	$(RUST_ENV) $(CARGO) build --release --locked -p blastit-py -p blastit-dbproxy --features blastit/python
	$(MAKE) php-sapi
	@set -eu; \
	rm -rf "$(PREFIX)/share/blastit"; \
	mkdir -p "$(INSTALL_LIBDIR)" "$(INSTALL_PYDIR)/blastit/_bin" "$(INSTALL_BINDIR)"; \
	install -m 0755 target/release/blastit "$(INSTALL_LIBDIR)/blastit"; \
	install -m 0755 target/release/blastit-lsp "$(INSTALL_LIBDIR)/blastit-lsp"; \
	for shim in target/php-sapi/libblastit_php*.so; do \
		[ -e "$$shim" ] || continue; \
		install -m 0755 "$$shim" "$(INSTALL_LIBDIR)/"; \
	done; \
	install -m 0644 python/blastit/*.py "$(INSTALL_PYDIR)/blastit/"; \
	ext="$$($(PYO3_BIN) -c 'import sysconfig; print(sysconfig.get_config_var("EXT_SUFFIX"))')"; \
	install -m 0755 target/release/lib_blastit.so "$(INSTALL_PYDIR)/blastit/_blastit$$ext"; \
	install -m 0755 target/release/blastit-dbproxy "$(INSTALL_PYDIR)/blastit/_bin/blastit-dbproxy"; \
	ln -sfn "$(INSTALL_LIBDIR)/blastit" "$(INSTALL_BINDIR)/blastit"; \
	ln -sfn "$(INSTALL_LIBDIR)/blastit-lsp" "$(INSTALL_BINDIR)/blastit-lsp"; \
	shims=""; \
	for shim in "$(INSTALL_LIBDIR)"/libblastit_php*.so; do \
		[ -e "$$shim" ] || continue; \
		shims="$$shims $$(basename "$$shim")"; \
	done; \
	echo "install: $(INSTALL_BINDIR)/blastit -> $(INSTALL_LIBDIR)/blastit"; \
	echo "install: $(INSTALL_BINDIR)/blastit-lsp -> $(INSTALL_LIBDIR)/blastit-lsp"; \
	echo "install: capabilities: python, php, static"; \
	[ -n "$$shims" ] || shims=" none on this host (make php-sapi prints why)"; \
	echo "install: php sapi shims:$$shims"; \
	echo "install: python gate under $(INSTALL_PYDIR)"; \
	echo "install: $$("$(INSTALL_BINDIR)/blastit" --version)"; \
	case ":$$PATH:" in \
		*":$(INSTALL_BINDIR):"*) echo "install: $(INSTALL_BINDIR) is already on PATH" ;; \
		*) echo "install: add it to PATH:  export PATH=\"$(INSTALL_BINDIR):\$$PATH\"" ;; \
	esac

uninstall: ## Remove the bundle `make install` staged under PREFIX
	@set -eu; \
	removed=0; \
	for name in blastit blastit-lsp; do \
		link="$(INSTALL_BINDIR)/$$name"; \
		if [ -L "$$link" ]; then \
			target=$$(readlink -f "$$link" 2>/dev/null || true); \
			if [ "$$target" = "$(INSTALL_LIBDIR)/$$name" ]; then \
				rm -f "$$link"; removed=$$((removed + 1)); \
			else \
				echo "uninstall: leaving $$link: it points at $${target:-nowhere}, not $(INSTALL_LIBDIR)/$$name" >&2; \
			fi; \
		elif [ -e "$$link" ]; then \
			echo "uninstall: leaving $$link: it is not a symlink into $(INSTALL_LIBDIR)" >&2; \
		fi; \
	done; \
	if [ -d "$(PREFIX)/share/blastit" ]; then \
		rm -rf "$(PREFIX)/share/blastit"; removed=$$((removed + 1)); \
	fi; \
	if [ "$$removed" -eq 0 ]; then \
		echo "uninstall: nothing installed under $(PREFIX)"; \
	else \
		echo "uninstall: removed the bundle under $(PREFIX)"; \
	fi
