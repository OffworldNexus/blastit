---
id: MB.21.3.2
title: "LL(1) Blastfile parser: the lossless CST and the recursive-descent parser that builds it"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.3.1]
paths: ["crates/blastit-manifest/src/blastfile/**", "crates/blastit-manifest/src/lib.rs"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T02:16"
---

# Goal

Add the LL(1) parser and the lossless CST it builds: Blastfile text → the one tree of design §14.1,
with `line:col` on every refusal and every loop bounded by the input. **Lowering to `Manifest` is
`MB.21.3.8`** — this leaf is design `MB.21.3.2a` (§21) and stops at the tree.

# Context

- Design: `docs/design/blastfile-dsl.md` §4.1/§4.2 (the module tree, one tree with two readers), §5
  (tokens and trivia), §6 (FIRST/FOLLOW, the one-token choices between the view forms and the store
  block) and §4.5's budgets. `Manifest::from_blastfile_str`/`parse_blastfile` and every lowering rule
  are `MB.21.3.8`'s: build no `Manifest` here.
- Sections to parse: `app`, `entrypoint python` (with optional `preload` block), `entrypoint php`,
  the three store forms (`postgres $URL`, `postgres <name> $URL`, `postgres [name] { host $DB_HOST; … }`),
  `trait <name> { … }`, `view` in both headers, and `static "<prefix>" { … }`.
- Refusals at `line:col`: unknown keyword at top level, unknown directive inside a block, a directive
  in the wrong section, a duplicate `app`/`entrypoint`/`view`/`trait`/store name, a string literal
  where `$IDENT` is required, `unrestricted` or `egress any` inside a `trait`, a `body` on a static-only
  mount, a `use` of an unknown trait, more than one `entrypoint`, `query` outside a grant block.
- **The CST is the deliverable.** The arena of §14.1 (`Element`, `NodeId`, trivia kept) is what
  `MB.21.3.4` prints and `MB.21.3.8` lowers; there is no second AST and no second serialiser. A parse is
  `Result<Cst, ManifestErrors>` over the stream `MB.21.3.1` emits.
- Budgets are enforced where §4.5 puts them: `MAX_BLOCK_DEPTH` (`2`) in the depth counter and
  `MAX_LIST_ITEMS` (`256`) in every list loop; a hostile input fails with `line:col` instead of growing
  a stack.
- Keep `Manifest::from_toml_str` working and untouched; `MB.21.4.5` deletes it. Do not rename accessors,
  and do not add a parsing helper a later leaf must undo.
- Tests live **inside** the module (`#[cfg(test)] mod tests`, §4.1), so the parser can assert private
  items: the refusal matrix with exact `line:col`, the CST shape of one document per §6 form, and the
  two budgets. The protected integration suite `tests/critical_blastfile_parser.rs` lands with
  `MB.21.3.8`, because an integration test may only use the public entry points and none exists until
  lowering does.

# Acceptance

```console
$ cargo test -p blastit-manifest --locked blastfile::parser
$ cargo test -p blastit-manifest --locked
$ make rs-fmt
$ make rs-lint
$ make check
```

1. Every §6 form parses to the CST both readers walk: one in-module test per production (`keyword
   [args] { … }`, both `view` headers, the store block, the grant block, `static`, `trait`,
   `entrypoint`), each asserting the node kinds it produced.
2. Every refusal in the list above is a test asserting message **and** `line:col`, and no refusal
   silently defaults to a permissive value.
3. Parsing a 256 KiB `Blastfile` of many views completes without unbounded recursion (nesting is two
   block levels by grammar; assert a hostile `{{{{…` input fails cleanly instead of overflowing).
4. `make check` exit 0; the TOML path is byte-for-byte unchanged (existing suites green).
5. Files: `src/blastfile/{tokens,syntax,parser}.rs` with the `mod.rs` registration already in place, and
   in-module tests only (≤ 5 files, ≤ 600 changed lines). No `lower.rs`, no `model.rs`.

# Notes

- Do not reach for `validate.rs` here: validation is `MB.21.3.8`'s, and a parser that validates would let
  the two leaves disagree about which document is legal.
- `query 'SELECT … WHERE x = $?'` keeps the existing normalized spelling; the parser stores the raw
  string and the analyser does the `$?` work it already does.
