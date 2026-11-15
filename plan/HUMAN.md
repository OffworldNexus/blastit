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

## H-005 [open] (node MB)

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

