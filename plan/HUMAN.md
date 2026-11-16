# HUMAN inbox

Requests for the project owner. Agents add items with `uv run --no-project tools/plan.py --agent <role> human add "..."`.

To answer, write under **Answer:**, save, and run `/human` (or tell the conductor "answered"). Never write raw secrets here — put credentials in `.env` and reply "added to .env".

## H-001 [closed] (node MA)

- asked: 2026-11-12T01:39 by conductor

Raise expensive_cap_usd in plan/BUDGET.json (currently $238.88 spent of $100 cap = 239%, status EXHAUSTED). The autopilot plugin now blocks strategist/architect/expert, so (a) the MA milestone gate cannot run, (b) core DX design acceptance cannot be escalated, (c) any expert rescue on hard Rust/Linux/WASM work is unavailable. I have re-scoped MA.1 to the cheap doc-writer/designer tier so the roadmap keeps moving, but MA/MB/MC/MD gates will need strategist. Please raise the cap (e.g. to $400) or confirm the roadmap should run gates on the cheap tier instead.

**Answer:**

## H-002 [closed] (node MA)

- asked: 2026-11-12T22:58 by lead

Install cargo-mutants on the dev host (or allow a one-off `cargo install cargo-mutants`): the MA milestone gate requires `make mutants-rust PKG=blastit-manifest` and `PKG=blastit-sqlpolicy` (>=75% killed), but the binary is missing so no mutation audit can run locally.

**Answer:** Allowed — run a one-off `cargo install cargo-mutants` and use the real mutation audit as MA gate evidence (owner answered 2026-11-13).

## H-003 [closed] (node MA)

- asked: 2026-11-13T03:12 by conductor

OpenRouter credits exhausted: every strategist/architect/expert request fails with 'requires more credits ... you requested up to 256000 tokens, but can only afford ~79-80k' on key c048f174. The MA (0.1.1) milestone gate is mandatory for the expensive tier, so with credits gone the roadmap cannot advance past MA even though MA's work is complete and green (make check + make e2e PASS at 4338f88). Please raise the key's weekly limit (or top up credits). Also still open: H-002 install cargo-mutants for the gate's literal mutation audit.

**Answer:** Add $100 to the budget — `expensive_cap_usd` raised from 100 to 200 (owner answered 2026-11-13).

## H-004 [closed] (node MA)

- asked: 2026-11-13T10:46 by conductor

Release v0.1.1 is ready: PR https://github.com/OffworldNexus/blastit/pull/62 (branch release/0.1.1, head 42bc8ac). Review and squash-merge it into develop on GitHub — merging validates CI, Docs, and Live DigitalOcean e2e on develop and automatically publishes v0.1.1 to PyPI and GitHub Releases.

Done already, no action needed from you: the hot-path performance work from PR #61 was backported into this branch (commit 341698a, conflict resolutions in blastit-http/conn.rs and blastit-scheduler/scheduler.rs, perf suites riding along), and both #60 and #61 are closed with comments pointing here. #60's benchmark/ harness was intentionally left out of 0.1.1 as dev tooling, not release behaviour; its branch is untouched.

**Answer:**

## H-005 [closed] (node MB)

- asked: 2026-11-14T00:55 by conductor

Release v0.2.0 is ready: https://github.com/OffworldNexus/blastit/pull/64 — branch release/0.2.0, CI 16/16 green (at 1e307d6: smoke root, pytest plugin, docs gate, e2e Django, e2e rootless, wheels 3.13/3.14/uv, sdist, fmt+clippy+doc, msrv, release guard, critical-test guards, build --strict). Review and squash-merge it on GitHub — merging validates CI, Docs and Live DigitalOcean e2e on develop and automatically publishes v0.2.0 to PyPI and GitHub Releases.

Scope: static asset serving with safe path resolution, SPA/front-controller rewrite precedence, the two-URL pooler presets (DigitalOcean managed pooler + PgBouncer), and the dev/check wiring. Two things to know when reading it: (1) the `smoke (root)` job now excludes `critical_static_cli` alongside `critical_dev_and_fail_closed`, because both assert what an *unprivileged* caller sees (root can list anything), and both run unprivileged in `test-critical`; the exclusion list is guarded against silent growth. (2) The branch also carries MB.3.1's FastCGI/php2shell research note under `docs/research/`, which is excluded from the public site — harmless in 0.2.0, but the PHP feature work itself continues on `release/0.2.1`.

**Answer:**

## H-006 [closed] (node MB.3.7)

- asked: 2026-11-15T10:02 by implementer

MB.3.7 (Measured, not fixed here — crates/** is out of its paths): ADR-0010 §1/§2 and MB.3.7's §7.3 sentence describe [route.php] entry as 'pinned at boot / the entry inode'; it is not. crates/blastit/src/php_view.rs holds PhpBinding.entry as a PathBuf and crates/blastit-php-sapi/c/blastit_php_sapi.c sets SG(request_info).path_translated and zend_stream_init_filename to that path per request, so PHP re-opens the script each time. Only libphp and the shim are fd-pinned (ADR-0010 §1). Measured by e2e/rootless/php_entry_swap.py: a symlink to a decoy outside the manifest dir fails closed with 500 (open_basedir; decoy 0 opens), a decoy INSIDE the manifest dir executes (200 + DECOY-RAN), and the status is 500 not 503. The guarantee that does hold is ADR-0010 §2's permission half (the route uid cannot chmod/create/unlink the tree) and that is what the check asserts. Decide: pin entry by descriptor like libphp/the shim, or correct the claim in ADR-0010 and the node text.

**Answer:**

## H-007 [closed] (node MB.3.7)

- asked: 2026-11-15T10:05 by lead

MB.3.7 filed H-006: [route.php] entry is re-resolved per request, not pinned by descriptor as ADR-0010 §3 claims. Measured: swapping public/ aside outside the manifest dir gives 500 (fail-closed), but a decoy INSIDE manifest_dir answers 200 DECOY-RAN. Decide: pin the entry by (st_dev, st_ino) at boot like libphp/the shim, or amend ADR-0010 §3 and the node's §7.3 to what the code does — and say whether the provenance gate (manifest_dir not writable by a route uid, MB.3.40) is considered sufficient mitigation.

**Answer:**

## H-008 [closed] (node MB)

- asked: 2026-11-15T16:28 by conductor

Release v0.2.0 is ready on branch release/0.2.0 (tip 73960fc). Review and squash-merge PR #64 into develop — merging validates CI, Docs and Live DigitalOcean e2e on develop and publishes v0.2.0 to PyPI + GitHub Releases. Scope: safe static serving (no .php source servable), SPA/rewrite precedence, pooler presets (DO managed + PgBouncer), embedded PHP via host libphp (no php-fpm, no fallback), portable Python-free binary, and the raw-ingress fail-open fix. Matrix: 8 wheels / 4 archives / 12 shims declared, 4 / 2 / 6 built+exercised on x86_64 (aarch64 CI-only).

**Answer:**

## H-009 [closed]

- asked: 2026-11-15T17:36 by implementer

MB.3.58 attempt 2: four edits this node cannot make (its paths are .github/**, scripts/**, e2e/**, Makefile), all of them measured in the legs' own images. The gate now prints the support fact (`scripts/platform_matrix.py --summary`: `served: 2 of 12`), but prose needs an owner.

1. `docs/frameworks/php.md` (the PHP support/install table) must say, not a footnote: PHP 8.3 and 8.4 are supported on Debian/Ubuntu; PHP 8.2 is BUILT BUT REFUSED, because the distro's `libphp8.2` exports no `php_version`, which `crates/blastit-php-sapi/src/loader.rs` requires before `php_module_startup` (a host-package fact, not a blastit bug — the shim's real-load probe still serves); on Alpine, 8.3/8.4 serve only when `BLASTIT_LIBPHP` names a versioned file, because Alpine's `/usr/lib/libphp83.so` and `/usr/lib/php84/libphp.so` carry no major.minor and the boot refuses ("no shim can be chosen"). Measured: 8.3/8.4 gnu `served: HTTP/1.1 200 OK` through the worker (5 + 21 tests each); ALPINE 8.3/8.4 `GET / -> app 200` with `/tmp/libphp8.4.so` copied from the distro file; 8.2 gnu and musl both `the loaded images have no php_version: Symbol not found`.

2. `plan/HUMAN.md` H-008 says "4 / 2 / 6 built+exercised on x86_64"; that reads as six working shims. Measured: 6 built and exercised at the probe level, and 2 of 12 SERVED (x86_64 gnu 8.3/8.4; 4 of 12 once the aarch64 gnu legs run). Please add: "of the 6 x86_64 shims, 2 serve `[route.php]` (8.3/8.4 gnu), 2 are refused (8.2 both libcs), 2 are probed only (Alpine 8.3/8.4 need a versioned BLASTIT_LIBPHP)".

3. `docs/knowledge/LESSONS.md` (the bullet starting "A \"directory only one uid can write\" fixture") still says `make rs-test-php-image` runs already-built binaries with `--user <host uid>:<host gid> -v $CURDIR:$CURDIR`. Since MB.3.58 the leg image builds the suites and the target runs them as the image's root (plus one unprivileged single-test run for the `PR_SET_DUMPABLE(0)` witness), so `target/tmp` is the image's own; the reason the container cannot use `/tmp` is unchanged.

4. `crates/blastit/tests/critical_php_embed.rs` lines ~44-47 ("# CI coverage ... closed by MB.3.39") still imply the runner builds the suites. They are now compiled inside the leg image (`e2e/php-sapi/Dockerfile`, `PHP_SAPI_SUITES=1`) and only where `matrix.suites` is true: gnu 8.3/8.4.

Separately, a crates-side limitation this node measured (crates/** is out of its paths): `stage0.rs`'s PHP discovery reads the minor from the libphp FILE NAME, so Alpine's own package paths are refused even though the pair serves. Reading the version from the ELF (or accepting `libphpXY.so`) is the fix if Alpine PHP is meant to be supported out of the box.

**Update (implementer, attempt 3).** The node's `paths` were widened to `docs/frameworks/php.md` and
`docs/knowledge/LESSONS.md`, so items **1 and 3 are done here** and there is nothing left for you on
them: `docs/frameworks/php.md` §1 now carries the support line (8.3/8.4 supported; 8.2 built but not
served, because `libphp8.2` exports no `php_version`; on Alpine, `libphp83.so` /
`/usr/lib/php84/libphp.so` are refused with `no shim can be chosen` and `BLASTIT_LIBPHP` must name a
versioned file), and the LESSONS bullet now describes the in-image build, the root run, the
unprivileged maps leg and the missing `CAP_SYS_PTRACE`. **Still yours:** item 2 (the H-008 sentence:
"of the 6 x86_64 shims, 2 serve `[route.php]`, 2 are refused, 2 are probed only" — the gate's exact
sentence is now `reaches: served 2 of 12 shims, 2 probed, 2 refused, 6 unbuilt; 4 served once the
aarch64 gnu legs run`, printed identically by `--check` and `--summary`) and item 4 (a two-line
comment in `crates/blastit/tests/critical_php_embed.rs`). The discovery limitation is now **MB.3.59**
(priority 1, `depends_on: [MB.3.58]`), which the owner ruled release-blocking: the Alpine naming
refusal and 8.2's missing `php_version` are one defect, and the minor must come from the library
rather than its file name.

**Answer:**

