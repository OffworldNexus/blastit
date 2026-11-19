# MB.41.3 — architecture / security / test review

VERDICT: **FAIL** (2 MAJOR: two fences are `example`/`static`-accounted with reasons
that are factually false, so documented commands the gate can and does run
elsewhere are never executed and a doc regression passes silently).

Base: `ba347dd1f5e495217a8af94f9b1609d3844ec510`. Reviewed the whole tree, not the
diff only.

## Commands run (evidence)

```
cargo test -p blastit-manifest --test docs_fences
  test result: ok. 13 passed; 0 failed; ... finished in 0.07s
make docs-unit docs-test
  117 passed ... ; 59 fences: 32 executed, 27 accounted, 0 failed · 30.3s
uv run --no-project tools/critical_guard.py --base ba347dd… --node MB.41.3
  no change to a protected critical test
make -n rs-test-php-image            # parses (rc=0); exactly 2 real `docker run`s, both scoped
target/debug/blastit fmt --check     # on docs/frameworks/php/index.md's own Blastfile → exit 0
```

## [BLOCKER]

None.

## [MAJOR]

### 1. `python/tests/runnable_docs/inventory.toml:182-185` — `docs/frameworks/php/index.md` fence 2 is accounted instead of executed, with a false reason

The fence is `$ blastit fmt --check` alone (`docs/frameworks/php/index.md:93-96`), the
page calls it "the gate", and its own prose says it "needs no PHP at all". The
inventory says "The gate builds no PHP project fixture to format", which is not a
blocker: `fmt --check` needs no project fixture, no entrypoint and no `libphp`.

Proof, with the binary this gate already uses:

```
$ target/debug/blastit fmt --check          # cwd holds the page's own text title="Blastfile"
rc=0
```

The inventory contradicts itself about the same command: `docs/blastfile.md` fence 6's
reason states "`fmt --check` parses a PHP manifest with no libphp", and `docs/blastfile.md`
fence 1 really runs `uv run blastit fmt --check`. But no fence anywhere runs `fmt --check`
against a page's **own** manifest — only against the `shop` fixture — so the PHP page's
manifest can silently stop being canonical (`docs_fences` parses, never checks
canonicity) and `make docs-test` stays green. That is exactly the "doc regression now
passes silently" case.

Fix (small, uses existing helpers): add `mode_fmt_check(fence, spec)` —
`root, env = self.prepare(fence, spec, manifest=_page_manifest(fence))`, run the
fence's command with `self.run_split`, require `code == 0` and `self.expect(command,
code, stdout+stderr, "exact")`, then set `docs/frameworks/php/index.md` fence 2 (and
the `fmt --check` half of `docs/blastfile.md` fence 6, or split that fence) to
`mode = "fmt-check"` with `fixture = "empty"`. If that is refused, the reason must name
the real blocker, not a project fixture that is not needed.

### 2. `python/tests/runnable_docs/inventory.toml:297-300` — the landing page's Static & SPA tab is `example` with a fabricated blocker

Reason: "the launcher needs the release archive this gate does not unpack". That is
false. This same run executed the same commands:

```
PASS  docs/frameworks/static-and-spa.md  fence 1  dev          served on 127.0.0.1:8080
PASS  docs/local.md  fence 4  dev  (fixture static-site)       served on 127.0.0.1:8080
PASS  docs/frameworks/static-and-spa.md  fence 5  check-roots  every root audited
PASS  docs/local.md  fence 7  check-roots                      every root audited
```

`docs/index.md:88-92` is `$ blastit dev` + `$ blastit check` on the same `static-site`
fixture; the venv `blastit` binary stands in for the archive everywhere else. So the
front page's banner (`http · scheduler · dbproxy · 0 routes`) and its root-audit
promise are asserted nowhere: `mode_dev` requires exactly one server command,
`mode_dev_curl` a dev+curl pair, `mode_summary` would `assert_clean` a mount, and the
fence was parked in `example` instead of getting the ~15-line mode it needs.

Fix: add `mode_dev_roots` (start `blastit dev` via `start_dev`, `self.expect(server,
None, dev.stdout, "exact")`, stop, then run the remaining `blastit check`
command(s) with the `check-roots` assertions: exit `0`, and for `--json`
`ok is True` and `routes.total == 0`) and use it here. If it stays `example`, the
reason must say "mixed dev+check fence has no mode yet", never a nonexistent
archive dependency.

## [MINOR]

1. `crates/blastit-manifest/tests/docs_fences.rs:735` — the new broken-tab test asserts
   `message.contains("fixture.md:") && message.contains("does not boot")`, but
   `describe_failure` emits both strings unconditionally (`docs_fences.rs:459-462`).
   The assertion is tautological once `expect_err` succeeds, so the test would pass for
   the wrong parse error. Assert the diagnostic names the offending line/directive
   (`not_a_directive`), e.g. `message.contains("not_a_directive") || message.contains("keyword")`,
   and keep `contains("does not boot")` as the outer contract.
2. `crates/blastit-manifest/tests/docs_fences.rs:226-282` — de-indent by the opening's
   own width is correct for 4- and 8-space tab bodies (`leading_width`/`deindent` agree
   on ASCII space/tab; `closes_fence` still requires the trimmed line to be marker-only),
   but the new contract is not pinned for the two shapes the brief names: a fence inside
   a list item (opening indented, body at a *deeper* relative indent that must survive)
   and a longer fence nested inside a shorter one. `an_eight_space_indented_document_is_de_indented_once`
   covers admonition nesting only. Add those two cases so a later "normalise everything"
   edit cannot silently re-indent documents.
3. `python/tests/runnable_docs/harness.py:1547-1590` (`mode_check_roots`) — never calls
   `self.expect(...)`, so if any of the four fences it now owns gains a documented line
   (or a wrong one) nothing compares it; also `if not commands: raise` is dead
   (`_require_commands` already refuses an empty fence). Add
   `self.expect(command, code, stdout + stderr, "exact")` plus "stderr empty on a clean
   audit". Same mode is otherwise sound (`run_split` applies `translated`).
4. `python/tests/runnable_docs/harness.py:717-725` — `prepare` now branches on the *page
   prose* (`"uv init" in command.text`) and, for `docs/index.md` fence 1
   (`summary`, `shared=True`), leaves `UV_PROJECT_ENVIRONMENT` pointing at the shared
   `base/.venv`, so that fence's `uv add blastit` syncs/prunes the environment every
   other fence uses. Make it an inventory fact (`own-project = true`, rejected with
   `shared=True`) and call `base_env(..., shared=False)` for it, instead of text
   sniffing.
5. `crates/blastit-manifest/tests/common/mod.rs:435` (`DOCUMENTED_PAGES` 12 → 11) —
   only `docs/local.md` was removed, because that page no longer carries a
   `text title="Blastfile"` fence (measured: `PAGES` pins it at `(0, 12)`; every fence
   on it is `console`/`mermaid`/`json`). It is **not** vacuous: the other 11 pages are
   asserted by name, `docs_fences` still fails any document on `docs/local.md` (pin is
   `0`), and the §20 corpus still clears `ROUND_TRIP_FLOOR = 20` (15 fixtures + 17
   documents). But the protected corpus no longer covers the local-development page at
   all, so the *coverage* did shrink; record that in the invariant's doc comment (it
   currently only says "re-pinned") and note that `docs/local.md` shows no complete
   `Blastfile` anywhere — if that was not intended, restore a tabbed document instead of
   re-pinning.
6. `python/tests/runnable_docs/inventory.toml:346-349` — `docs/local.md` fence 9's reason
   says "`blastit dev --learn` and `blastit fmt` need a PHP project fixture and the host
   NTS `libphp`". `dev --learn` does; `blastit fmt` does not (see MAJOR 1). Narrow the
   reason so the audit does not teach a false blocker.
7. `crates/blastit-manifest/tests/docs_fences.rs:82-96` + `:512-535` — `PAGES` (counts)
   and `SHAPES` are two parallel 13-row tables kept in lockstep only by the runtime
   `found.len() == expected.len()` assert; `SHAPES` is also the only place a page is
   *permitted* to have zero documents without an explicit gesture. Merge into one row
   type `(page, Counts, &'static [Shape])` so a page cannot be half-pinned.
8. `Makefile:529-531, 534, 552` — correct and scoped: `make -n` parses, only the two
   `docker run`s of `rs-test-php-image` changed, the root/unprivileged asymmetry,
   `--cap-add SYS_PTRACE` and the inner `seccomp(SECCOMP_SET_MODE_FILTER)` tests
   (`a_probe_that_hangs_is_killed`, the `no_libphp_is_mapped_above_the_view_worker`
   witness) are untouched, and the comment is accurate. Residual: no test in this target
   asserts the *outer* profile, so a future edit could widen it to `--privileged`
   unnoticed; a custom profile allowing only `fanotify_init`/`fanotify_mark` would keep
   the rest of Docker's default, and one witness line pinning "outer seccomp is the only
   layer disabled" would make the loosening self-checking.
9. `README.md:67`, `docs/getting-started.md:100` — the banner now pins `/.*` (the compiled
   regex) while the page's own manifest says `"/*"`. Honest output, but the page teaches a
   pattern the reader never writes; `expect(..., "exact")` now locks the mismatch in. The
   RESULT already lists it as a follow-up — keep it visible, do not let the pin hide it.

## Answers to the review questions (measured, not trusted)

* **Pins are measured, not fitted.** Recounted by hand from the tree: README
  `1 doc / 6 illus` (7 fences), `docs/index.md` `3/5` (8), `docs/getting-started.md`
  `3/10` (13), `docs/local.md` `0/12` (12), `docs/frameworks/index.md` `0/1`,
  `python/index.md` `1/1`, `django.md` `1/3`, `flask.md` `1/3`, `php/index.md` `1/4`,
  `symfony.md` `1/1`, `laravel.md` `1/1`, `static-and-spa.md` `1/7`, `blastfile.md`
  `3/8` (11) → documents 17, illustrations 62, both equal to `TOTAL`. No page pin is
  looser than the old rule; `TOTAL` is additionally cross-checked against the sum of
  `PAGES`, and `STATEMENT_LISTS` losing `docs/ci.md`/`docs/local.md` is content
  (both pages now have zero plain `text` fences), not a rule change.
* **Classification stays exhaustive** (`Document` / `StatementList` / `Illustration`,
  `class_of` at `docs_fences.rs:305`), and a document on an unpinned page still fails by
  name (`unpinned_document_error`, `pinned_seen == PAGES.len()`, partition assert).
* **`SHAPES` is a real pin**: per document, in page order,
  `assert_eq!(shape_of(&manifest), *expected)` (`docs_fences.rs:603-616`); a tab whose
  document degrades from `ViewsAndMount` to `StaticSite` fails. `docs/getting-started.md`
  PHP tab is genuinely `ViewsAndMount` (`static "/" { root "public" }` + `view app`).
* **`test_docs_assets.py` 40/32/8 is real**: `measured` is built from `text_blocks()`
  over the tree and compared page-by-page against `EXPECTED_FENCES` for
  `set(measured) | set(EXPECTED_FENCES)`, so a page with `text` fences and no row fails;
  the 32/8 totals are asserted against `len(rows)`. `docs/local.md: ()` and the dropped
  `docs/ci.md` row are honest (no `text` fences left there).
* **`test_harness.py:48` +1 line is plumbing**, not a new assertion (the `CliHelp` field
  is required); the `fmt` branches of `_static_blastit`/`_serve_argv` are only covered by
  the integration run, no unit test.
