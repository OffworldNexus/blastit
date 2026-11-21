# MB.3.47 — `critical_php_embed`'s CI-coverage note is true again

## Summary

`crates/blastit/tests/critical_php_embed.rs` lines 48–73 (was 48–58): the
`# CI coverage` section no longer says this suite has no CI witness. It now names
the job (`.github/workflows/php-sapi.yml`'s `shim`), the command
(`make rs-test-php-image`, which is `cargo test -p blastit --locked --no-run`
over `Makefile`'s `PHP_SUITE_ARGS` on the runner plus the binaries executed in
the `php-sapi-e2e` image), the evidence the `guard: the pair legs must serve,
not refuse` step demands, the honest host-gated/always-run split, and the
trigger half (the gate is the PR into `develop`).

**Documentation only**: `git diff` on the file shows `//!` lines and nothing
else; no test, assertion, fixture, constant, `#[ignore]` or skip construct was
added or touched. The `cc` requirement paragraph and `Boot::launch`'s free-port
hand-off were left alone.

## Evidence

### The new paragraph (read back from the file)

```text
//! # CI coverage (`MB.3.13.4` review, closed by `MB.3.39`)
//!
//! `make check` runs on a host without PHP, so the real-pair legs can only take
//! their refusal branch there. The served half needs a NTS `libphp` plus the
//! shim, and `.github/workflows/php-sapi.yml`'s `shim` job has both: `make
//! rs-test-php-image` builds these binaries on the runner (a `--no-run` `cargo
//! test -p blastit --locked` over `Makefile`'s `PHP_SUITE_ARGS`, i.e. `--test
//! critical_php_backend --test critical_php_embed`) and runs them in the
//! `php-sapi-e2e` image against its `libphp8.4.so` and shim. A `guard: the pair
//! legs must serve, not refuse` step then fails that job if either suite prints
//! its no-pair line (`refused (no usable libphp + shim on this host` or
//! `refused: … no usable NTS libphp was found`) or if the served evidence is
//! missing: `served: HTTP/1.1 200 OK` (the backend suite) and `retired pid …
//! respawned pid` (the containment leg here). The trigger is `push` or
//! `pull_request` on `develop` (plus `workflow_dispatch`), so a release-branch
//! push is not covered; the PR is the gate.
//!
//! So this job is the CI witness for `no_libphp_is_mapped_above_the_view_worker`,
//! the containment legs (`an_exit_is_contained_and_the_next_request_is_served`,
//! `a_fatal_is_contained_and_the_next_request_is_served`,
//! `a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time`),
//! `a_blocking_script_is_charged_cpu_time_not_wall_time` and
//! `a_form_body_reaches_the_front_controller` (plus the sibling suite's served
//! leg), while the stub-refusal and probe legs, the provenance legs and the
//! `egress` twin run everywhere, and `make rs-test-php` is the same pair legs on
//! a host that has the pair.
```

### What each claim was verified against

`.github/workflows/php-sapi.yml`:

```yaml
43:   shim:
44:     name: build libblastit_php8.4.so and load it
93:       - name: make rs-test-php-image (critical_php_backend + critical_php_embed on the real pair)
98:           make rs-test-php-image 2>&1 | tee "$RUNNER_TEMP/php-pair.log"
122:      - name: 'guard: the pair legs must serve, not refuse'
126:          if refuses=$(grep -nE \
127:                '^refused( \(no usable libphp \+ shim on this host|: .*no usable NTS libphp was found)' \
133:          for evidence in 'served: HTTP/1\.1 200 OK' 'retired pid .* respawned pid'; do
134:            if ! grep -qE "$evidence" "$log"; then
```

and the trigger it runs under:

```yaml
24: on:
25:   push:
26:     branches: [develop]
27:   pull_request:
28:     branches: [develop]
29:   workflow_dispatch:
```

`Makefile`:

```make
526: PHP_SUITE_ARGS := --test critical_php_backend --test critical_php_embed
551: rs-test-php-image: ## Docker: the same two PHP legs, run against the php-sapi-e2e image's pair (what CI runs)
558: 	echo "cargo test -p blastit $(PHP_SUITE_ARGS) --no-run   # build here, run in the image"; \
560: 	for binary in $$(CARGO_TERM_COLOR=never $(RUST_ENV) $(CARGO) test -p blastit --locked $(PHP_SUITE_ARGS) --no-run 2>&1 \
574: 		docker run --rm --user "$$(id -u):$$(id -g)" \
576: 			-e BLASTIT_LIBPHP="$(PHP_SAPI_IMAGE_LIBPHP)" \
577: 			-e BLASTIT_PHP_SHIM_DIR="$(PHP_SAPI_IMAGE_SHIM_DIR)" \
546: rs-test-php: php-sapi ## Real-pair PHP legs: critical_php_backend + critical_php_embed
```

Evidence provenance, checked rather than assumed: `served: …` is printed by the
**sibling** suite (`critical_php_backend.rs:616–620`, `println!("served: {} …
cpu_ns=…")` with the response's status line), and `retired pid … respawned pid`
by this one (`critical_php_embed.rs:2414–2419`, in `containment()`) — the guard
greps the single `tee`'d log of both binaries. Six of the 19 tests here call
`host_php()` (`no_libphp_is_mapped_above_the_view_worker`, the three containment
legs, `a_blocking_script_…`, `a_form_body_…`); the other thirteen run on any
host, which is the split the second paragraph states.

### Verification commands

```console
$ cargo test -p blastit --locked --test critical_php_embed
running 19 tests
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.53s

$ make critical-skip-check
protected critical tests hold no skip construct

$ uv run --no-project tools/critical_guard.py --base 36b8aad --node MB.3.47
critical-guard: 36b8aad..working tree, 5 changed file(s)
node MB.3.47: paths ['crates/blastit/tests/critical_php_embed.rs']
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/critical_php_embed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.3.47

$ make check
...
clippy      -p blastit{,-view} (standalone)
env -u PYO3_PYTHON -u PYTHONHOME cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.36s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

Comment-only proof:

```console
$ git diff --stat
 crates/blastit/tests/critical_php_embed.rs | 35 +++++++++++++++++++++---------
$ git diff -- crates/blastit/tests/critical_php_embed.rs | grep -E '^[+-]' | grep -vE '^[+-]{3}' | grep -v '^[+-]//!'
(no output)
```

## Files

- `crates/blastit/tests/critical_php_embed.rs` — module doc, lines 48–73
  (`//!` only).

## Follow-ups

1. `crates/blastit/tests/critical_php_backend.rs:36–38` (not in this node's
   `paths`) says: *"The real load — `make php-sapi-e2e` in a container with the
   packaged `libphp-embed` — is CI's proof; this suite proves the composition
   root wires the worker on a host that builds it."* Checked: **nothing in it is
   false** (the shim probe is CI's proof of the real load, and the suite does
   prove the wiring on a host that builds it), so no edit is required and none
   was made. It is merely **incomplete** since `MB.3.39` — the suite also runs
   against the image's pair in the `shim` job. If a curator wants it exact, the
   same one-clause addition ("and `make rs-test-php-image` runs it on the
   image's pair") needs a node with that file in `paths`.
2. `Boot::launch`'s free-port hand-off (`AUDIT-MB.3.13-CLOSURE.md` R4, the ~1-in-12
   noise-red) is untouched, as this node's `# Notes` requires; it is still
   `MB.3.40` follow-up 4's to fix.
3. Scope discipline: the `# CI coverage` heading lost its "follow-up for the CI
   owner" tag because `MB.3.39` closed it; no other section of the file was
   reviewed or reworded.
