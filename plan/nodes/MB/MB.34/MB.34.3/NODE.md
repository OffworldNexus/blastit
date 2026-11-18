---
id: MB.34.3
title: "blastit-sqlpolicy: one deterministic template validator ($?, IN ($?, ...), VALUES rows) with exact character offsets and a protected suite"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.34.2]
paths:
  - "crates/blastit-sqlpolicy/src/template.rs"
  - "crates/blastit-sqlpolicy/src/lib.rs"
  - "crates/blastit-sqlpolicy/src/literals.rs"
  - "crates/blastit-sqlpolicy/tests/critical_query_template.rs"
  - "crates/blastit-sqlpolicy/Cargo.toml"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T05:02"
---

# Goal

Ship the deterministic template validator in `crates/blastit-sqlpolicy` — the engine
`MB.34.4` will call — with its own critical tests, and nothing else. One public function, one
error type, exact offsets:

```rust
pub fn check_template(sql: &str) -> Result<(), TemplateError>;
```

- Input is the **raw decoded** statement (what the developer wrote, inside the quotes — not
  `normalize`'s output).
- On success the statement is one parseable PostgreSQL 18 statement once rewritten per the
  `MB.34.2` table: `$?` → `$1, $2, …`; `IN ($?, ...)` → `IN ($1)`; `($?, $?), ...` → one
  `VALUES` row; `$1…$n` pass through.
- `TemplateError` carries the **character offset into the input** (0-based at this boundary),
  and a `kind` that names exactly one defect with the fixed sentence the CLI prints:
  `Empty`, `MultipleStatements`, `StrayEllipsis`, `Syntax` (pg_query's own message, clipped and
  escaped to one line), and the existing literal/redaction cases stay where they are in
  `validate.rs` (this function does not duplicate them).
- Never `panic`, never `unwrap`, never print the statement (a query can hold a value). Bounded:
  the rewrite's output length is checked against the input length + the placeholder cap; any
  arithmetic overflow is a refusal.

`normalize` stays byte-for-byte unchanged — the wire matcher depends on it. This function is
additive and must be reachable without constructing a manifest.

# Context

- **Why this crate.** It already depends on `pg_query = "6.2.1"` (`Cargo.toml`), has
  `lexer::lex` with `Region`/`RegionKind` for finding quoted regions and comments, and knows the
  two divergences from PostgreSQL's scanner. `crates/blastit-manifest` must not gain `pg_query`
  (it is a parse, not a model concern, and the LSP links the manifest).
- **The lexer is `pub(crate)` today.** `Region`, `RegionKind`, `lex`, `is_space` and
  `placeholder_end` are crate-private; the new module can use them as-is. Do not widen the
  public API beyond `check_template` and `TemplateError` unless the design says so.
- **`...` is Blastit syntax, not SQL.** It only ever appears where `normalize` puts it:
  after an `IN` list and after a `VALUES` row (see `normalize.rs:100-105`, `lib.rs:1-20`). Any
  other occurrence is `StrayEllipsis` at its first `.`.
- **`$` outside a placeholder.** `$` inside a quoted literal is not a placeholder; the lexer
  already distinguishes `RegionKind::Code` from quoted regions. A bare `$` in code is
  PostgreSQL's business — let `pg_query` refuse it at its own position.
- **Empty / multi.** `pg_query::parse("")` and `parse("SELECT 1; SELECT 2")` decide the two
  structural refusals; do not pre-trim into a different answer. A trailing `;` on one statement
  is legal SQL and stays legal.
- **Do not touch** `crates/blastit-manifest/**`, `crates/blastit/src/lsp/**`, `editors/**`, or
  `normalize.rs`'s behaviour. Tests go in a **new protected** file
  `crates/blastit-sqlpolicy/tests/critical_query_template.rs`; `tools/critical_guard.py` will
  guard it from the commit that adds it.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ cargo test -p blastit-sqlpolicy
$ cargo test -p blastit-sqlpolicy --test critical_query_template
$ cargo clippy -p blastit-sqlpolicy --all-targets -- -D warnings
$ cargo mutants -p blastit-sqlpolicy --file src/template.rs --no-shuffle   # if available
```

`critical_query_template.rs` must contain, at minimum:

1. **Every rewrite row**: `WHERE price > $?`; `IN ($?)`; `IN ($?, ...)`; `IN ($1, $2, $3)`;
   `VALUES ($?, $?)`; `VALUES ($?, $?), ...`; `VALUES ($1, $2), ($3, $4)`; a statement mixing
   `$?` before and after an `IN` list (numbering is statement order).
2. **The refusal set, each with its offset**: `SELCT id FROM t` (offset of `SELCT`);
   `SELECT id FROM` (pg_query position or the pinned fallback); `IN ($?, $?)` where `...` was
   dropped (a two-element `IN` is legal — this must **pass**); `SELECT 1; SELECT 2` (the second
   statement's first character); `""` and `"   "`; `SELECT ...` (stray ellipsis at the first
   dot); an over-long statement (the cap).
3. **A property loop**: for every fixture that `check_template` accepts, exactly one statement
   parses. Reach the rewritten text through one crate-private helper that both the production
   path and the test use (never a second copy of the expansion in the test), so the test proves
   the shipped rewrite and not a lookalike. The public API stays one function.
4. **UTF-8 offsets**: a statement with a non-ASCII identifier (`SELECT name FROM "café"`) has
   offsets counted in characters, and the byte-vs-char difference is asserted.
5. **No printing**: the error's `Display` never contains the statement text (assert on a
   statement holding `'a@b.com'`).

Paste `cargo test` tails and the `mutants` result (or say the tool is unavailable and why) into
`RESULT.md`.

# Notes

- **Keep the module one screen of logic plus a table.** The rewrite is a scan over
  `lexer::lex` regions; no regex, no second tokenizer.
- **PostgreSQL's message is data.** `TemplateError::Syntax` stores a clipped `String`
  (`clipped()`/`escape_control()` from `error.rs` are in the manifest crate — this crate cannot
  use them; write the two-line sanitizer here or in a small shared helper the design names).
- **No new dependency.** `pg_query` + `serde_json` are already declared; if the design needs
  `thiserror`, it is already a workspace pattern — add it only if `blastit-sqlpolicy` already has
  it, otherwise hand-write `Display`.
- **Sequencing.** `MB.34.4` depends on this leaf's exact API; if the signature must change,
  change it here rather than adding a wrapper there.
