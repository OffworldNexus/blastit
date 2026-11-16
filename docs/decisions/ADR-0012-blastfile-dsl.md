# ADR-0012: The Blastfile DSL — grammar, no header, hand-written lexer/parser, lossless CST

- **Status:** Accepted
- **Date:** 2026-11-16
- **Node:** MB.21
- **Owner:** owner (language decisions), designer (implementation decisions)
- **Supersedes:** nothing. Retires `blastit.toml` as an input (its reader is
  deleted in `MB.21.4`); `ADR-0002`'s discovery *shape* survives with a new file
  name, `ADR-0003`'s atomic-write and literal-redaction rules survive, and
  `ADR-0011`'s `raw` boot refusal survives with new nouns.
- **Analysis:** `docs/design/blastfile-dsl.md` (the full design),
  `docs/blastfile.md` + `docs/reference/blastfile.md` (the accepted DX contract,
  MB.21.1).

## Context

`blastit.toml` is Blastit's whole routing table and permission model, and TOML is
a poor fit for the one artefact every developer edits by hand: a `version`
header nobody can guess, `=` between every keyword and value, four-level tables
for a grant (`[route.<proxy>.tables]`), no way to say "this is an
environment-variable *name*", and no way to hold SQL or a regex without fighting
TOML's quoting. `--learn` then has to splice TOML through a text editor that
re-parses the result.

The owner replaced the file with a purpose-built DSL and settled the language
(2026-11-16, MB.21): universal `keyword [args…] { … }` blocks, newline/`;`
terminators, `#` comments, comma-separated lists, **zero `=`**, **no `version`
header**, `"…"` escaped / `'…'` raw strings, a dedicated `$IDENT` token,
protocol-as-keyword (`postgres`), explicit `use <trait>`, no vendor knobs, and
`postgres` as the only store keyword in 0.2.0.

This ADR records the *implementation* decisions that make that language buildable
by a cheap model, offline, with no new dependency — and the two owner decisions
that must not be re-opened: the `body raw` acknowledgement and the removal of the
per-view `workers` cap.

## Decision

1. **The grammar is LL(1), parsed by hand, in one pass.** `file → statement*`,
   six top-level sections (`app`, `entrypoint`, `postgres`, `trait`, `view`,
   `static`) with disjoint FIRST sets, and one-token lookahead for the three
   decisions that matter: store form (`$ENV` / `{` / `name`), view form (`{` /
   Method / pattern), and "endpoint vs directive" inside a view block. A bare word
   whose first byte is `A`–`Z` is a `Method`; every other word is a
   keyword-or-name. That lexical rule is the whole of the case-sensitive
   disambiguation and it makes `get "/x"` an error instead of a mystery.
2. **Zero new dependencies.** No parser generator and no combinator crate. The
   workspace stays `--offline`-buildable at MSRV 1.88, the audit surface stays at
   zero, and the error position and the budgets stay visible in code we own
   (design §19.1).
3. **Every loop is bounded by the input and every nesting by the grammar.** The
   lexer walks `char_indices()` (never a hand-written `i += 1`, so no mutant can
   hang a scan — `LESSONS.md`, `MB.1.4`); block depth is **2** by the grammar
   (`view { postgres { … } }` is the deepest) and by a counter; a file over 1 MiB,
   a token stream over 262 144 tokens, a list over 256 items and a **third** nested
   block are each refused with one sentence (design §4.5).
4. **The CST is lossless and is the only structure `fmt` and `--learn` touch.**
   Every byte is in exactly one token or trivia node, so `print(tree)` returns the
   source byte for byte; `fmt` is a layout pass over the tree; `--learn` splices
   through the tree. Two invariants are the contract: `lower(fmt(text))` equals
   `lower(text)` (formatting never changes meaning) and `fmt(fmt(text))` equals
   `fmt(text)` (a canonical file is already formatted) — design §14.3.
5. **The parser lowers onto today's `Manifest`, and the Rust names do not
   change.** `view` becomes `Route`, `trait` becomes `Group`, `postgres` becomes
   `Proxy`. The model gains exactly four things (an `app` name, top-level `mounts`,
   `manifest.php()`, `Route.sandbox`) and loses the per-route static/PHP fields,
   which the new file cannot express. Renaming `Route` to `View` across every
   crate would buy nothing a developer can see (design §4.3, §19.4).
6. **`body raw` is the loud opt-out and `body raw unvalidated` is the
   acknowledgement** (owner decision, recorded here, not re-openable). `body raw`
   lowers to `(BodyCodec::Raw, allow_raw = false)`: `blastit dev` warns, `blastit
   check` reports `raw_ingress`, and the production boot refuses the file through
   `Manifest::raw_ingress_refusals` — the one function both stages call
   (`ADR-0011`). `body raw unvalidated` lowers to `(Raw, true)`: a deploy boots
   with the same boot warning. The acknowledgement is a **sub-token of the `body`
   directive**, never a second directive and never `=`, and `--learn` cannot
   write either form because the editor's proposed-view type has no codec field
   (design §10).
7. **`workers` is removed, not deferred** (owner decision). The per-view
   concurrency cap duplicated what the scheduler already enforces, was
   downward-only, and gave `check` nothing to verify. A v1 `[route.php] workers`
   route loses it and adds nothing; `workers` is not a keyword, and writing it is
   a parse refusal whose sentence names the removal (no ADR id in a user-facing
   message). No replacement knob exists (design §12.3).
8. **`static` is a top-level mount, and the request precedence moves the
   filesystem before the view.** A mount is not a route: the scheduler resolves
   the longest matching prefix first, serves a regular file if one is there, and
   on a miss falls through to the view table, then to the mount's `fallback`
   (GET/HEAD), then `404`. `spa = true` + `index` becomes `fallback "<file>"`, and
   `.php`/`.phtml`/`.phar`, dotfiles, `..`, NUL, absolute/backslash paths and
   symlinks stay refused by the unchanged `openat2` resolver (design §9).
9. **`HEAD` is implied by `GET` at parse time.** Normalising the method list in
   `lower` (before the `Route` exists) is what keeps the scheduler's matching
   order unchanged; `routing.rs`'s "HEAD is never implied" comment is retired
   with the file that motivated it (design §8.4).
10. **`$IDENT` is a name, never a value.** The lexer emits `Env(name)`, `lower`
    copies the name into `Proxy.input`, and nothing in `blastit-manifest` reads
    `std::env`. An unresolved name is a boot error from `blastit-env` naming the
    variable; a quoted literal where a name is required is a syntax error; and a
    `Blastfile` therefore holds no credential (design §7.4, §17.2).
11. **Errors are one line, `Blastfile:<line>:<col>: <what>; <how to write it>`.**
    A whole-file refusal names the view/trait/store and the fix; a file-size
    refusal has no position and says so. Values are escaped and cut at 120
    characters so no file can break the event stream (design §18).
12. **Documentation fences keep the `text title="Blastfile"` opening, not
    ```` ```blastfile ````.** Pygments has no `blastfile` lexer, so the title-keyed
    opening is what the site can render and what the harness can count; a docs
    test refuses ```` ```blastfile ```` in `docs/**` so the two spellings cannot
    drift (design §16.1).
13. **The code lives in `crates/blastit-manifest/src/blastfile/`, not a new
    crate.** `docs_fences.rs`, `edit.rs` and the protected manifest suites keep
    their home, no `Cargo.toml`/`Cargo.lock` serialization chain is needed, and
    the crate is already the one that owns "one document becomes a typed
    manifest" (design §4.1).
14. **One manifest format, and the retired one is erased** (owner decision,
    2026-11-16: "delete every reference, delete all the code … never to be spoken
    of again"). The retired format's reader, its patcher's TOML splicing, the
    `toml`/`toml_edit` dependencies, its documentation page and **every
    occurrence of its file name** in `crates/`, `python/`, `scripts/`, `e2e/`,
    `Makefile`, `docs/`, `README.md` and `ARCHITECTURE.md` are deleted. There is
    no format detection, no sniff, no fallback format, no converter and no
    migration sentence: a missing `./Blastfile` is a plain missing-file error, and
    any other content is the ordinary `Blastfile:<line>:<col>` syntax error
    (design §15). The law and its precedent are `ADR-0010` / `MB.3.17` / `MB.3.30`
    (the retired FastCGI design): delete the implementation, delete the name, and
    leave a mechanical guard. The guard is a pytest in the existing
    `python/tests/test_docs_assets.py` (run by `make docs-unit`; no new file and no
    new Makefile target), fails the build if the name, the dependency or the
    retired reference page reappears (R27–R29), and carries a negative
    self-test so the rule cannot silently stop running.
15. **The top-level "attach this to every view" key is dropped, not mapped.** A
    grant set attaches only where a view writes `use <trait>`; an invisible global
    attach cannot fail closed, and no keyword replaces it (design §7.1). The model
    keeps `global_groups`, always empty from a Blastfile, so `effective_grants`
    and `check` are untouched.

## Consequences

- **Positive:** a developer writes a Blastfile with no `=` and no version header,
  and every mistake comes back as one line at the exact position with the fix.
- **Positive:** `fmt` is comment-preserving and idempotent, so a CI step
  (`blastit fmt --check`) can be a required gate and `--learn` can be surgical
  without a second serialiser to drift.
- **Positive:** the parser is unprivileged, dependency-free, MSRV-pinned and
  mutation-testable module by module; a hostile or generated file is refused by
  four independent caps before it can cost boot time.
- **Positive:** the cutover is a compile-error-driven walk: the model's four
  additions and the two removals make every stale call site fail to build rather
  than silently misread a file.
- **Trade-off:** the model keeps `version`, `global_groups` and
  `PhpPolicy.workers` as inert fields (`1`, `[]`, `None`) so the scheduler, the
  brokers and `check` need no change; a reader of the model has to know the file
  has no spelling for them. Recorded in design §4.3 and §7.1.
- **Trade-off:** dropping the retired format's `output` map renames environment
  variables for a deployment that used it. There is no compatibility path: the
  file's own text says "every input variable keeps its name", and design §7.5
  records the drop with its reason.
- **Trade-off:** one `view` with several endpoints lowers to several `Route`s
  sharing one `sandbox` id. `UidLayout` numbers by `sandbox`, so the sandbox
  promise holds, but a reader who maps "route index" to "uid" must read the new
  accessor (design §10.3).
- **Risk accepted:** a 1 MiB cap is a new failure mode for a machine-generated
  file. It is named in the refusal, and the reference's own complete example is
  ~5 KB, so the cap is ~200x the largest hand-written file this repository ships.

## Refused, with reasons

| Proposal | Why not |
|---|---|
| `logos` / `pest` / `chumsky` / `nom` | a dependency in the offline gate and the MSRV surface for no work removed; a generated parser hides the position and budget checks this design must control exactly |
| a `version` header, or `key = value` prologue | the owner's spec forbids both; a version header is the first thing a developer has to look up |
| keeping the retired reader behind a feature flag, or shipping a one-shot converter | keeps a second grammar, a second parser and a second set of refusals alive; "temporarily" is how a compatibility surface becomes permanent (owner decision, decision 14) |
| a second `allow`/`allow_raw` directive for the acknowledgement | a second thing to forget; the accepted page wrote `body raw unvalidated` |
| keeping `workers` as a view directive | duplicates the scheduler's budget, downward-only, unverifiable by `check` |
| lowering `static` mounts into forwarding routes | cannot express "mount misses, then a view answers" without turning routing into a per-route filesystem loop |
| renaming `Route`→`View`, `Group`→`Trait`, `Proxy`→`Store` | churn across every crate for no developer-visible change, and it breaks the `--json` keys the accepted page keeps |
| resolving `$IDENT` at parse time | makes `fmt`/`check` machine-dependent and puts a secret read inside the parser |
| a ```` ```blastfile ```` fence | no Pygments lexer; the title-keyed `text` fence renders and counts |

## What would make this decision wrong

1. **If a second, non-Blastfile format had to be read** (a generated JSON
   manifest for CI). Then one reader with one grammar is no longer enough, and
   the "one parse entry point" property (R21) would need a generation boundary.
   Rejected for 0.2.0: `--learn` writes the file.
2. **If the file were attacker-controlled.** It is policy: a writer can grant
   `unrestricted`. The caps and the total-refusal rules defend the *boot*, not a
   hostile author, and the design says so (design §17.1).
3. **If `fmt` needed to wrap long lines or align columns.** The canonical layout
   is deliberately width-free so the printer is a single walk; a width-sensitive
   formatter would need a measurement pass and a stability proof the tests do not
   have.
4. **If the mount-before-view order broke a documented page.** It does not: the
   accepted precedence (`docs/reference/blastfile.md` "Request precedence") is
   exactly this order, and the two `405`/`fallback` interactions are pinned as
   assertions (design §9.2).
5. **If a critic's dependency demand were accepted** (a combinator crate "to save
   code"). The hand-written parser is ~1,100 lines across ten functions; the
   saving is smaller than the offline-gate and audit cost, and the design's tests
   are the same either way.
6. **If the erasure guard were the only thing keeping the retired format dead.**
   It is not: the reader, the dependency and the page are deleted, so the guard
   only refuses a *re-introduction*. A guard that is the sole mechanism would be
   a comment with a process exit code (design §15.2).

## References

- `docs/design/blastfile-dsl.md` §4–§18 (the design this records), §19
  (alternatives), §20 (the test plan), §21 (the leaves).
- `docs/blastfile.md`, `docs/reference/blastfile.md` (the accepted DX, MB.21.1).
- `ADR-0001` (dev simulation and the secret barrier), `ADR-0002` (discovery),
  `ADR-0003` (atomic writes, literal redaction), `ADR-0004` (`check --json`),
  `ADR-0006`/`ADR-0011` (`body raw`).
- `crates/blastit-manifest/src/{model,validate,routing,edit}.rs`,
  `crates/blastit/src/{config,check,stage0,cli,audit_v1}.rs`,
  `crates/blastit-scheduler/src/{route,rewrite,static_asset}.rs`.
