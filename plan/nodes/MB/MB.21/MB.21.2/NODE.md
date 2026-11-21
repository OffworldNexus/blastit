---
id: MB.21.2
title: "Design the Blastfile lexer, parser, CST and cutover (ADR-0012) with security/implementability/testability/consistency critics"
kind: task
status: done
owner: designer
priority: 1
depends_on: [MB.21.1]
paths:
  - "docs/design/blastfile-dsl.md"
  - "docs/design/blastfile-dsl.review-security.md"
  - "docs/design/blastfile-dsl.review-implementability.md"
  - "docs/design/blastfile-dsl.review-testability.md"
  - "docs/design/blastfile-dsl.review-consistency.md"
  - "docs/decisions/ADR-0012-blastfile-dsl.md"
  - docs/decisions/README.md
attempts: 3
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:30"
---

# Goal

Turn the accepted DX into the technical design that the implementers can build without guessing:

* `docs/design/blastfile-dsl.md` — the full design of the Blastfile language implementation.
* `docs/decisions/ADR-0012-blastfile-dsl.md` — the decision record (grammar, no `version` header,
  protocol-as-keyword, explicit `use`, zero-dependency hand-written lexer/parser, CST formatter).

Both pass fresh-context `security`, `implementability`, `testability` and `consistency` critics before
any lexer exists. The design has to prove **nothing real is dropped** and answer every open question
the guide deliberately left open.

# Context

- Inputs: `MB.21.1`'s two accepted pages (the DX contract), the settled spec in `MB.21`'s body, the v1
  capability map, and the code that must keep behaving: `crates/blastit-manifest/src/{model,parse,
  validate,routing,edit,error}.rs`, `crates/blastit-manifest/tests/*`, `crates/blastit/src/{config,
  stage0,cli,check,dev_learn,audit_v1}.rs`, `crates/blastit-scheduler/src/scheduler.rs` (body plan),
  `docs/reference/manifest.md`.
- **ADR number:** `ADR-0011` is taken (`raw-ingress boot refusal`). This ADR is **`ADR-0012`**, and
  `docs/decisions/README.md` gets its row.
- The design must pin, each in one short section with the code site it lands in:
  1. **Tokens and grammar** as EBNF: `keyword [args…] [{ directives… }]`, newline/`;` terminators,
     `#` comments, comma-separated items, escaped `"…"`, raw `'…'`, `$IDENT`; `line:col` on every
     error; zero `=`; no header. Bounded iteration (one pass, byte-indexed), nesting depth fixed by
     the grammar (two block levels), a file-size cap, and the exact refusal for an unbounded input.
  2. **LL(1) parse**: first/follow sets, one-token lookahead choices (view single-signature vs
     multi-endpoint vs store block), and the AST that maps 1:1 onto today's `Manifest`
     (`app`/`preload`, `Proxy` with raw `input`/`output`, `Group` ← `trait`, `Route` with
     `ingress`/`static_policy`/`php`, `PgGrant` + `TableGrants`).
  3. **Path-pattern compiler**: `{name}` → `[^/]+`, `{name:int}` → `[0-9]+`, `{name:uuid}` → the UUID
     form, trailing `*` → prefix tail, `.` and every other literal escaped; `regex '…'` kept
     verbatim and fully anchored. Shown as a table of pattern → regex, plus the refusal for a
     duplicate/disallowed parameter name. **`HEAD` implied by `GET`** must land at parse time
     (normalized into `methods`) so the scheduler's matching order does not change.
  4. **Static precedence**: how the scheduler gets (1) file-on-disk under the prefix → (2) first
     matching view → (3) the declared fallback file → (4) `404`; how `fallback "<file>"` expresses
     what `spa = true` + `index` express today; and that `.php`/`.phar` stay `404` and dotfiles,
     `..`, NUL and symlink escapes stay refused.
  5. **`body` and the acknowledgement**: how `body raw` relates to today's `allow_raw = true`
     (production boot refusal, `dev` warning, `check` finding) and why `--learn` never writes `raw`.
  6. **Traits**: expansion into today's `Group` semantics, `use` resolution order, and the parse-time
     refusals (`unrestricted` and `egress any` inside a `trait`, unknown name, duplicate name, a trait
     that is never used → boot warning parity).
  7. **`entrypoint php` and `workers`**: the one top-level front controller for the whole app, how it
     reproduces today's per-view `[route.php] entry`, and **the decision for the optional `workers`
     cap** — either a `workers <n>` view directive or a documented removal with an ADR reason and a
     migration row. Silence here is a BLOCKER.
  8. **Grant mapping**: `transactions` → `transaction_control`, `functions …`, `select|insert|update
     t(c1, c2)` and `delete t` → `TableGrants`, `query '…'` → `queries` with the existing `$?`
     normalization, `unrestricted` inline-only, and the boot-warning parity list.
  9. **CST and tooling**: a lossless concrete syntax tree (trivia nodes carry every byte), canonical
     printing for `blastit fmt [--check] [PATH]` (idempotent, comment-preserving), and the surgical
     edit API that replaces `edit.rs` for `--learn` while keeping its postcondition
     (`expected_routes`-style verify: an edit either reproduces the whole document or writes nothing).
     State the round-trip invariant (`parse(print(ast)) == ast`, `print(parse(text))` byte-identical for
     an already-canonical file) as the two properties the tests assert.
  10. **Discovery and the migration error**: `./Blastfile`, `BLASTIT_MANIFEST`, no upward search; how
      a `.toml` path or TOML-looking content is classified and named (`Blastfile`, `./Blastfile`, the
      reference page) in one sentence, at every entry point.
  11. **`docs_fences`**: the ```` ```blastfile ```` opening, which fences are complete documents, and
      how the count-parity rule replaces today's TOML fence counting.
- **Security section (skill `security-invariants`)**: `$IDENT` is a *name*, never resolved by the
  parser (no environment read at parse time); an unresolved name is a boot error naming the variable,
  not an empty value; a literal where a name is required is a syntax error; patterns cannot smuggle a
  path traversal into the file server; regex stays Rust `regex` (linear time, no backtracking); a
  parse error never falls back to a permissive default.
- Critics write `docs/design/blastfile-dsl.review-{security,implementability,testability,consistency}.md`
  and `docs/design/blastfile-docs.review-*` already exist from `MB.21.1` — do not touch those.

# Acceptance

```console
$ ls docs/design/blastfile-dsl.md docs/decisions/ADR-0012-blastfile-dsl.md
$ grep -n 'ADR-0012' docs/decisions/README.md
$ grep -rn 'VERDICT' docs/design/blastfile-dsl.review-*.md
$ uv run --no-project tools/plan.py validate
$ make docs-check
$ wc -l docs/design/blastfile-dsl.md docs/decisions/ADR-0012-blastfile-dsl.md
```

1. The design covers items 1–11 above, each with the crate/module it lands in and the test tier that
   proves it; a reviewer can find the answer to "what happens to `[route.php] workers`?" without
   asking a human.
2. The capability map from `MB.21` has a Blastfile spelling and a code site for **every** row; a row
   that is deliberately dropped is named as dropped with its reason and its migration note.
3. The two round-trip invariants, the `line:col` error contract, the bounded-input rule and the
   `$IDENT`-is-a-name rule are all in the testability and security sections, phrased as assertions a
   critical test can make.
4. All four critic files end in `VERDICT: PASS`; every BLOCKER/MAJOR is fixed or answered in
   `RESULT.md` with the section it changed.
5. `docs/decisions/README.md` lists `ADR-0012` with status `Accepted` and node `MB.21`; `make
   docs-check` is green (both trees are excluded from the public site, so nothing leaks).

# Notes

- Single-writer artefact (`docs/knowledge/LESSONS.md`, `MB.3.14`): write the long document
  incrementally as `blastfile-dsl.md.part` and rename at the end, and commit early — an untracked
  design has no recovery.
- Keep the design implementable by a cheap model: numbered sections, one decision per section, a
  table wherever a mapping exists, and explicit "refuse this, with this message" rows. The lexer and
  parser leaves must be writable from this document alone.
- If a critic demands a dependency (a parser combinator crate), refuse it and say why in the ADR:
  hand-written keeps the gate offline, the MSRV at 1.88 and the audit surface at zero.
