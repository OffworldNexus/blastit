# Blastit root Makefile: unified quality gates for Rust crates, Python tooling, docs, and the plan tree.
#
#   make help             # list targets
#   make fast-check       # fast per-slice check: guard + rs-fmt + rs-lint + rs-doc + rs-test + py-lint + plan-validate + install-test
#   make check            # full unprivileged epic/release gate (fast-check + docs + wheel + standalone ELF + E2E)
#   make clean            # format Rust and Python harness tools, then run lints
#   make docs-check       # build Zensical documentation site with --strict (zero broken links/anchors)
#   make docs-unit        # run the package's offline Python suites (no wheel build)
#   make docs-serve       # build the site and serve it through a freshly built `blastit dev` (PORT=8080)
#   make docs-serve-check # bounded smoke test of the docs serve stack (binds a port; not in `check`)
#   make test-critical    # run critical skip check + staged Python critical tests (offline)
#   make rs-test-critical # run every crates/*/tests/critical_*.rs suite standalone
#   make coverage         # instrumented line/region % + uncovered lines (TESTS="…" FILES="…")
#   make e2e              # build manylinux_2_28 wheel and run Docker + rootless capability e2e tests
#   make e2e-universal    # one staged bundle, four runtimes: CPython 3.13/3.14, PHP 8.4, static, handoff (slow)
#   make install          # stage a self-contained release bundle under PREFIX (default ~/.local)
#   make uninstall        # remove the bundle `make install` staged under PREFIX
#   make packages         # build blastit_<ver>_<arch>.deb + blastit-<ver>-<rel>.<arch>.rpm (slow)
#   make packages-test    # install those packages in debian:trixie-slim / rockylinux:9 (slow, Docker)
#   make php-sapi         # build the per-minor C SAPI shims this host has headers for
#   make php-sapi-e2e     # Docker: build one shim and prove it with the real-load probe
#   make rs-test-php      # real-pair PHP suites against host libphp + shim
#   make rs-test-php-image # real-pair PHP suites inside the php-sapi-e2e image

UV ?= uv
UVX ?= uvx --offline
CARGO ?= cargo

# `make install`: one self-contained bundle under PREFIX (`MB.40.8`).
# `$PREFIX/bin/{blastit,blastit-lsp}` are symlinks into `$PREFIX/share/blastit`,
# so `--version`, `dev` and `check` work with no `uv`, no wheel and no `PATH`.
PREFIX ?= $(HOME)/.local
INSTALL_BINDIR := $(PREFIX)/bin
INSTALL_SHARE  := $(PREFIX)/share/blastit
INSTALL_LIBDIR := $(INSTALL_SHARE)/lib
INSTALL_PYDIR  := $(INSTALL_SHARE)/python
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

# Native packages (`MB.42.3.1`): built from a bundle assembled by
# `scripts/build-package-bundle.sh`, which compiles `blastit`/`blastit-lsp` inside
# `manylinux_2_28` and takes the already-probed `make py-sapi` / PHP distro shims.
# A host `make install` build is deliberately *not* the input: its glibc floor is
# whatever machine ran it, and a package is a claim about other machines.
#
# The floor is a *promise to the oldest advertised distribution*:
# `PACKAGES_SHIM_FLOOR` (2.34, Rocky/RHEL 9) is the ceiling `--bundle-floor` and
# both builders enforce, and the `Depends:`/`Requires:` they write is the measured
# number. A shim above the ceiling is rebuilt on an older base (Surý/bookworm for
# PHP, `manylinux_2_28` for the rest) — never accommodated by dropping a
# distribution.
#
# Staging, the built files and the container proof all live under `/tmp/opencode`;
# nothing is installed on the host. Both targets are slow (a container build,
# Docker, dnf) and stay out of `check`. `MB.42.3.4` wires `packages.yml`/`nightly.yml`;
# until then they run by hand (and are the commands that gate will call).
# The `.deb` keeps `blastit_<ver>_<arch>.deb` (no Debian revision by default);
# the `.rpm` needs a release, which is what a nightly stamps.
PACKAGES_SCRATCH ?= $(if $(TMPDIR),$(TMPDIR),/tmp)/opencode/blastit-packages
PACKAGES_PREFIX  ?= $(PACKAGES_SCRATCH)/prefix
PACKAGES_OUT     ?= $(PACKAGES_SCRATCH)
# The declared ceiling for a package bundle, `<major>_<minor>`, from the matrix
# that states it — never a literal here.
PACKAGES_SHIM_FLOOR ?= $(shell $(UV) run --no-project scripts/platform_matrix.py --shim-floor)
BLASTIT_VERSION  ?= $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
BLASTIT_RELEASE  ?= 1

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

.PHONY: help fast-check check clean guard critical-skip-check plan-validate \
        rs-format rs-fmt rs-lint rs-doc rs-test rs-test-critical rs-no-python rs-no-default coverage \
        universal-smoke py-format py-lint test-critical docs-check docs-unit e2e e2e-universal \
        docs-serve docs-serve-stage docs-serve-check install uninstall php-sapi php-sapi-e2e py-sapi \
        rs-test-php rs-test-php-image python-runtime standalone standalone-glibc \
        zed-check \
        packages packages-test

help: ## Show available Make targets
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

clean: rs-format py-format rs-lint py-lint ## Format Rust + Python tools, then lint

guard: ## Refuse >5 MiB files, .env secrets, or stray binary artefacts
	$(UV) run --no-project tools/guard.py

plan-validate: ## Validate plan/nodes consistency
	$(UV) run --no-project tools/plan.py validate

# The extension is compiled and unit-tested here; those tests are the grammar
# guard: `editors/zed/src/lib.rs` parses both committed `highlights.scm` files
# (the grammar package's must be a **subset** of Zed's, never an equal copy),
# checks every query against `node-types.json` and `test/corpus`/`test/fixtures`,
# and rejects a capture no bundled theme styles.
zed-check: ## Compile the Zed extension; its tests guard the grammar and query fixtures
	cd editors/zed && $(CARGO) test --locked

rs-format: ## Format Rust workspace
	$(CARGO) fmt --all

rs-fmt: ## Check Rust workspace formatting
	$(CARGO) fmt --all -- --check

rs-lint: ## Clippy workspace with warnings denied (python feature enabled)
	$(RUST_ENV) $(CARGO) clippy --workspace --all-targets --locked --features python -- -D warnings

rs-doc: ## Check rustdoc across workspace with warnings denied
	$(RUST_ENV) RUSTDOCFLAGS="-D warnings" $(CARGO) doc --workspace --no-deps --locked --features python

# The per-minor CPython shims every WSGI test loads: `target/py-sapi` is a
# build artifact, so a gate that skipped it would pass only on a machine that
# already had it. `PY_SAPI_REQUIRE_ALL=1` turns a missing interpreter into a
# refusal instead of a skip.
python-runtime: ## Build both CPython shims for the test gate (fails closed)
	$(MAKE) PY_SAPI_REQUIRE_ALL=1 py-sapi

rs-test: python-runtime ## Run all unprivileged Rust unit, integration (including critical_*), and doc tests
	$(RUST_ENV) $(CARGO) test --workspace --all-targets --locked --features python
	$(RUST_ENV) $(CARGO) test --workspace --doc --locked --features python

# Reviewer-owned coverage audit (`MB.55`): rebuilds only the requested test
# target into `target/cov` (never `target/debug`), runs it, and prints per-file
# line/region % plus exact uncovered line ranges. Uncovered lines are not an
# error: `review-arch` decides delete-vs-extend-E2E. The first run pays the
# instrumented rebuild of the dependency graph; warm runs are fast.
coverage: ## Per-file line/region % + uncovered lines (TESTS="-p <crate> --tests" FILES="crates/<crate>/src/lib.rs")
	@if [ -z '$(strip $(TESTS))' ] || [ -z '$(strip $(FILES))' ]; then \
		echo 'usage: make coverage TESTS="-p <crate> --tests" FILES="crates/<crate>/src/lib.rs"' >&2; \
		exit 2; \
	fi
	$(RUST_ENV) $(UV) run --no-project tools/coverage_report.py --tests "$(TESTS)" --files "$(FILES)"

# Verify the default universal `blastit` links zero libpython/libphp/RUNPATH and still
# pulls the shim ABI (design universal-distribution.md §1.1), and that
# `blastit-view` stays runtime-agnostic.
#
# **What a host build can and cannot prove (`MB.42.14`).** This leg compiles on
# whatever machine runs it, so it can prove the *shape* of the artifact — no
# interpreter linked, no build-host RUNPATH, the frozen shim ABI still pulled — and
# nothing about a C-library floor: a binary built on glibc 2.41 needs 2.41 no matter
# what the gate wishes. Asserting `--max-glibc 2_28` here was therefore structurally
# red on every developer machine and CI host, and it blocked `make check`. The 2.28
# promise is a claim about a *compiler image*, so it belongs to `standalone-glibc`,
# which builds inside `quay.io/pypa/manylinux_2_28_<arch>` and hands the ceiling to
# `scripts/build-standalone.sh` / `scripts/build-lsp.sh` there. This leg keeps that
# owner honest: it refuses to let the ceiling disappear.
rs-no-python: ## Build + inspect the universal binary: zero libpython, zero libphp, zero RUNPATH
	@echo "cargo build -p blastit --locked            # default universal artifact"
	$(STANDALONE_ENV) $(CARGO) build -p blastit --locked
	scripts/check-portable-elf.sh "$(CURDIR)/target/debug/blastit"
	@echo "cargo build --release -p blastit --locked        # the binaries make install ships"
	$(STANDALONE_ENV) $(CARGO) build --release -p blastit --locked
	$(STANDALONE_ENV) $(CARGO) build --release -p blastit-lsp --locked
	@echo "host build: no glibc ceiling — that is standalone-glibc's job (quay.io/pypa/manylinux_2_28_<arch>)"
	scripts/check-portable-elf.sh "$(CURDIR)/target/release/blastit" "$(CURDIR)/target/release/blastit-lsp"
	@set -e; recipe=$$($(MAKE) -n --no-print-directory standalone-glibc); \
	case "$$recipe" in \
		*"--max-glibc $(STANDALONE_MAX_GLIBC)"*) \
			echo "glibc floor: standalone-glibc still builds with --max-glibc $(STANDALONE_MAX_GLIBC) inside its manylinux image" ;; \
		*) \
			echo "error: standalone-glibc no longer passes --max-glibc $(STANDALONE_MAX_GLIBC): the manylinux floor has no owner left." >&2; \
			echo "  It is a claim about the compiler image, so it is asserted there — never on a host build like this one." >&2; \
			exit 1 ;; \
	esac
	@set -e; count=$$(grep -c -e pyo3 -e blastit-php-sapi crates/blastit-view/Cargo.toml || true); \
	if [ "$$count" -ne 0 ]; then \
		echo "error: crates/blastit-view names an interpreter ($$count line(s)): the transport crate must stay plain std" >&2; \
		exit 1; \
	fi; \
	echo "blastit-view: 0 pyo3 / blastit-php-sapi lines"
	$(CARGO) test -p blastit-view --locked
	$(STANDALONE_ENV) $(CARGO) test -p blastit --locked --lib
	$(STANDALONE_ENV) $(CARGO) test -p blastit --locked --test e2e_prod_refusals -- --nocapture
	$(STANDALONE_ENV) $(CARGO) clippy -p blastit --all-targets --locked -- -D warnings
	$(CARGO) clippy -p blastit-view --lib --locked -- -D warnings
	@set -e; tree=$$($(CARGO) tree -p blastit --locked -e normal --prefix none); \
	for crate in pyo3 blastit-view-python; do \
		count=$$(printf '%s\n' "$$tree" | grep -c "^$$crate v" || true); \
		if [ "$$count" -ne 0 ]; then \
			echo "error: the default build still depends on $$crate ($$count): the cut-over is incomplete" >&2; \
			exit 1; \
		fi; \
		echo "default build: 0 $$crate dependencies"; \
	done; \
	count=$$(printf '%s\n' "$$tree" | grep -c "^blastit-py-sapi v" || true); \
	if [ "$$count" -eq 0 ]; then \
		echo "error: the default build must pull blastit-py-sapi (the frozen shim ABI)" >&2; \
		exit 1; \
	fi; \
	echo "default build: blastit-py-sapi present, 0 pyo3, no libpython linked"

# Verify `--no-default-features` pulls no view/pyo3 crates and exercises
# `static_feature_off`. It asserts a *dependency graph*, not an ELF, so it carries no
# portable-ELF or glibc claim: the artifact shape belongs to `rs-no-python` and the
# 2.28 floor to `standalone-glibc`. Adding a ceiling here would smuggle a host build
# into a claim only the manylinux image can honour (`MB.42.14`).
rs-no-default: ## Build feature-less binary and prove static mount refusal when disabled
	$(STANDALONE_ENV) $(CARGO) build -p blastit --locked --no-default-features --all-targets
	$(STANDALONE_ENV) $(CARGO) clippy -p blastit --locked --no-default-features --all-targets -- -D warnings
	@set -e; tree=$$($(CARGO) tree -p blastit --no-default-features --locked -e normal --prefix none); \
	for crate in blastit-py-sapi blastit-view-php blastit-view-static pyo3; do \
		count=$$(printf '%s\n' "$$tree" | grep -c "^$$crate v" || true); \
		if [ "$$count" -ne 0 ]; then \
			echo "error: --no-default-features still pulls $$crate ($$count): a feature edge is inverted" >&2; \
			exit 1; \
		fi; \
		echo "no-default build: 0 $$crate dependencies"; \
	done
	$(CARGO) test -p blastit-scheduler --locked --no-default-features --test static_feature_off

# The facts `MB.42.2.4` adds to the per-slice gate that `rs-test` and
# `rs-no-python` do **not** already carry: the debug `blastit` is still the
# portable universal artifact (the portable-ELF gate on a release binary runs in
# `check`), and the §4 handoff suite runs on its own so a regression names
# handoff instead of hiding inside `rs-test`'s 74 s output.
universal-smoke: ## Debug binary + portable-ELF and the handoff suite (in fast-check)
	$(STANDALONE_ENV) $(CARGO) build -p blastit --locked
	scripts/check-portable-elf.sh "$(CURDIR)/target/debug/blastit"
	$(RUST_ENV) $(CARGO) test -p blastit --locked --features python --test e2e_cli

standalone: py-sapi ## Package host standalone archive (release binary + both shim families + checksum)
	scripts/build-standalone.sh

STANDALONE_ARCH ?= $(shell uname -m)
STANDALONE_TARGET ?= $(STANDALONE_ARCH)-unknown-linux-gnu
STANDALONE_BUILDER ?= quay.io/pypa/manylinux_2_28_$(STANDALONE_ARCH)
# The shipped glibc floor (`MB.42.14`). The image *is* the claim: inside
# `$(STANDALONE_BUILDER)` the C library is already 2.28, and
# `scripts/build-standalone.sh` / `scripts/build-lsp.sh` measure the finished binary
# against this number and refuse a build that outgrew it. This is the only place 2.28
# is asserted — a host build cannot honour it, so `rs-no-python` no longer pretends to.
STANDALONE_MAX_GLIBC ?= 2_28

standalone-glibc: ## Build gnu standalone archive inside manylinux_2_28 image (as CI ships it)
	scripts/build-standalone-in-image.sh \
		--image "$(STANDALONE_BUILDER)" \
		--target "$(STANDALONE_TARGET)" \
		--max-glibc $(STANDALONE_MAX_GLIBC)

packages: py-sapi php-sapi ## Build the .deb + .rpm from a manylinux bundle (slow; Docker for the bundle and rpmbuild when absent)
	scripts/build-package-bundle.sh --out "$(PACKAGES_PREFIX)" \
		--bundle-floor "$(PACKAGES_SHIM_FLOOR)"
	scripts/build-deb.sh --version "$(BLASTIT_VERSION)" \
		--bundle "$(PACKAGES_PREFIX)" --out "$(PACKAGES_OUT)" \
		--max-glibc "$(PACKAGES_SHIM_FLOOR)"
	scripts/build-rpm.sh --version "$(BLASTIT_VERSION)" --release "$(BLASTIT_RELEASE)" \
		--bundle "$(PACKAGES_PREFIX)" --out "$(PACKAGES_OUT)" \
		--max-glibc "$(PACKAGES_SHIM_FLOOR)"

packages-test: ## Install the built .deb/.rpm in debian:trixie (WSGI), debian:bookworm-slim and rockylinux:9 (slow, Docker; not in `check`)
	@set -eu; \
	newest() { \
		find "$(PACKAGES_OUT)" -maxdepth 1 -name "$$1" -type f 2>/dev/null \
			| while IFS= read -r file; do printf '%s %s\n' "$$(stat -c %Y "$$file")" "$$file"; done \
			| sort -nr | head -n 1 | cut -d' ' -f2-; \
	}; \
	deb=$$(newest '*.deb'); rpm=$$(newest '*.rpm'); \
	if [ -z "$$deb" ] && [ -z "$$rpm" ]; then \
		echo "packages-test: no package under $(PACKAGES_OUT): run \`make packages\` first" >&2; \
		exit 1; \
	fi; \
	echo "packages-test: deb=$${deb:-none} rpm=$${rpm:-none}"; \
	BLASTIT_DEB="$$deb" BLASTIT_RPM="$$rpm" scripts/test_packages.sh

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

# A comment is prose, not a construct: the guard strips `//` lines from the Rust
# files and `#` lines from the Python ones *before* matching, so it cannot be
# satisfied only by avoiding a word in a doc comment. `#[ignore]` begins with
# `#`, so a Rust attribute line is never stripped.
critical-skip-check: ## Refuse skip constructs in protected critical tests (R42)
	@set -e; \
	rust=$$(ls crates/*/tests/critical_*.rs 2>/dev/null || true); \
	python=$$(ls python/tests/critical/test_*.py 2>/dev/null || true); \
	[ -n "$$rust$$python" ] || { echo "no protected critical tests to check"; exit 0; }; \
	found=""; \
	if [ -n "$$rust" ]; then \
		found=$$(grep -nEHv '^[[:space:]]*//' $$rust | grep -E "$(CRITICAL_SKIP_PATTERN)" || true); \
	fi; \
	if [ -n "$$python" ]; then \
		found="$$found$$(grep -nEHv '^[[:space:]]*#' $$python | grep -E "$(CRITICAL_SKIP_PATTERN)" || true)"; \
	fi; \
	if [ -n "$$found" ]; then \
		echo "error: a protected critical test may never skip (R42):"; \
		echo "$$found"; \
		exit 1; \
	fi; \
	echo "protected critical tests hold no skip construct"

# Stage the universal binary + the pure-Python package and run the protected Python critical tests (`CARGO_NET_OFFLINE=true` for maturin compatibility).
# Rust `critical_*.rs` binaries run once inside `rs-test`; invoke `make rs-test-critical` for standalone Rust critical runs.
test-critical: critical-skip-check ## Run critical skip check + staged Python critical tests (offline)
	@set -e; \
	stage="$(CRITICAL_STAGE)"; \
	rm -rf "$$stage"; \
	mkdir -p "$$stage/blastit/_bin"; \
	cp python/blastit/*.py "$$stage/blastit/"; \
	echo "cargo build --locked -p blastit"; \
	$(RUST_ENV) CARGO_NET_OFFLINE=true $(CARGO) build --locked -p blastit; \
	cp target/debug/blastit "$$stage/blastit/_bin/blastit"; \
	echo "python -m unittest discover -s python/tests/critical"; \
	CARGO_NET_OFFLINE=true UV_OFFLINE=1 PYTHONPATH="$${stage}:$(CURDIR)/python" \
		$(RUST_ENV) $(UV) run --no-project python -m unittest discover \
		-s python/tests/critical -p 'test_*.py'

docs-check: ## Build Zensical documentation site with --strict
	$(UVX) zensical build --strict

docs-unit: ## Run the package's offline Python suites (no wheel build)
	UV_OFFLINE=1 PYTHONPATH="$(CURDIR)/python" $(UV) run --no-project python -m unittest \
		python/tests/test_check_pytest_gate.py python/tests/test_launcher.py

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

e2e: ## Run the application/runtime e2e suite (Docker, PostgreSQL, PHP, Django/Flask, static)
	$(CARGO) build -p blastit --release --locked
	$(MAKE) python-runtime
	$(UV) run --no-project e2e/run_apps.py

# One artifact, four runtimes (`MB.42.2.4`). Slow: it stages a release bundle,
# builds a venv per interpreter and boots a real `blastit dev` per leg, so it is
# not in `fast-check`; `make check` (the epic/release gate) and this target run it.
# Every leg and its PASS/FAIL summary live in `e2e/universal_binary.py`, and that
# harness refuses a SKIP unless `--allow-skip` is passed — which this target, and
# `make check`, never do.
#
# The bundle is staged **outside the checkout**: `stage0::project_python`'s
# `cargo_target()` adds `<target>/py-sapi` as a `dev` shim candidate when the
# binary sits under a directory named `target`, so a leg could pass on the build
# tree's shim and never touch the bundle's. `BUNDLE_REQUIRE_PHP_SHIMS=1` makes
# `install` refuse a bundle with no PHP shim, so the gate cannot exit 0 on a host
# that never had `libphp-embed`.
E2E_UNIVERSAL_PREFIX ?= $(if $(TMPDIR),$(TMPDIR),/tmp)/opencode/blastit-e2e/prefix
E2E_UNIVERSAL_LEGS ?= --python 3.13 --python 3.14 --php 8.4 --static --handoff --check

e2e-universal: ## One staged bundle proves CPython 3.13/3.14, PHP 8.4, static and handoff (slow)
	BUNDLE_REQUIRE_PHP_SHIMS=1 $(MAKE) install PREFIX="$(E2E_UNIVERSAL_PREFIX)"
	$(UV) run --no-project e2e/universal_binary.py \
		--binary "$(E2E_UNIVERSAL_PREFIX)/bin/blastit" $(E2E_UNIVERSAL_LEGS)

# Build per-minor C SAPI shims (`libblastit_php<mm>-<arch>-<libc>.so`) using `-isystem` for PHP headers.
PHP_MINORS ?= 8.6 8.5 8.4 8.3
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
		nm -uD "$$out" | grep -qE ' Py_Initialize(Ex|FromConfig)$$' \
			|| { echo "py-sapi: $$out carries no undefined dynamic Py_Initialize entry point" >&2; exit 1; }; \
		for symbol in blastit_py_abi_v1 blastit_py_abi_version blastit_py_minor; do \
			nm -D --defined-only "$$out" | awk '{print $$2" "$$3}' | grep -qx "T $$symbol" \
				|| { echo "py-sapi: $$out does not export $$symbol" >&2; exit 1; }; \
		done; \
		echo "py-sapi: $$out carries the frozen artifact contract (DT_SONAME, UND Py_Initialize, exported entry points)"; \
		built=$$((built + 1)); \
	done; \
	if [ -n "$$skipped" ] && [ "$(PY_SAPI_REQUIRE_ALL)" = 1 ]; then \
		echo "py-sapi: PY_SAPI_REQUIRE_ALL=1 and these minors were not built:$$reasons; run uv python install$$skipped (drop --free-threaded where that is the reason) and rerun" >&2; \
		exit 1; \
	fi; \
	echo "py-sapi: $$built shim(s) for $$arch-$$libc in $(PY_SAPI_OUT); skipped:$${skipped:- none}"

# --- ecosystem wrappers: the universal wheel and the Composer pin (`MB.42.3.3`) ----

# `make wheel` builds the **host** wheel: `linux_<arch>` tag, no release promise.
# `make wheel-manylinux` is the release leg — it compiles inside the image whose
# glibc *is* the wheel's floor and requires all six shims (design §6.1).
WHEEL_OUT ?= dist

wheel: ## Build the `py3-none` wheel for this host (host tag; developer artifact)
	scripts/build-wheel.sh -o "$(WHEEL_OUT)"

wheel-manylinux: ## Build the release py3-none-manylinux_2_28 wheel in the builder image (Docker, slow)
	scripts/build-wheel.sh --manylinux 2_28 --require-shims all -o "$(WHEEL_OUT)"

wheel-smoke: ## Install the built wheel in python:3.12/3.13/3.14-slim and drive it (Docker, slow)
	scripts/wheel_container_smoke.sh "$(WHEEL_OUT)"

composer-smoke: ## Prove packages/composer-blastit in composer:2 from a local fixture (Docker, slow)
	scripts/composer_smoke.sh

# The wrapper's SHA-256 table is generated, never typed (`MB.42.3.3` review: all
# four rows sat `pending`, so a real `composer require` always refused). `stamp`
# takes whatever archives exist; a *release* runs the same script with
# `--require-complete` (in `publish-pypi.yml`) so a `pending` row can never ship.
BLASTIT_VERSION ?= 0.2.0

composer-stamp: ## Stamp packages/composer-blastit's digest table from dist/*.tar.gz
	uv run --no-project scripts/composer_sha256.py \
		--version "$(BLASTIT_VERSION)" --archives dist/*.tar.gz
	@uv run --no-project scripts/composer_sha256.py \
		--version "$(BLASTIT_VERSION)" --require-complete >/dev/null 2>&1 || \
		echo "composer-stamp: note: a row is still \`pending\`; \`make composer-stamp\` again once every leg has an archive" >&2

php-sapi-e2e: ## Docker: build one shim and prove it with the real-load probe (not in `check`)
	$(UV) run --no-project e2e/php_sapi_probe.py \
		$(if $(PHP_MINOR),--minor $(PHP_MINOR)) \
		$(if $(PHP_SAPI_LIBC),--libc $(PHP_SAPI_LIBC)) \
		--image "$(PHP_SAPI_IMAGE)" \
		$(if $(PHP_SAPI_KEEP),--keep)

# Real-pair PHP legs (`--nocapture` preserves the `served:` / `retired pid …
# respawned pid` evidence lines CI greps). One suite carries them now:
# `e2e_dev.rs`'s PHP half, built without the WSGI legs so this target needs no
# CPython shim on the host (`--no-default-features --features php,static`).
PHP_SUITE_ARGS := --test e2e_dev --no-default-features --features php,static
PHP_SAPI_IMAGE ?= blastit-php-sapi-e2e:local
PHP_SAPI_IMAGE_MINOR ?= 8.4
PHP_SAPI_IMAGE_LIBPHP ?= /usr/lib/libphp$(PHP_SAPI_IMAGE_MINOR).so
PHP_SAPI_IMAGE_SHIM_DIR ?= /src/target/php-sapi

rs-test-php: php-sapi ## Real-pair PHP legs: e2e_dev's PHP half (per-view grants, faults, php2shell, pin)
	@echo "BLASTIT_PHP_SHIM_DIR=$(PHP_SAPI_OUT) cargo test -p blastit $(PHP_SUITE_ARGS) -- --nocapture"
	BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_OUT)" \
		$(RUST_ENV) $(CARGO) test -p blastit --locked $(PHP_SUITE_ARGS) -- --nocapture

# Run the image-built `e2e_dev` PHP legs twice: as the image's root and as the
# caller's uid, so a served leg is proved for an ordinary user too. Both runs
# must print the `served:` line and the crash-recovery line, and neither may
# print a `SKIP` line: a skipped leg is not a witness, and this image is the only
# boot-level proof of the documented 8.3–8.6 support.
rs-test-php-image: ## Docker: run the image-built e2e_dev PHP legs (root + unprivileged)
	@docker image inspect "$(PHP_SAPI_IMAGE)" >/dev/null 2>&1 || { \
		echo "rs-test-php-image: no image '$(PHP_SAPI_IMAGE)'; build it first:" >&2; \
		echo "  make php-sapi-e2e PHP_SAPI_KEEP=1" >&2; \
		exit 1; \
	}
# Docker's default outer seccomp profile (`moby/profiles/seccomp/default.json`) answers `fanotify_init`
# with EPERM, and `blastit dev`'s reload needs it; `seccomp=unconfined` on the outer container leaves
# blastit's own inner `seccomp(SECCOMP_SET_MODE_FILTER)` enforcing every view/probe sandbox inside.
	@set -eu; \
	for run in "root:" "unprivileged:--user $$(id -u):$$(id -g)"; do \
		label=$${run%%:*}; userarg=$${run#*:}; \
		echo "docker run $$userarg $(PHP_SAPI_IMAGE)  # e2e_dev PHP legs ($$label)"; \
		out=$$(docker run --rm $$userarg --security-opt seccomp=unconfined \
			-e BLASTIT_LIBPHP="$(PHP_SAPI_IMAGE_LIBPHP)" \
			-e BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_IMAGE_SHIM_DIR)" \
			"$(PHP_SAPI_IMAGE)" \
			sh -eu -c ' \
				set --; \
				while IFS= read -r binary; do set -- "$$@" "$$binary"; done \
					< /src/target/php-suite-executables; \
				if [ "$$#" -ne 1 ]; then \
					echo "rs-test-php-image: the image records $$# suite executable(s), expected 1 (e2e_dev)" >&2; \
					exit 1; \
				fi; \
				exec "$$1" --nocapture' 2>&1) && rc=0 || rc=$$?; \
		printf '%s\n' "$$out"; \
		[ "$$rc" -eq 0 ] || { echo "rs-test-php-image: the $$label run failed (exit $$rc)" >&2; exit 1; }; \
		if skipped=$$(printf '%s\n' "$$out" | grep '^SKIP '); then \
			echo "rs-test-php-image: a PHP leg skipped instead of serving, so nothing was proved:" >&2; \
			printf '%s\n' "$$skipped" >&2; \
			exit 1; \
		fi; \
		for evidence in 'served: HTTP/1\.1 200 OK' 'retired pid .* respawned pid'; do \
			printf '%s\n' "$$out" | grep -qE "$$evidence" || { \
				echo "rs-test-php-image: the $$label run printed no '$$evidence' line" >&2; \
				exit 1; \
			}; \
		done; \
		echo "rs-test-php-image: $$label run served, crashed and recovered, no leg skipped"; \
	done

fast-check: guard rs-fmt rs-lint rs-doc rs-test py-lint plan-validate universal-smoke ## Fast per-slice Rust + Python + plan check

check: guard rs-fmt rs-lint rs-doc rs-test test-critical py-lint docs-unit docs-check rs-no-python rs-no-default e2e-universal zed-check plan-validate ## Full unprivileged epic/release gate

# `make install` stages the §6.1 bundle: one universal `blastit` (the default
# `python,php,static` features), its `blastit-lsp`, every shim this host built,
# the licence and the pure-Python package, with two symlinks as the entry points.
# `scripts/check-bundle-layout.sh` then asserts the tree, so a host that skipped a
# shim cannot quietly ship a bundle that half works. Never run this against
# `~/.local` in a test: pass `PREFIX=<scratch dir>`.
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
	@echo "cargo build --release --locked -p blastit            # python, php, static"
	$(STANDALONE_ENV) $(CARGO) build --release --locked -p blastit
	@echo "cargo build --release --locked -p blastit-lsp"
	$(STANDALONE_ENV) $(CARGO) build --release --locked -p blastit-lsp
	$(MAKE) py-sapi
	$(MAKE) php-sapi
	@set -eu; \
	rm -rf "$(INSTALL_SHARE)"; \
	mkdir -p "$(INSTALL_LIBDIR)" "$(INSTALL_PYDIR)/blastit" "$(INSTALL_BINDIR)"; \
	install -m 0755 target/release/blastit "$(INSTALL_LIBDIR)/blastit"; \
	install -m 0755 target/release/blastit-lsp "$(INSTALL_LIBDIR)/blastit-lsp"; \
	for shim in target/py-sapi/libblastit_py*.so target/php-sapi/libblastit_php*.so; do \
		[ -e "$$shim" ] || continue; \
		install -m 0755 "$$shim" "$(INSTALL_LIBDIR)/"; \
	done; \
	install -m 0644 LICENSE "$(INSTALL_SHARE)/LICENSE"; \
	install -m 0644 python/blastit/*.py "$(INSTALL_PYDIR)/blastit/"; \
	ln -sfn "../share/blastit/lib/blastit" "$(INSTALL_BINDIR)/blastit"; \
	ln -sfn "../share/blastit/lib/blastit-lsp" "$(INSTALL_BINDIR)/blastit-lsp"; \
	scripts/check-bundle-layout.sh "$(PREFIX)"; \
	for shim in "$(INSTALL_LIBDIR)"/libblastit_*.so; do \
		[ -e "$$shim" ] || continue; \
		if [ "$(CURDIR)/target/release/blastit" -nt "$$shim" ]; then \
			echo "install: WARNING: $$shim predates target/release/blastit: run \`make py-sapi php-sapi\` so the bundle matches the binary" >&2; \
		fi; \
	done; \
	py=0; php=0; \
	for shim in "$(INSTALL_LIBDIR)"/libblastit_*.so; do \
		[ -e "$$shim" ] || continue; \
		case "$$shim" in \
			*libblastit_py*) py=$$((py + 1)) ;; \
			*libblastit_php*) php=$$((php + 1)) ;; \
		esac; \
	done; \
	echo "install: capabilities: static always; python $$py shim(s); php $$php shim(s)"; \
	[ "$$py" -gt 0 ] || echo "install: note: no CPython shim was built, so WSGI views cannot boot here (make py-sapi prints why)"; \
	[ "$$php" -gt 0 ] || echo "install: note: no PHP shim was built, so [route.php] cannot boot here (make php-sapi prints why)"; \
	echo "install: python package: $(INSTALL_PYDIR)/blastit (it belongs to this binary, not to your interpreter)"; \
	echo "install: no pip needed:  PYTHONPATH=$(INSTALL_PYDIR) python -m blastit check"; \
	echo "install: next: in a project with a Blastfile, run  $(INSTALL_BINDIR)/blastit dev"; \
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

# `docs/install.sh` (`MB.42.3.2`): parsed as POSIX `sh` in `docs-unit`, and its
# contract suite (fixture HTTP server on loopback, scratch prefixes, no root, no
# package install) in `fast-check`. Both rules are appended here so the wiring is
# one block; `fast-check`/`docs-unit` get a prerequisite-only rule, no recipe.
.PHONY: install-test install-sh-syntax

fast-check: install-test
docs-unit: install-sh-syntax

install-sh-syntax: ## Parse docs/install.sh as POSIX sh (and dash when present)
	sh -n docs/install.sh
	if command -v dash >/dev/null 2>&1; then dash -n docs/install.sh; else echo "install-sh-syntax: dash not installed, skipped"; fi

install-test: ## Prove docs/install.sh contract against a local fixture (no root, no network)
	sh scripts/test_install.sh
