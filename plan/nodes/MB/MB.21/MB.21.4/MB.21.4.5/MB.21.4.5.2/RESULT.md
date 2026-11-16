# MB.21.4.5.2 — RESULT

**Status:** done. Both protected suites now prove the two `Blastfile` constructs that
replaced their per-route tables: a top-level `static "<prefix>" { … }` mount and one
`entrypoint php "<path>"`. Every table-driven case asserts `manifest.mounts()[i].policy`
and `manifest.php()`; no case reads `route.static_policy()` or `route.php()`, so the
recorded follow-up that deletes that per-route pair can land without touching these
files. Every refusal moved from `Location::Route` to `Location::Mount` (or to the
parser's own position-bearing variant) and keeps its sentence.

## Attempt 2 — review fixes

The reviewer's three findings are all closed, and nothing was removed to close them:

1. **(MAJOR) control-byte exemplars restored to the boundaries.** The `\n`/`\t` samples
   are gone; every rule now carries a raw `\u{0}` (and `\u{1f}` where the family has room)
   as a real byte, so `byte < 0x20` is pinned at both ends. The old justification was
   wrong: `lexer.rs`'s `quoted` copies any byte that is not `"` or `\` verbatim, and
   `tokens::str_value` copies it on, so a `"…"` string carries NUL and `0x1F` unchanged.
   Other protected suites already do this (`critical_blastfile_entrypoint.rs` and
   `critical_blastfile_errors.rs` pass a raw `\u{1}` inside a string). See *Control-byte
   boundaries*.
2. **(MINOR) `fallback` asserts all six `fallback_rule` branches**: `""` and `a//b` (one
   or more segments), `a\b` (no backslash) and `a\u{0}b`/`a\u{1f}b` (control byte) join the relative,
   parent and dotfile cases, each with its exact variant.
3. **(MINOR) the mount without `root`** asserts the concrete
   `ManifestError::StaticRootMissing { at: Position::at(1, 1) }` again, with the rendered
   `1:1: …` line as the second assertion, as its prefix sibling does.

Follow-up PASS (three more MINORs, closed here): both `.php` `Hidden` pointers now name
`crates/blastit-scheduler/tests/critical_static_entry.rs::php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates`
(`critical_static_resolve.rs` has no `.php` row); the `fallback` family gained
`fallback "a\u{1f}b"` so it pins both ends of `byte < 0x20`; and the root/php-entry
comments now name `blastfile/lower.rs`'s `root_rule`/`check_php_entry` inline copies
instead of `validate.rs`'s `has_control_byte`.

## What changed

| File | Was | Is |
|---|---|---|
| `tests/critical_static_manifest.rs` | 15 tests over `[route.static]` tables read by `from_toml_str` | 15 tests over `static "<prefix>"` mounts read by `from_blastfile_str`/`parse_blastfile` |
| `tests/critical_php_manifest.rs` | 10 tests over `[route.php]` tables | 11 tests over one `entrypoint php` (the extra one is the `duplicate` family) |

`PAGE` is now a comment-marked `Blastfile` fixture (root/index/fallback/immutable) naming
`MB.21.5.1` as the leaf that rewrites the framework page to it. The docs page was not
read. The static page's four-step precedence walk is left to `critical_blastfile_parity.rs`
and is not duplicated here; this suite keeps the per-key grammar and the root audit.

## One line per family

`A` = accepted side, `D` = denied side, both present unless stated.

### `critical_static_manifest.rs`

| Family | Carried by | A + D | Evidence |
|---|---|---|---|
| `root` | `every_bad_root_names_the_rule_it_breaks` | A: 5 relative spellings (`public`, `./public`, `a/b`, `current/public`, `pub lic`). D: all 8 refusals (`""`, `/srv/public`, `/`, `../public`, `current/../../public`, `pub\lic`, and two raw control bytes, `\u{0}` and `\u{1f}`), each an exact `StaticRootInvalid` at `Location::Mount`, plus the rendered `mount "/" (index 0): …` line. | unchanged rules; the boundaries are pinned (see *Control-byte boundaries*) |
| `index` | `every_bad_index_names_the_rule_it_breaks` | A: 4 (`index.html`, `shell.html`, `docs/index.html`, `a b.html`). D: all 10 rules (`""`, `a//b`, `a/`, `/index.html`, `../index.html`, `.env`, `docs/.env`, `a\b`, raw `\u{0}`, raw `\u{1f}`), exact variants + the rendered `.env` line | both ends of `byte < 0x20` pinned |
| `immutable` | `every_bad_immutable_pattern_names_the_rule_it_breaks` | A: literal pattern, 32-pattern boundary, 256-byte boundary, one-item list. D: no leading `/`, empty, `..`, raw `\u{0}`, raw `\u{1f}`, 257 bytes, 33 patterns; exact variants + rendered line | unchanged caps, now per mount; this is the case that kills `has_control_byte`'s `byte < 0x0B` / `byte <= 0x1E` mutants |
| `fallback` | `a_fallback_is_its_own_file_and_a_dotfile_one_is_refused` | A: `fallback "shell.html"` beside `index "index.html"` (that file is **not** `index`), `fallback` equal to `index`, and `root ".output"` (dot-directory root stays legal). D: `.env` and `docs/.env` (dotfile segment), `/shell.html` (relative), `../shell.html` (`..`), and `index ".env"` beside that same dot-directory root, plus every remaining `fallback_rule` branch: `""` and `a//b` (`must be one or more segments`), `a\b` (`must not contain \`), `a\u{0}b` and `a\u{1f}b` (control byte) | this is the retired `spa` key's both-directions replacement (§9.3); `spa` itself has no spelling. All **six** `fallback_rule` branches are now asserted |
| `declarations` | `a_mount_that_never_reaches_the_app_may_not_declare_them` | A: a mount plus the catch-all view where `body`, `prefork`, `use` and a `postgres` grant are all live and the file validates with no warning. D: the same four words inside a `static` block are refusals naming the block's four directives | **denied half re-homed**: a mount has no route, so the rule is now the parser's (`StaticDeadDeclaration` is retired-reader-only — see Follow-ups). The old "no app ⇒ dead" arm has no subject: a mount never reaches an app at all |

Also kept, unchanged in intent: `the_pages_mount_parses_with_the_defaults_it_omits`
(defaults + the accepted twin), `a_mount_without_a_root_and_a_mount_without_a_prefix_are_refused`
(the retired "static block with no root" **and** "static route with no path" — the latter
now `static "" { … }` ⇒ `StaticMountPrefixInvalid`, plus both halves of the header being
unomittable), `an_unknown_static_directive_is_refused_not_dropped` (the retired unknown-key
rules `queries`/`tables`, now `listen` too), `every_static_directive_takes_one_string`
(the retired type table, with `spa`'s slot taken by `fallback` and `immutable`'s by the
list rule), `only_a_manifest_whose_every_route_is_static_needs_no_app`,
`a_store_named_static_can_never_be_granted_by_a_mount`, `validation_never_touches_the_filesystem`,
`a_listable_root_is_accepted_while_its_index_need_not_exist_yet`,
`every_unusable_root_is_named_and_none_stops_the_list`,
`an_unlistable_root_is_permission_denied`.

### `critical_php_manifest.rs`

| Family | Carried by | A + D | Evidence |
|---|---|---|---|
| `entry` | `php_entry_must_be_a_relative_php_path`, `the_php_messages_match_the_design_table`, `a_front_controller_is_not_checked_against_a_mount_root` | A: `index.php`, `public/index.PHP`; a controller inside *and* outside a mount root. D: empty, leading `/`, `\`, `..` (twice), raw `\u{0}` and raw `\u{1f}` control bytes, non-`.php`, and a missing path (the parser's refusal at the newline) | exact `PhpEntrypointInvalid` at `Position::at(1,1)`; both ends of `byte < 0x20` pinned; the "beneath the view's static root" rule is **dropped by §12.2** — the per-request `.php`/`.phtml`/`.phar` `Hidden` proof is `crates/blastit-scheduler/tests/critical_static_entry.rs::php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates` |
| `duplicate` | `a_file_declares_one_entrypoint` | A: one entrypoint. D: a second one, in both language orders ⇒ `DuplicateEntrypoint` naming the first line | new family for §4.3's "declared once" |
| `workers` | `workers_is_gone_and_no_file_sizes_a_worker_count` | D: `WorkersRemoved` at seven sites (top level, the `entrypoint` itself, a view, an endpoint, a mount, and `workers "4"` / `workers 4294967296`). A: every accepted file (php-only, php+view, php+mount+view) answers exactly `PhpPolicy { entry, workers: None }` — **no** spelling sizes a count from a file | the retired `0/65/u32::MAX` value table and its `1/4/64` accepted twins are replaced by the one refusal that names the removal (`ADR-0012` §12.3; the message carries no ADR number, per R13) |
| `can_boot_without_app` | `a_php_entrypoint_is_its_own_executor_for_the_boot_predicates` | A: php-only, static+php, and a controller with two views (one entrypoint serves every view). D: a WSGI view, an empty file | plus `EntrypointNotReached` for a controller no view reaches — the warning, not a missing app |
| `is_all_static` | same test | A: a static-only file. D: php-only, static+php, WSGI view, empty file | keeps its 0.2.0 meaning |

Also kept: `a_one_line_entrypoint_parses_validates_and_is_the_php_executor` (the policy,
and the `wsgi`/`php` kind table with no `fastcgi`), `listen_is_not_a_key_of_the_php_form`
(the retired unknown-key rule: `listen` is not a keyword, and a block after the path is not
part of the form), `connect_and_trace_cannot_reach_a_view` (now one refusal for every view,
`MethodCannotReachView`, with `GET, POST` accepted and `GET ⇒ HEAD` asserted),
`a_refusal_never_carries_the_php_entry_it_refused` (a DSN-shaped entry, and the
application-name slot), and `a_fallback_beside_a_front_controller_is_legal_now`
(the retired "`spa`/deep link never reaches the front controller" denial has no subject —
`fallback` is evaluated after the views; the precedence walk is `critical_blastfile_parity.rs`'s).

## Control-byte boundaries

`has_control_byte` is `byte < 0x20` (`validate.rs:841`), and `Blastfile` spells a raw byte
in a `"…"` string: the lexer's `quoted` copies any byte that is neither a quote nor a
backslash verbatim (`blastfile/lexer.rs`), and `tokens::str_value` copies it on. So NUL and
`0x1F` reach the predicates as real bytes, and each family pins them at the boundaries
instead of at `\n`/`\t` (which every wrong predicate would still refuse):

| Exemplar | Pins |
|---|---|
| `root "pub\u{0}lic"` and `root "pub\u{1f}lic"` | `blastfile/lower.rs` `root_rule`'s inline `byte < 0x20`, both ends |
| `index "a\u{0}b"` and `index "a\u{1f}b"` | `blastfile/lower.rs` `index_rule`'s inline `byte < 0x20`, both ends |
| `immutable "/assets/\u{0}*"` and `immutable "/assets/\u{1f}*"` | `validate.rs` `has_control_byte` (via `check_static_immutable`), both ends |
| `fallback "a\u{0}b"` and `fallback "a\u{1f}b"` | `validate.rs` `has_control_byte` (via `fallback_rule`), both ends |
| `php_entry "in\u{0}dex.php"` and `"in\u{1f}dex.php"` | `blastfile/lower.rs` `check_php_entry`'s inline `byte < 0x20`, both ends |

Checked by hand-mutation in a throwaway worktree at the same base (src never touched in the
shared tree): rewriting every copy to `byte < 0x0B`, to `byte <= 0x1E`, or to
`byte == b'\n' || byte == b'\t'` makes these tests fail (`< 0x0B` → `every_bad_immutable_…`;
`<= 0x1E` → `every_bad_root_…`, `every_bad_index_…`, `every_bad_immutable_…`,
`a_fallback_is_…`, `php_entry_must_…`; `== '\n' || == '\t'` → all of those). The
worktree was removed; `git worktree list` shows only the main checkout and this one.

## Acceptance evidence

```console
$ grep -rnE 'from_toml_str|toml::' crates/blastit-manifest/tests/critical_static_manifest.rs crates/blastit-manifest/tests/critical_php_manifest.rs
(no output, exit 1)

$ grep -rnE 'blastit[.]toml' crates/blastit-manifest/tests/critical_static_manifest.rs crates/blastit-manifest/tests/critical_php_manifest.rs
(no output, exit 1)

$ cargo test -p blastit-manifest --test critical_static_manifest --test critical_php_manifest
running 11 tests
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
running 15 tests
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
exit=0
```

`make critical-guard BASE=2ef786ce381dce05208d8d4d09a50f8eb4d7589b NODE=MB.21.4.5.2`
in the shared worktree still reports **four** violations, none of them this node's files —
sibling leaves are mid-flight in the same working tree (`critical_manifest_011.rs` from
`MB.21.4.5.1`, three `blastit-scheduler` suites from another leaf):

```console
critical-guard: 2ef786ce381dce05208d8d4d09a50f8eb4d7589b..working tree, 26 changed file(s)
  M  crates/blastit-manifest/tests/critical_manifest_011.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-manifest/tests/critical_php_manifest.rs  [authorized by ...critical_php_manifest.rs]
  M  crates/blastit-manifest/tests/critical_static_manifest.rs  [authorized by ...critical_static_manifest.rs]
  M  crates/blastit-scheduler/tests/critical_charge.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_ingress_enforcement.rs  [VIOLATION: not covered by any node path]
  M  crates/blastit-scheduler/tests/critical_rewrite.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 4 protected critical test(s) changed or deleted without a covering path
```

Proof that this node's change is clean on its own: the same command run in a temporary
detached worktree at the same base (`git worktree add --detach … 2ef786c`) carrying only
these two files:

```console
critical-guard: 2ef786ce381dce05208d8d4d09a50f8eb4d7589b..working tree, 2 changed file(s)
  M  crates/blastit-manifest/tests/critical_php_manifest.rs  [authorized by ...critical_php_manifest.rs]
  M  crates/blastit-manifest/tests/critical_static_manifest.rs  [authorized by ...critical_static_manifest.rs]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.21.4.5.2
exit=0
```

(The temporary worktree was removed with `git worktree remove --force`; `git worktree list`
shows only the main checkout and this one.) No assertion was deleted, skipped or weakened:
every refusal keeps a full-variant equality or its complete rendered line, every family
keeps an accepted twin, and this attempt only **added** cases (the `fallback` branches, the
`0x1f` twins) and restored the `StaticRootMissing` variant equality.

Extra gates run here:

```console
$ rustfmt --edition 2024 --check crates/blastit-manifest/tests/critical_static_manifest.rs \
      crates/blastit-manifest/tests/critical_php_manifest.rs
exit=0
$ cargo fmt --all -- --check
exit=0        # workspace-wide, no diff at the time of this run
$ cargo clippy -p blastit-manifest --all-targets --locked -- -D warnings
Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.41s
```

## Follow-ups

1. **`StaticDeadDeclaration`, `PhpDeadDeclaration` and `PhpWorkersInvalid` are now
   unreachable from a `Blastfile`.** `validate::check_static`/`check_php` walk *routes*, and
   a lowered `Blastfile` never sets `route.static_policy()`/`route.php()`. The three
   variants survive only for the retired reader and should be deleted with it in
   `MB.21.4.5.6`; this suite stops asserting them deliberately (the `declarations` and
   `workers` families above carry the rules at the parser). `MB.21.4.5.6` should delete the
   two now-dead route walks in the same commit, or `validate.rs` keeps code no input can
   reach.
2. **`validate::check_mounts`'s `bridged` closure** (skip a mount whose policy equals a
   route's) exists only for the retired reader's lowering and goes with it; then
   `lower.rs`'s duplicate `root_rule`/`index_rule` can move into `check_mounts` as their
   own comment already plans.
3. The per-route `Route::static_policy`/`Route::php`/`with_static`/`with_php` pair (§4.3)
   remains, blocked on `MB.21.5.1`; these suites no longer reference it, so its deletion is
   a compile-error-free change in this crate's tests.
