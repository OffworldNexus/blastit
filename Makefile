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

# Dogfood the static backend: our own documentation site, built by Zensical and
# served by a freshly built `blastit dev` through the real `[route.static]` path
# (resolver, ETag/304, refusal matrix) — not a special-case file server. The
# stage lives under target/ (gitignored), so the manifest sits beside the files
# it serves and `root` never needs a `..`. `docs-serve-check` binds a TCP port
# and builds the standalone binary, so it is a developer target, not part of
# `check`.
PORT ?= 8080
DOCS_SERVE_STAGE ?= $(CURDIR)/target/docs-serve
DOCS_SERVE_MANIFEST := $(DOCS_SERVE_STAGE)/blastit.toml
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
        rs-format rs-fmt rs-lint rs-doc rs-test rs-test-critical rs-no-python \
        py-format py-lint test-critical docs-check docs-unit docs-test e2e mutants-rust \
        docs-serve docs-serve-stage docs-serve-check php-sapi php-sapi-e2e \
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

# The standalone archive's build (ADR-0010 §4): every embedded-CPython role off,
# so nothing in the graph links `libpython`, and the `view.chan` framing, the
# stage-0 env strip, the seccomp roles and the supervisor still compile. The
# unit tests that need no interpreter run here too, so the gate proves the split
# rather than only compiling it; the integration suites all need a live CPython
# and belong to the default build. `cargo tree -e normal` excludes dev- and
# build-dependencies, which is exactly "what the binary links".
rs-no-python: ## Compile + test the interpreter-free (standalone/PHP) build
	@echo "cargo check -p blastit --no-default-features --features php"
	$(CARGO) check -p blastit --no-default-features --features php --locked
	@echo "cargo check -p blastit-view --no-default-features"
	$(CARGO) check -p blastit-view --no-default-features --locked
	@echo "cargo test  -p blastit --no-default-features --features php --lib"
	$(CARGO) test -p blastit --no-default-features --features php --locked --lib
	@echo "cargo test  -p blastit --no-default-features --features php --test no_python_build"
	$(CARGO) test -p blastit --no-default-features --features php --locked --test no_python_build
	$(CARGO) test -p blastit-view --no-default-features --locked --lib
	@echo "clippy      -p blastit{,-view} --no-default-features"
	$(CARGO) clippy -p blastit --no-default-features --features php --all-targets --locked -- -D warnings
	$(CARGO) clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
	@count=$$($(CARGO) tree -p blastit --no-default-features --features php -e normal \
		| grep -c pyo3 || true); \
	if [ "$$count" -ne 0 ]; then \
		echo "error: the interpreter-free build depends on pyo3 ($$count): it would link libpython" >&2; \
		exit 1; \
	fi; \
	echo "interpreter-free build: 0 pyo3 dependencies (no libpython linked)"

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

# Build the site, then stage exactly what blastit serves: the files under
# `site/` and a generated manifest beside them. Non-strict on purpose —
# `docs-check` owns `--strict`; a preview should still show a page whose
# neighbour has a broken link.
docs-serve-stage:
	@set -eu; \
	echo "zensical build   -> site/"; \
	$(UVX) zensical build; \
	rm -rf "$(DOCS_SERVE_STAGE)"; \
	mkdir -p "$(DOCS_SERVE_STAGE)/site"; \
	cp -R site/. "$(DOCS_SERVE_STAGE)/site/"; \
	printf '%s\n' \
		'# Generated by `make docs-serve`; blastit serves the built site from here.' \
		'version = 1' \
		'' \
		'[[route]]' \
		'name = "docs"' \
		'path = [".*"]' \
		'' \
		'[route.static]' \
		'root = "site"' \
		> "$(DOCS_SERVE_MANIFEST)"; \
	echo "cargo build      -> $(DOCS_SERVE_BIN)"; \
	$(RUST_ENV) $(CARGO) build -p blastit --locked

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

e2e: ## Build wheel and run Docker + rootless capability e2e tests
	scripts/build-wheel.sh
	E2E_DOCKERFILE=Dockerfile.prebuilt $(UV) run --no-project e2e/run.py
	$(UV) run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt

# Per-minor PHP SAPI shims (MB.3.12, `crates/blastit-php-sapi`). A host without
# PHP headers skips a minor with one line naming the package, so `make check`
# stays green on a PHP-free machine: the shim is *built* here, and only
# `php-sapi-e2e` (Docker + Debian's `libphp8.4-embed`) *proves* that it loads,
# serves a request, contains an `exit()`, and refuses the four FastCGI names.
PHP_MINORS ?= 8.4 8.3 8.2
PHP_SAPI_CRATE := $(CURDIR)/crates/blastit-php-sapi
PHP_SAPI_OUT ?= $(CURDIR)/target/php-sapi
PHP_SAPI_CC ?= $(CC)

php-sapi: ## Build the C SAPI shims whose php-config exists; name the absent ones
	@set -eu; \
	mkdir -p "$(PHP_SAPI_OUT)"; \
	built=0; skipped=""; \
	explicit="$(PHP_CONFIG)"; \
	for minor in $(PHP_MINORS); do \
		cfg=""; \
		if [ -n "$$explicit" ] && [ "$$("$$explicit" --version 2>/dev/null | cut -d. -f1-2)" = "$$minor" ]; then \
			cfg="$$explicit"; \
		fi; \
		if [ -z "$$cfg" ]; then \
			for candidate in "php-config$$minor" php-config; do \
				path="$$(command -v "$$candidate" 2>/dev/null || true)"; \
				[ -n "$$path" ] || continue; \
				if [ "$$("$$path" --version 2>/dev/null | cut -d. -f1-2)" = "$$minor" ]; then cfg="$$path"; break; fi; \
			done; \
		fi; \
		digits="$$(printf '%s' "$$minor" | tr -d '.')"; \
		out="$(PHP_SAPI_OUT)/libblastit_php$$digits.so"; \
		if [ -z "$$cfg" ]; then \
			echo "php-sapi: skipped $$minor: no php-config for PHP $$minor (Debian/Ubuntu: apt install php$$minor-dev libphp$$minor-embed)"; \
			skipped="$$skipped $$minor"; \
			continue; \
		fi; \
		echo "php-sapi: cc -> $$out ($$cfg reports $$("$$cfg" --version))"; \
		"$(PHP_SAPI_CC)" -O2 -D_GNU_SOURCE -fPIC -shared -Wall -Wextra -Werror \
			-Wl,-soname,libblastit_php$$digits.so \
			-I"$(PHP_SAPI_CRATE)/c/include" \
			$$("$$cfg" --includes) \
			-o "$$out" "$(PHP_SAPI_CRATE)/c/blastit_php_sapi.c"; \
		built=$$((built + 1)); \
	done; \
	echo "php-sapi: $$built shim(s) in $(PHP_SAPI_OUT); skipped:$${skipped:- none}"

php-sapi-e2e: ## Docker: build one shim and prove it with the real-load probe (not in `check`)
	$(UV) run --no-project e2e/php_sapi_probe.py $(if $(PHP_MINOR),--minor $(PHP_MINOR))

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

check: guard critical-guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check docs-test rs-no-python plan-validate ## Full fast unprivileged check
