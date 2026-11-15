# Result MB.3.62

## Summary

Closed both MB.3.61 review MINORs. Both were "a claim that can drift from what is proved".

**(a) The two-package image's caption now says what the helper demonstrates.** Reworded rather than
adding a boot: `e2e/php-sapi/Dockerfile.two-package` builds only the two suites and the shim (no
`blastit` binary), so a real `blastit dev` there means a full musl workspace build plus a manifest and
a curl — not "a few lines", and unverifiable here without building that image. The caption now states:
the image proves the suites' **probe order on the real Alpine layout** plus **boot agreement with the
probe** (each suite hands the boot `BLASTIT_LIBPHP=<probe's resolved path>`), and that stage 0's
*unset*-override walk over `PhpSearch::lib_dirs` is **unit-tested** (`stage0.rs`'s
`two_embed_packages_serve_from_the_newer_library`, injected directories), **not** image-tested. Also
made the image's premise an assertion: the helper now exits 1 if `BLASTIT_LIBPHP` is set, because then
the suites' probe would never walk the host's directories at all. The stale "the boot's own
`manifest.warning` proves which one" comment and the `grep` failure sentences were corrected too (the
`served:` line is the *probe's* answer; the boot's agreement is asserted inside each suite).

**(b) The probe table can no longer drift.** New `crates/blastit/tests/common/libphp_probes.rs` reads
`crates/blastit/src/stage0.rs` at compile time (`include_str!`) and compares it, entry by entry, with
the `PROBES` table each suite carries — name, order, and the `Option<&str>` per-minor constraint
including the final unconstrained `libphp.so`. `PROBES` was hoisted to module scope in both suites
(unchanged data) so the test can see it, and the "cannot disagree" doc comment now says it is a copy
that `the_probe_table_matches_stage0` checks. The checker is not a rubber stamp: its own tests prove
it errors on an unreadable/empty declaration and reports `entry 3` (dropped `Some("php84")`),
`entry 12` (fallback made per-minor) and `entry 0` (two entries swapped) on the **real** declaration
mutated in memory. A **red run** of the drift assertion against a drifted copy is pasted below.

No assertion was weakened; `#[ignore]` was not added; discovery behaviour in `stage0.rs` is untouched.

## Evidence

### Acceptance 1 — `cargo test -p blastit --locked --test critical_php_embed`

```console
$ cargo test -p blastit --locked --test critical_php_embed
test a_world_writable_manifest_root_refuses_a_production_boot ... ok
...
test libphp_probes::tests::a_changed_entry_is_reported_at_its_index ... ok
test libphp_probes::tests::an_unreadable_declaration_is_an_error ... ok
test libphp_probes::tests::the_declaration_reads_as_thirteen_ordered_probes ... ok
test the_probe_table_matches_stage0 ... ok
test a_sleeping_bailout_is_charged_its_wall_time_and_not_its_cpu_time ... ok
test a_fatal_is_contained_and_the_next_request_is_served ... ok
test an_entry_rewritten_in_place_is_refused_and_a_restart_serves_the_new_file ... ok
test no_libphp_is_mapped_above_the_view_worker ... ok
test an_egress_route_lets_the_probe_open_a_socket ... ok
test an_exit_is_contained_and_the_next_request_is_served ... ok

test result: ok. 25 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.78s
```

### Acceptance 2 — `cargo test -p blastit --locked --test critical_php_backend`

```console
$ cargo test -p blastit --locked --test critical_php_backend
running 9 tests
test libphp_probes::tests::an_unreadable_declaration_is_an_error ... ok
test libphp_probes::tests::the_declaration_reads_as_thirteen_ordered_probes ... ok
test the_probe_table_matches_stage0 ... ok
test libphp_probes::tests::a_changed_entry_is_reported_at_its_index ... ok
test check_reaches_the_same_discovery_gate ... ok
test a_static_only_manifest_is_not_refused_by_this_gate ... ok
test a_blastit_libphp_that_is_not_a_library_refuses_the_boot ... ok
test a_php_route_is_served_or_the_boot_refuses_naming_the_package ... ok
test a_symlinked_blastit_libphp_is_resolved_before_it_is_served ... ok

test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s
```

The shared module's tests run inside **both** binaries (they are `#[cfg(test)]` items of the
`#[path = "common/libphp_probes.rs"]` module each suite includes), so the oracle covers both copies:

```console
$ cargo test -p blastit --locked --test critical_php_embed -- --list | grep probe
libphp_probes::tests::a_changed_entry_is_reported_at_its_index: test
libphp_probes::tests::an_unreadable_declaration_is_an_error: test
libphp_probes::tests::the_declaration_reads_as_thirteen_ordered_probes: test
the_probe_table_matches_stage0: test
```

### The drift test goes red (pasted) — proof it is not vacuous

`stage0.rs` copied to `/tmp/mb362-stage0-drifted.rs` with **one** entry changed, the embed suite's
`include_str!` pointed at the copy for one run, then restored:

```console
$ cp crates/blastit/src/stage0.rs /tmp/mb362-stage0-drifted.rs
$ sed -i '0,/per_minor_dir: Some("php84"),/s//per_minor_dir: None,/' /tmp/mb362-stage0-drifted.rs
$ diff <(sed -n '495,555p' crates/blastit/src/stage0.rs) <(sed -n '495,555p' /tmp/mb362-stage0-drifted.rs)
19c19
<         per_minor_dir: Some("php84"),
---
>         per_minor_dir: None,

$ cargo test -p blastit --locked --test critical_php_embed the_probe_table_matches_stage0
running 1 test
test the_probe_table_matches_stage0 ... FAILED

---- the_probe_table_matches_stage0 stdout ----

thread 'the_probe_table_matches_stage0' (3076918) panicked at crates/blastit/tests/critical_php_embed.rs:895:9:
the suite's probe table has drifted from stage0.rs's `const LIBPHP_PROBES`: entry 3 is ("libphp.so", None) in stage0.rs and ("libphp.so", Some("php84")) in the suite
  stage0.rs: [("libphp8.4.so", None), ("libphp-8.4.so", None), ("libphp84.so", None), ("libphp.so", None), ("libphp8.3.so", None), ("libphp-8.3.so", None), ("libphp83.so", None), ("libphp.so", Some("php83")), ("libphp8.2.so", None), ("libphp-8.2.so", None), ("libphp82.so", None), ("libphp.so", Some("php82")), ("libphp.so", None)]
  suite:     [("libphp8.4.so", None), ("libphp-8.4.so", None), ("libphp84.so", None), ("libphp.so", Some("php84")), ("libphp8.3.so", None), ("libphp-8.3.so", None), ("libphp83.so", None), ("libphp.so", Some("php83")), ("libphp8.2.so", None), ("libphp-8.2.so", None), ("libphp82.so", None), ("libphp.so", Some("php82")), ("libphp.so", None)]

failures:
    the_probe_table_matches_stage0
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 24 filtered out; finished in 0.00s
```

Restored afterwards (the green reruns above are from the restored file; `grep` confirms
`include_str!("../src/stage0.rs")` is back at line 894).

### Protected-test authorization

```console
$ uv run --no-project tools/critical_guard.py --base 71dcaf8 --node MB.3.62
critical-guard: 71dcaf8..working tree, 6 changed file(s)
node MB.3.62: paths ['crates/blastit/tests/**', 'e2e/php-sapi/**']
  M  crates/blastit/tests/critical_php_backend.rs  [authorized by crates/blastit/tests/**]
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/**]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.3.62
```

### The e2e helper's new premise assertion runs

```console
$ sh -n e2e/php-sapi/run-two-package.sh && echo "sh -n: ok"
sh -n: ok
$ BLASTIT_LIBPHP=/usr/lib/libphp8.4.so sh e2e/php-sapi/run-two-package.sh; echo "exit=$?"
two-package: FAIL — BLASTIT_LIBPHP is set (/usr/lib/libphp8.4.so), so the suites'
  probe would not walk the host's directories at all
exit=1
$ sh e2e/php-sapi/run-two-package.sh; echo "exit=$?"     # unset, on this non-Alpine host
two-package: the host's own layout
ls: cannot access '/usr/lib/libphp82.so': No such file or directory
ls: cannot access '/usr/lib/php84/libphp.so': No such file or directory
exit=2
```

(The image itself was **not** built or run here: it is an Alpine musl image built by hand
(`docker build -f e2e/php-sapi/Dockerfile.two-package ...`) and is not a `make check` or
`platform_matrix.py` leg. Only the script's syntax and its new guard were executed. The caption edit
adds no claim the suites do not already assert.)

### Acceptance 3 — `make check`

```console
$ make check
...
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims; reaches: served 4 of 12 shims, 0 probed, 2 refused, 6 unbuilt; 8 served once the aarch64 legs run; 0 leg(s) dropped
UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py
workflow-check: rule 15 self-test ok — `make standalone` is a host build
workflow-check: ok — 364 checks over 6 workflows and 1 actions, 0 problem(s)
UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 29 cases passed (workflow=14, matrix=11, wheel-tag=4)
uv run --no-project tools/plan.py validate
plan OK
make check exit=0
```

## Files

- `crates/blastit/tests/common/libphp_probes.rs` (**new**) — `probes_in` / `assert_matches`, the
  `include_str!`-driven drift check and its negative self-tests.
- `crates/blastit/tests/critical_php_embed.rs` — `#[path]`-included module, `PROBES` hoisted to module
  scope (data unchanged), `the_probe_table_matches_stage0`, doc comment no longer claims the probe and
  the boot "cannot disagree" (says it is a copy the drift test checks).
- `crates/blastit/tests/critical_php_backend.rs` — same three edits.
- `e2e/php-sapi/run-two-package.sh` — caption reworded to "probe order + boot agreement, not stage 0's
  unset-override walk"; `BLASTIT_LIBPHP`-set guard; corrected `served:`-line/grep comments and failure
  sentences; closing `ok` line no longer implies the boot walked the directories.
- `e2e/php-sapi/Dockerfile.two-package` — header and "witness" paragraph reworded to match (the boot is
  handed the probe's path; the unset-override walk is unit-tested).
- `cargo fmt --all` was run; the two suite diffs contain no unrelated formatting churn.

## Follow-ups

- **`dirs` is still a hand copy.** Each suite's `libphp_path` also hard-codes the library directory list
  (`/usr/lib/<triplet>`, `/usr/lib64`, …, `/usr/lib/php84`, …) that `stage0.rs::library_dirs()` builds,
  and `stage0.rs`'s `LIBPHP_CANDIDATES` is likewise not compared with `PROBES`. The same drift test
  could cover both (parse `library_dirs()`'s literals; assert `PROBES`' names are exactly
  `LIBPHP_CANDIDATES` plus the per-minor `libphp.so` insertions). Not needed for MB.3.62's finding.
- **The two-package image is still proven by hand only.** `e2e/php-sapi/Dockerfile.two-package` is not a
  `scripts/platform_matrix.py` leg and no workflow runs it, so the Alpine two-package layout is not
  re-proved on CI. Either declare it (a leg would need a runner with Docker + a musl build) or accept
  that it is a manual witness — worth deciding before the claim is quoted anywhere else.
- **Lesson worth folding into `docs/knowledge/LESSONS.md`** (left unwritten: that file is outside this
  node's `paths`): a test table that must mirror production data needs `include_str!` + a mini reader
  **and** a negative self-test on the real declaration mutated in memory — otherwise the green
  assertion passes for two empty lists, and a text-level `grep` over the source would also match a
  commented-out entry; a `#[path = "common/…"]`-included module's `#[cfg(test)]` items run inside every
  including integration-test binary, which is how one oracle covers both copies.
