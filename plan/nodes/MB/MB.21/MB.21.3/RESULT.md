# Result MB.21.3

## Summary

The Blastfile toolchain exists in `crates/blastit-manifest/src/blastfile/` and is
proven end to end: a bounded one-pass lexer, an LL(1) parser building the single
lossless CST arena of design §14.1, the path-pattern compiler and method
normalisation, the `Manifest` lowering with the model-equality oracle, the
canonical printer with `blastit fmt [--check] [PATH]`, the surgical
comment-preserving editor that `--learn` will use, and `docs_fences` reading
`Blastfile` documents.

Nothing is wired into the product's file discovery yet — that is `MB.21.4`'s
episode — so the retired reader still drives `blastit.toml` and every leaf landed
green on its own commit. Zero new dependencies and no `Cargo.toml`/`Cargo.lock`
change: the lexer, parser, printer and editor are hand-written.

The proof obligation is model **equality**, never a second file format: fifteen
fixtures (one per capability-map row and shape) are compared with the expected
`Manifest` built inside the test, and every row the design drops is asserted
absent.

## Evidence

### 1. Every child is `done` with command + output evidence in its `RESULT.md`

```console
$ uv run --no-project tools/plan.py tree --under MB.21.3
○ MB.21.3 [epic/ready] ⇠MB.21.2 Blastfile toolchain: …
  ● MB.21.3.0 [task/done] @implementer a1 …     ● MB.21.3.5 [task/done] @implementer a2 …
  ● MB.21.3.1 [task/done] @implementer a1 …     ● MB.21.3.6 [task/done] @implementer a1 …
  ● MB.21.3.2 [task/done] @implementer a2 …     ● MB.21.3.7 [task/done] @implementer a2 …
  ● MB.21.3.3 [task/done] @implementer a2 …     ● MB.21.3.8 [task/done] @implementer a1 …
  ● MB.21.3.4 [task/done] @implementer a2 …
```

Nine of nine `done`, each with its own `RESULT.md` carrying a command + output
tail per acceptance item. Commits: `ef783db` 3.0, `3600326` 3.7, `58c8ed9` 3.1,
`d230370` 3.2, `46e1b80` 3.3, `1d0d17b` 3.8, `8299a72` 3.4, `d652117` 3.5,
`c15feb9` 3.6.

### 2. `make check` exit 0 for the whole tree — no leaf needed a later leaf

```console
$ make check ; echo EXIT=$?
… 161 × "test result: ok", 0 × FAILED …
manifest-purity self-test: ok — 3 fixtures proved, 17 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

`make check` ran after every leaf's commit (and again on the final gate tree).
Two of the ten runs hit a pre-existing `blastit-view` timing flake, never a
Blastfile test; see `# Follow-ups`. `cargo fmt --all -- --check`, clippy with
`-D warnings` over `--all-targets`, and `rustdoc -D warnings` are part of it.

### 3. `cargo test -p blastit-manifest --locked`

```console
$ cargo test -p blastit-manifest --locked
… 32 × "test result: ok" (0 failed), 527 tests …
all doctests ran in 0.28s
```

### 4. The toolchain's shape

```console
$ ls crates/blastit-manifest/src/blastfile/
lexer.rs  lower.rs  mod.rs  parser.rs  pattern.rs  print.rs  syntax.rs  tokens.rs  write.rs
$ ls crates/blastit-manifest/tests/critical_blastfile_*.rs
critical_blastfile_body.rs       critical_blastfile_parity.rs    critical_blastfile_security.rs
critical_blastfile_entrypoint.rs critical_blastfile_parser.rs    critical_blastfile_traits.rs
critical_blastfile_errors.rs     critical_blastfile_patterns.rs  critical_blastfile_lexer.rs
                                                                 critical_blastfile_print.rs
$ grep -n 'blastfile' crates/blastit-manifest/src/lib.rs
95://! [`blastfile::read`] is the one reader of a `Blastfile`: a regular file of at
108:pub mod blastfile;
```

### 5. The model-equality oracle exists and is protected

`crates/blastit-manifest/tests/critical_blastfile_parity.rs` (20 tests, 887
lines) builds the expected `Manifest` field by field in the test and compares with
`Manifest::equivalent`; `assert_dropped_rows` proves the four dropped rows absent
(`version == 1`, `global_groups` empty, no per-view static or front-controller
block, `workers` `None`) and separate tests prove a `view` cannot carry one and
that `workers` is refused with `line:col`. No `from_toml_str`, no
`parse_and_validate`, no TOML twin appears anywhere in the leaf.

Fifteen fixtures under `crates/blastit-manifest/tests/fixtures/` are the corpus
contract: `CORPUS_SIZE = 15` is asserted by the printer's P1–P3 loop, by
`the_corpus_is_canonical`, and by the fence suite, so a dropped fixture fails.

### 6. Zero new dependencies

```console
$ git diff 3cf4eabc82cc999d2a58239a20c8de4cb04cd9ef --stat -- Cargo.toml Cargo.lock crates/blastit-manifest/Cargo.toml
(no output)
```

`grep -n '^toml' crates/blastit-manifest/Cargo.toml` still prints the retired
reader's dependency: `MB.21.4.5` removes it with the reader.

### 7. The retired reader is untouched and the existing suites still pass

`src/parse.rs`, `src/edit.rs`'s TOML patcher and every `blastit.toml` behaviour
are still live; `blastit.toml` parses and serves exactly as before. Four
pre-existing suites changed, each authorized by its leaf and recorded there:

| Suite | Leaf | Why |
|---|---|---|
| `critical_static_manifest.rs`, `critical_php_manifest.rs` | 3.7 | the `spa` field became `fallback`; every assertion kept |
| `docs_fences.rs` | 3.7 + 3.6 | one `StaticPolicy` literal; then the dual-opener reader |
| `critical_manifest_edit*.rs`, `edit_branches.rs`, `crates/blastit/tests/critical_learn_executor_blocks.rs` | 3.5 | rewritten to `Blastfile` fixtures, every branch ported |

`critical_guard.py --node <id>` reported "all protected change(s) authorized" for
3.5 (3) and 3.8 (1), and "no change" for the rest.

## Files

`crates/blastit-manifest/src/blastfile/{mod,tokens,lexer,syntax,parser,pattern,lower,print,write}.rs`,
`src/{model,routing,uid,validate,parse,edit,error,lib}.rs`; ten
`tests/critical_blastfile_*.rs`, seven `tests/blastfile_*.rs`,
`tests/{parse,uid,common/mod,fmt_command}.rs`, `tests/fixtures/*.blastfile` (15);
`crates/blastit/src/cli.rs`, `crates/blastit/tests/{critical_blastfile_fmt,fmt_command}.rs`;
`crates/blastit-scheduler/src/{route,static_asset}.rs` and two of its suites;
`scripts/check_manifest_purity.py` + its self-test and one `Makefile` line;
`docs/knowledge/LESSONS.md`.

## Follow-ups (hand-off; none of these is a gap in a delivered leaf)

### Deferred by design to `MB.21.4.2` (design §4.3's "Consumers" column)

Attempt 1 of 3.7 proved they cannot land before the runtime reads mounts: the four
per-view accessors are the model's only view→mount / view→front-controller
association, and four protected contracts read them. `MB.21.4.2`'s `paths` must
widen to take them.

- delete `Route::static_policy()`, `Route::with_static`, `Route::php()`,
  `Route::with_php`; rename `StaticOutcome::SpaLost` → `fallback_missing` (which
  also reaches `rewrite.rs`, `critical_static_resolve.rs`,
  `critical_static_assets.rs`);
- §9.5 `is_all_static`/`can_boot_without_app`, §9.6 `static_roots` over mounts
  (`validate.rs` plus `crates/blastit/tests/critical_static_cli.rs`'s
  `route "site"` sentences);
- §9.2's four-step precedence and `RouteInfo::static_forward`'s deletion;
- §9.4's per-mount `immutable` caps for a `Blastfile` mount, and §9.1's dotfile
  definition for `check_static_index` (it still refuses `.`).

### `MB.21.4.3` (re-spelling sweep)

- §13.2's six boot-warning sentences, §10.2's `body raw` production sentence and
  §11.3's `group` → `trait` noun. All three are pinned by
  `crates/blastit/tests/critical_raw_ingress.rs`, `tests/validate.rs` and
  `tests/mutation_gaps.rs`, so 3.7/3.8 could not re-spell them.
- `ManifestErrors`' `Display` join, `Location::View` printing "(index 0)",
  `InvalidApp`/`PhpEntryInvalid` echoing their value (§18), a
  `ManifestError::position()` for `cli.rs`, and the TOML patcher's own
  `EditError` sentences.
- the editor's entry-point rename (`BlastfileEdits` replaces `ManifestEdits`), the
  `dev_learn`/`check`/`audit_v1` call sites, and the end-to-end
  `blastit dev --learn` critical case (3.5 drives the editor directly until then).

### `MB.21.4.1`

`fmt`'s discovery through `config::manifest_path` (it is local today) and the
re-parse-before-writing that §22/S11 claims (R25's cap check is present).

### `MB.21.5.2` (the strict flip)

Flip `THE_RETIRED_OPENING_IS_GONE`; delete the TOML column; add §16.2's exhaustive
`docs/**` scan and the statement-list/illustration classes; self-test a valid
`blastfile` fence and the "missing document" direction; a plain `toml` fragment on
a pinned page must not slip through the retired-opening rule.

### `MB.21.5.3` (mutation audit) — small gaps measured here

- `scripts/check_manifest_purity.py`'s needle is a literal substring, so
  `use std::{env}` + `env::var` would evade R24;
- `UnrestrictedWithContent` is unreachable through the grammar;
- `write.rs`'s `first_difference` has no `unresolved_grants` arm, so the P4
  no-write case is blamed on "the top-level keys"; `BlastfileEdits::apply_to` has
  no direct test;
- `PgGrant::with_unrestricted` is `#[doc(hidden)] pub` but unused outside the
  crate; `Manifest::from_parts` still takes `version`/`global_groups`;
  `lower.rs` carries one unjustified `#[allow(clippy::type_complexity)]`;
- `cli.rs`'s `one_line_text` duplicates `error::clipped`, and
  `critical_blastfile_fmt.rs`'s write-failure test returns silently under uid 0;
- `fmt` reuses `TooLarge` for the *formatted* length; a comment hoisted out of an
  unwrapped list is not covered by §14.3's owner oracle.

### Design-text inconsistencies (accepted documents, not edited here)

§4.5 and §6.7 row 26 give two different `BlockTooDeep` sentences (§4.5's wins);
§6.3's prose says the two reserved words cannot be a name while its table scopes
`regex` to a pattern list (the table wins); §20/§21 assign
`critical_blastfile_parser.rs` and the fixture corpus to 3.2 while both need the
public entry point 3.8 lands; §10.3/§13.2 still name 3.0 for work that moved.

### Host and process

- **Pre-existing flake, needs its own leaf:** `crates/blastit-view/src/php.rs`
  `wait_for` allows 200 × 5 ms = 1 s for three frames, so
  `php::tests::a_runtime_that_retires_ends_the_loop_after_its_done` failed in 2 of
  10 `make check` runs under load. No leaf of this epic touches that crate.
- `H-010`–`H-013` are open, each answered by a lead decision recorded in the node
  logs (the 3.7 scope split; `read`'s two position-less sentences; the parser's
  one catch-all sentence; `error.rs` in 3.3's paths) — close them.
