---
id: MB.3.4.3
title: "Harden the protected lockdown suite: SCRIPT_NAME query, total-byte cap via build, Params Debug, hop-by-hop names, Error/LockdownError Display"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: [MB.3.4.2]
paths: ["crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T12:36"
updated: "2026-11-14T12:36"
---

# Goal

Close the five surviving mutants and the two branch-inspection holes that live in the
**protected** lockdown suite, without touching `src/` behaviour: add five tests to
`crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` so the protected tier — not an
in-crate `#[cfg(test)]` block — owns them.

Source: `docs/testing/AUDIT-MB.3.4.md` §"Surviving mutants" #1, #2, #4, #5, #7 and §"The two
accepted MINORs" (a).

# Context

`make mutants-rust PKG=blastit-fastcgi` at `0f441af` left 9 survivors; five of them are
assertions that simply do not exist yet, and one is the `MB.3.4.2` accepted MINOR (a):

1. `lockdown.rs:515` `||` -> `&&` in `script_name_bytes` survives because no test puts a `?` in
   `script_name`. `malformed_pinned_paths_are_refused` puts the `?` on `script_filename` only.
   Under the mutant `build` emits `SCRIPT_NAME = "/index.php?x"` (§4.6.2 says `SCRIPT_NAME` is
   `?`-free).
2. `lockdown.rs:133` `Debug for Params -> Ok(())` survives: the impl exists only to satisfy `R18`
   ("a value carries cookies") and nothing asserts it. The other four log-safe `Debug` impls in
   the crate (`Pair`, `Record`, `Field`, `Head`) all have a positive+negative assertion.
3. `lib.rs:87` / `lib.rs:96` / `lockdown.rs:109` (`Error::Display`, `Error::source`,
   `LockdownError::Display`) survive: no test formats `Error` or `LockdownError`, while
   `RecordError` and `CgiError` `Display` *are* pinned.
4. **MINOR (a)**: the `R19` encoded <= 65 535 **total** cap is only asserted in
   `lockdown::tests::insert_enforces_the_pair_total_and_count_budgets`. The protected byte case
   (60 000-byte name + 8 192-byte value) trips the **pair** cap (65 531) first, so the total
   branch is never reached through the public `build`.
5. `lockdown::is_hop_by_hop` has nine names; only `connection` and `proxy-connection` are
   asserted. `keep-alive`, `proxy-authenticate`, `proxy-authorization`, `te`, `trailer`,
   `transfer-encoding`, `upgrade` are unasserted (§4.6.1 "hop-by-hop names dropped before
   derivation").

Do **not** change any assertion that already exists, and do not move or rename existing tests.

# Acceptance

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_lockdown --locked
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit-fastcgi --locked
test result: ok. 41 passed; ...        # unittests, unchanged
test result: ok. 30 passed; ...        # critical_fastcgi_lockdown
test result: ok. 1 passed; ...         # doc-test

$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.3.4.3
# no violation: this node's `paths` cover the file it modifies

$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-3-mut make mutants-rust PKG=blastit-fastcgi
348 mutants tested in ~60s: 4 missed, 326 caught, 18 unviable
# the 4 remaining are the two `|`->`^` equivalences, `Params::is_empty`, and
# lib.rs:96 `source -> None`; every one of #1,#2,#4,#5,#7 above must be caught
```

Use a **private** `MUTANTS_TMP` (LESSONS #26/#29) and never delete another campaign's scratch.

# Notes

- The five tests, in the file's existing style (one `///` line naming the rule, then the binary
  assertion):
  - `script_name_with_a_query_is_malformed` — `FastCgiPaths { script_name: "/index.php?x" }` ->
    `build` = `Err(LockdownError::Malformed)`; the accepted twin `/index.php` still builds.
  - `the_total_param_byte_budget_is_enforced_through_build` — eight headers of exactly 8 192
    value bytes (encoded pair = 1 + 4 + name + 8 192 ~= 8 200, i.e. **below** the 65 531 pair cap)
    exceed the 65 535 total and must be `Err(ParamsTooLarge)`; seven must build. Assert the
    boundary, not just "it errors".
  - `params_debug_prints_names_never_values` — `Cookie: session=secret` + `X-Foo: 1`;
    `format!("{params:?}")` contains `HTTP_COOKIE` and `names`, and does not contain `secret`.
  - `every_hop_by_hop_name_is_dropped` — all nine names, each in `Params::dropped()` and absent
    from `pairs()`.
  - `error_display_and_source_are_not_empty` — `Error::from(RecordError::Truncated).to_string()`
    equals the `RecordError` phrase, same for `CgiError`; `source().is_some()` for both; the four
    `LockdownError` phrases are non-empty and pairwise distinct.
- `Params::is_empty -> true` is deliberately **not** tasked: it has no production caller and a
  getter-only assertion is fluff (skill `testing` §3).
