# `query` templates, the Quick Fix and `sql_string`: security review (MB.34.2)

- **Lens:** security
- **Node:** MB.34.2
- **Target:** `docs/design/blastfile-dsl.md` (§3 R31–R36, §4.4, §4.5, §13.4, §18.1, §20),
  `docs/design/blastfile-lsp-zed.md` (§0.2, §2.2–§2.3, §4.6, §5.3, §7.5–§7.6, §7A, §9.2a,
  §9.3, §9.5, §10.1–§10.2, §11, §13), `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`
  (2026-11-18 amendment), `docs/decisions/ADR-0003-wire-learn-and-literal-redaction.md`
  (2026-11-18 amendment).
- **DX pages checked against the design:** `docs/reference/blastfile.md` (`query` section,
  `Limits`), `docs/local.md` `## Editors`, `docs/ci.md`, `docs/troubleshooting.md`.
- **Invariants:** `.opencode/skills/security-invariants/SKILL.md` (§5 "Never leak, never
  panic"; §3 no parser differentials), `docs/VISION.md`, `ARCHITECTURE.md` §5–6.
- **Code read to test the claims:** `crates/blastit-sqlpolicy/src/{lexer,normalize,literals,
  learn}.rs`, `crates/blastit-manifest/src/{model,validate}.rs`,
  `crates/blastit-manifest/src/blastfile/lower.rs`, `editors/tree-sitter-blastfile/grammar.js`,
  and `pg_query 6.2.1`'s bundled `libpg_query/.../pg_config.h`.
- **Copies read:** the amendment is the **uncommitted** working tree of
  `.worktrees/release-0.2.0` (the `release/0.2.1` checkout still carries the pre-amendment
  documents). This review refers to that working tree.

**Summary:** `check_template` itself stays a parse, and the `$?`/`$n` numbering reuse is
sound for matching. The Quick Fix is not: it is a **policy change** that can turn a refused
`pg_read_file('…')`/`dblink('…')`/`lo_import('…')` literal into a grant for any path or host,
bypassing the `DANGEROUS_LEARN_FUNCTIONS`/`COPY … PROGRAM` refusal that `--learn` and `fold`
enforce. The `Syntax` sentence also reintroduces the very leak `learn.rs` strips. There are
quadratic/unbounded paths in the span map and `parameterize`, and the validator parses with
PostgreSQL 17.7 while the design says 18.

| ID | Grade | File:section | Finding | Why it matters |
|---|---|---|---|---|
| S1 | **BLOCKER** | `blastfile-dsl.md` §13.4.6; `blastfile-lsp-zed.md` §7A.2 | The Quick Fix parameterises **every** quoted literal (`pg_read_file('/etc/passwd')` → `pg_read_file($?)`) and the result is accepted as a grant; the design calls it "the canonical grant text … the bytes `--learn` writes". | A one-click broadening of a capability from one constant to any value; it bypasses the dangerous-function/`COPY … PROGRAM` refusal in `learn.rs` that `--learn`/`fold` enforce. |
| S2 | **BLOCKER** | `blastfile-dsl.md` §13.4.3–§13.4.4; R33 | `Syntax` prints "`the statement does not parse: <pg_query's message, one line, ≤ 120 chars, control bytes escaped>`"; the design says "The sentence still names the token". | `pg_query`'s message quotes the offending token (`at or near "s3cr3t"`); `learn.rs` deliberately strips it for exactly this reason. `check_template` is public and performs no policy, so a literal-bearing statement can reach it and land in CI stderr/LSP diagnostics. |
| S3 | **MAJOR** | `blastfile-lsp-zed.md` §7A.2–§7A.3 | The action is offered iff `parameterize` is `Some`; it never checks that the result parses. | `SET TIME ZONE 'Europe/Paris'`, `COPY … PROGRAM 'sh'`, `DO $$…$$`, a two-statement query and a syntax error all get a Quick Fix, and applying it publishes a different refusal, violating "the reply must re-parse clean". |
| S4 | **MAJOR** | `blastfile-dsl.md` §13.4.2 | "the `, ...` is **deleted**" does not say the deletion is token-exact or that the `;`/empty refusals run **before** the rewrite. | A range deletion can remove a code `;` (e.g. `IN ($?, ; ...)`), making `stmts.len() == 1` true for text that is really two statements. |
| S5 | **MAJOR** | `blastfile-dsl.md` §13.4.5 | `escapes: Vec<u32>` is scanned with "if `site.escapes contains i`" for every decoded character. | `Vec::contains` is O(k): a 16 MiB `"…"` query with millions of `\"` escapes is O(n·k), a local DoS in `check`/boot/LSP; `MAX_TEMPLATE_CHARS` only bounds `check_template`, not a `LiteralQuery` site. |
| S6 | **MAJOR** | `blastfile-dsl.md` §13.4.6; `blastfile-lsp-zed.md` §7A.3, §13.3 | `parameterize(sql: &str)` has no `MAX_TEMPLATE_CHARS` cap, and the LSP calls it on `q.raw` from an unvalidated document (up to `MAX_SOURCE_BYTES`). | A 16 MiB query allocates a 64 MiB `Vec<char>` and can produce a `TextEdit` response near/over `MAX_FRAME_BODY_BYTES`; the design's "MAX_TEMPLATE_CHARS for the statement" is only `check_template`. |
| S7 | **MAJOR** | `blastfile-dsl.md` §3 R31, §13.4.1 | "one parseable PostgreSQL 18 statement", parsed with `pg_query 6.2.1`. | `pg_query 6.2.1` bundles **PostgreSQL 17.7** (`PG_VERSION_STR "PostgreSQL 17.7 (libpg_query)"`, `PG_VERSION_NUM 170007`); the validator and the `postgres:18` server are different grammars, so a legal 18 statement is a false red and the parser-differential claim is weakened. |
| S8 | **MAJOR** | `blastfile-lsp-zed.md` §13.3, §11 S2; `blastfile-dsl.md` §13.4.1 | "one parse per hover/`codeAction` on top of the per-publish parse"; a 16 MiB document is re-parsed for every hover, and `pg_query` (C) has no time/fuel budget. | O(file) per interaction on a single-threaded server; a cloned repo's `Blastfile` can peg the editor; `catch_unwind` does not catch a `pg_query` SIGSEGV, so "parsed, never executed" only proves no `execve`. |
| S9 | **MAJOR** | `blastfile-lsp-zed.md` §7A.5 C3 | C3's fixture is `query 'SELECT id FROM t WHERE email = ''a@b.com'''` labelled "the `'…'` spelling". | `'…'` has no escapes and ends at the first `'`; the fixture is not a legal Blastfile. The only positive literal Quick Fix test — the path that can broaden a dangerous call — cannot run. |
| S10 | **MAJOR** | `blastfile-dsl.md` §13.4.1, §3 R31 | "`COPY x FROM PROGRAM $?` … is **legal here**". | PostgreSQL's `COPY … FROM PROGRAM` takes a string constant, not a parameter; `$?` is a syntax error, and the literal form is refused by `LiteralQuery`. The "parse, not policy" example is false, and a cheap implementer's R31 test cannot pass as written. |
| S11 | MINOR | `blastfile-dsl.md` §13.4.5; `model.rs` | `pub raw: String` is the pre-redaction statement on a `PgGrant` whose `Debug` is derived. | A `LiteralQuery` document (lowered without `validate`) carries the literal in `raw`; any future `{:?}` logs it, violating "never log SQL parameter values". No current log path, hence MINOR. |
| S12 | MINOR | `blastfile-dsl.md` §13.4.4; `docs/reference/blastfile.md`, `docs/local.md` | `Syntax`'s offset is pinned to `0`, but the accepted pages promise "at that token"/"the SQL token that is wrong". | Every typo after the first token points at the query's start; the only benefit of offset 0 is the token echo that S2 says must go. |
| S13 | MINOR | `blastfile-dsl.md` §13.4.1, §13.4.4 | "never `panic`s, never `unwrap`s", but `pg_query::Error` also has `Conversion` (NUL), `Decode`, `InvalidJson`, `InvalidPointer`. | A `'…'` raw string may contain a NUL; a match on only `Error::Parse` with `unwrap` on the rest panics. `template.rs` is outside `blastfile/mod.rs`'s lint. |

## S1 — The Quick Fix is a policy change that broadens dangerous capabilities

The design defines the Quick Fix's text as:

> `blastit_sqlpolicy::parameterize(sql: &str) -> Option<String>` is the *inverse* direction of
> `redact_literals`: every quoted literal region becomes `$?` … The result is therefore **the
> canonical grant text**: the bytes `--learn` writes, the bytes `check` matches …
> — `blastfile-dsl.md` §13.4.6

and its trigger:

> The action is offered iff `parameterize(&q.raw)` is `Some(new)` — … a quoted literal outside
> `KNOWN_STATIC_SETUP_SQL`, which is the `LiteralQuery` case; or a hand-written `$1…$n` …
> — `blastfile-lsp-zed.md` §7A.2

`--learn` and `fold` refuse exactly the statements this turns into grants:

> `DANGEROUS_LEARN_FUNCTIONS`: `lo_export`, `lo_import`, `pg_file_rename`, `pg_file_unlink`,
> `pg_file_write`, `pg_logdir_ls`, `pg_ls_dir`, `pg_read_binary_file`, `pg_read_file`,
> `pg_stat_file`, `dblink`, `dblink_connect`, … 
> — `crates/blastit-sqlpolicy/src/learn.rs:38-65`

and `check_learnable` refuses `COPY … PROGRAM` (`learn.rs:286-288`) and any such function
(`learn.rs:292-294`). The page warns:

> Read it before pasting: `pg_read_file`, `COPY … PROGRAM`, `dblink` and `lo_import` are file or
> network capabilities, not ordinary statements.
> — `docs/reference/blastfile.md` (Grants)

A developer pastes `query "SELECT pg_read_file('/etc/passwd')"`. `Manifest::validate` refuses it
as `LiteralQuery` (`validate.rs`'s `check_queries`, ADR-0003 decision 2). The editor offers the
Quick Fix; `parameterize` produces `SELECT pg_read_file($?)`; `check_template` is a parse and
accepts it; the next `check` is clean. The grant now lets the application read **any** file on
the database host, not the one path the author pasted. The same transformation turns
`dblink('host=attacker.example …')` into `dblink($?)` (SSRF to any host) and
`lo_import('/etc/shadow')` into `lo_import($?)` (arbitrary file import). The design's claim that
the result is "the bytes `--learn` writes" is false: `--learn` would have refused the statement.
ADR-0003's amendment is therefore also wrong:

> `docs/design/blastfile-lsp-zed.md` §7A's Quick Fix is the editor's answer to this refusal, not
> a change to it.
> — `ADR-0003` amendment

It **is** a change: it changes a refusal into a grant and bypasses the dangerous-function policy.
"The Quick Fix only ever narrows" is not true for a capability argument: `'a@b.com'` → `$?` is a
broadening even for a predicate; for `pg_read_file` it is arbitrary file read.

**Fix.** `parameterize` (or the `codeAction` predicate) must reuse the `learn.rs` policy on the
**expanded/parsed** result — `DANGEROUS_LEARN_FUNCTIONS` and `is_copy_program` — and return
`None` for those statements. `find_dangerous_function` is already in the same crate; make it
`pub(crate)` for `template.rs`. Add C-cases asserting `[]` for
`query "SELECT pg_read_file('/etc/passwd')"`, `query "SELECT dblink('host=…', 'select 1')"` and
`query "COPY x FROM PROGRAM 'sh'"`; add the sentence to the hover/refusal so the user learns why
the fix is not offered.

## S2 — `Syntax` echoes `pg_query`'s token, reintroducing the leak `learn.rs` strips

> `Syntax` … `0` — pinned … ``the statement does not parse: <pg_query's message, one line, ≤ 120
> chars, control bytes escaped>``
> — `blastfile-dsl.md` §13.4.3

> The sentence still names the token (pg_query says `syntax error at or near "SELCT"`) …
> — `blastfile-dsl.md` §13.4.4

The existing code has already made the opposite decision:

> … the parser quotes the offending bytes (`syntax error at or near "s3cr3t"`), and the refusal
> is printed to a human.
> — `crates/blastit-sqlpolicy/src/learn.rs:79-81`

and `check_learnable` calls `sanitize_parser_message(&syntax(&error))` (`learn.rs:271`) so the
token never reaches `Display`. The new design reintroduces `(*result.error).message` verbatim.
`check_template` is `pub` (`blastfile-dsl.md` §4.4) and R31 says it "performs no policy"; the
literal gate is a **caller convention** in `check_queries`, not the function's contract. A future
caller (`--learn`, a check runner, a test, the LSP) can pass a literal-bearing statement, and the
`Syntax` sentence will print its value. `blastit check` writes refusals to stderr
(`docs/ci.md:22-24`), which is a CI log; the LSP publishes the same sentence as a diagnostic.
Even a literal-free statement can have a bare token that is a secret (`SELECT s3cr3t …`), which is
exactly the `s3cr3t` example in `learn.rs`.

R33 also says "one `kind` whose `Display` is one fixed sentence; `Display` never contains the
statement". `Syntax` is variable and does contain a token, so R33 is false as written.

**Fix.** Reuse `sanitize_parser_message` (or a fixed "the statement does not parse" plus the
offset) and make the no-literal precondition explicit in the API, not in a caller's ordering. If
the token name is needed for DX, expose `cursorpos` and use a fixed message; do not scrape the
message. Add a critical test that every `TemplateError::Display` is one of the six fixed prefixes
and contains none of the raw statement's tokens, including a direct call with
`SELECT 's3cr3t'`.

## S3 — The Quick Fix is offered for statements that still do not parse

> The action is offered iff `parameterize(&q.raw)` is `Some(new)` …
> — `blastfile-lsp-zed.md` §7A.2

> The reply must re-parse clean. Applying `newText` in place and running `blastfile::check` again
> must publish **no `query` diagnostic** …
> — `blastfile-lsp-zed.md` §7A.3

`parameterize` only replaces literals and runs `normalize`; it never calls `check_template`. The
node brief nevertheless requires "the result must parse and contain `$?`". Counterexamples:

| Query | `parameterize` result | What `check` says after Apply |
|---|---|---|
| `SET TIME ZONE 'Europe/Paris'` | `SET TIME ZONE $?` | `Syntax`: utility statements take no parameters |
| `COPY t FROM PROGRAM 'sh'` | `COPY t FROM PROGRAM $?` | `Syntax`: `PROGRAM` wants a string constant |
| `DO $$BEGIN … END$$` | `DO $?` | `Syntax` |
| `SELECT 'x'; SELECT 2` | `SELECT $?; SELECT 2` | `MultipleStatements` |
| `SELCT 'x'` | `SELCT $?` | `Syntax` |

The C-suite only exercises a normal `WHERE` (C2/C3), so it never sees these. The design's §13.4.1
example has the same problem:

> A template that is parseable but dangerous — `COPY x FROM PROGRAM $?`, `pg_read_file($?)`,
> `dblink(…)`, `lo_import($?)` — is **legal here** …
> — `blastfile-dsl.md` §13.4.1

`COPY … FROM PROGRAM` requires an `Sconst`; `COPY x FROM PROGRAM $1` is a syntax error, so the
example is not "parseable".

**Fix.** `parameterize` returns `None` unless `check_template(&new)` is `Ok(())` **and** the
dangerous-function policy (S1) is clear. Add C-cases for the four rows above; the applied-edit
assertion should start from a `LiteralQuery` fixture, not from a clean query.

## S4 — The `, ...` deletion must be token-exact and the pre-checks must run first

> The output is the input's characters, in order, with exactly two deletions and the numbering;
> **no re-spacing, no case folding, no comment removal**.
> — `blastfile-dsl.md` §13.4.2

> `IN ($?, ...)` → `IN ($1)` … the `, ...` is **deleted**
> — §13.4.2 rows 3–5

> After the rewrite: `pg_query::parse(&expanded)`; on `Ok`, `protobuf.stmts.len() == 1` is
> required …
> — §13.4.2

Rows 7 and 8 (empty and second statement) appear **after** the expansion rows, and nothing says
they run on the raw text before any deletion. If a cheap implementer deletes a character range
from the comma to the ellipsis, `IN ($?, ; ...)` loses the code `;`; the subsequent
`stmts.len() == 1` is then true for text that is really two statements. The design's row 6 and
"holding exactly one placeholder and then `...`" imply a refusal, but the order and the span are
not pinned.

**Fix.** State explicitly: rows 6–8 run on the **raw decoded** text **before** any output is
produced; rows 3–5 delete exactly the comma token and the ellipsis token, never a character
range. Add adversarial tests for `IN ($?, ; ...)`, `IN ($?, /* ; */ ...)`,
`IN ($?, $?, ...)`, `VALUES ((SELECT 1; SELECT 2)), ...` and a `;` inside a quoted region, and
assert the refusal or the exact output.

## S5 — `inner_position` is O(n·k) and the 64 Ki cap does not cover literal/redacted sites

> `pub escapes: Vec<u32>` … the decoded character indices whose source spelling is a two-character
> escape …
> — `blastfile-dsl.md` §13.4.5

> `for (i, c) in site.raw.chars().enumerate().take(offset): if site.escapes contains i: pos.col += 2`
> — §13.4.5

`Vec::contains` is linear. A 16 MiB `"…"` query of `\"` pairs has 8 M escapes; `inner_position` at
an offset near the end does ~6.4 × 10¹³ character checks. `MAX_TEMPLATE_CHARS` (65 536) is applied
inside `check_template` ("the input is bounded … before the rewrite", §13.4.1), but a
`LiteralQuery`/`RedactedQueryPlaceholder` site is computed **before** the template check and can
be on a 16 MiB query, bounded only by `MAX_SOURCE_BYTES`. A generated or hostile `Blastfile` can
hang `blastit check`, the boot or the LSP.

The memory claim is also light: raw + normalised + one `u32` per two source characters is about
3× the source, not "≤ 2× its own `query` text" (§13.4.5). It is bounded, but the design's number
is wrong.

**Fix.** Keep `escapes` ascending (it is built by scanning left to right) and walk it with a merge
pointer (`while escapes[j] < i { j += 1 }`), or use a `BTreeSet`/bitmap; state the O(n + k)
complexity. Add a critical test with a large escaped `"…"` query and a time bound.

## S6 — `parameterize` has no cap and can exceed the LSP frame budget

> one parse per hover/`codeAction` on top of the per-publish parse, bounded by `MAX_SOURCE_BYTES`
> and by `MAX_TEMPLATE_CHARS` for the statement that is actually inspected.
> — `blastfile-lsp-zed.md` §13.3

That sentence describes `Manifest::from_blastfile_str` + `check_template`, not `parameterize`.
The LSP obtains `q.raw` from `from_blastfile_str` (parse + lower, **no validate**), so a `query`
can be `MAX_SOURCE_BYTES` (16 MiB), not 64 Ki. `parameterize`'s signature has no cap:

> `fn parameterize(sql: &str) -> Option<String>` …
> — `blastfile-dsl.md` §4.4 / §13.4.6

`redact_literals` allocates `Vec<char>` (4 bytes/char → 64 MiB for a 16 MiB ASCII query),
`normalize` allocates more, and §7A.3 replaces the **whole interior** with the result. A `newText`
of 16 MiB with control characters can exceed `MAX_FRAME_BODY_BYTES` (33 619 968) once JSON-escaped;
there is no output cap. The single-threaded server can spend seconds and hundreds of MiB on one
`codeAction`, and the client may reject the frame.

**Fix.** `parameterize` returns `None` for a decoded statement over `MAX_TEMPLATE_CHARS`, using the
same `TooLong` sentence; the `codeAction` writer caps `newText`/the response body. Add a test with
a 65 537-char query and a 16 MiB query.

## S7 — The validator's grammar is PostgreSQL 17.7, not 18

> `check_template(&str)` … parses it with `pg_query::parse`, and requires `stmts.len() == 1`.
> — `blastfile-dsl.md` §3 R31

> **is this one parseable PostgreSQL 18 statement once Blastit's placeholders are expanded?**
> — §13.4.1

The host's pinned `pg_query 6.2.1` embeds:

```
#define PG_VERSION_NUM 170007
#define PG_VERSION_STR "PostgreSQL 17.7 (libpg_query)"
```

— `~/.cargo/registry/src/…/pg_query-6.2.1/libpg_query/src/postgres/include/pg_config.h:636,639`

The server in `docs/ci.md` is `postgres:18`. A legal PostgreSQL 18 statement can therefore be a
false `Syntax` (fail closed but a DX regression), and a statement whose 17.7 parse differs from
18 is a parser differential — precisely the class §17.5 claims to eliminate. A future
`pg_query` upgrade can change the accepted grammar without any design change. The design's
"PostgreSQL 18" claim is false and the user page inherits it.

**Fix.** State the actual grammar version in R31/§13.4.1 and pin it (crate + `libpg_query`
version); add a differential test against the `postgres:18` CVE harness for the syntax the docs
allow; use a PG 18 `libpg_query` if one is available, or document the 17.7 delta as a known false
red.

## S8 — O(file) per hover/keystroke and an unbounded C parser

> one parse per hover/`codeAction` on top of the per-publish parse, bounded by `MAX_SOURCE_BYTES`
> and by `MAX_TEMPLATE_CHARS` for the statement that is actually inspected.
> — `blastfile-lsp-zed.md` §13.3

`from_blastfile_str` re-parses and lowers the **whole** document on every hover/codeAction; a
16 MiB `Blastfile` (allowed) is 16 MiB of work per mouse move, on a single-threaded server with no
cache or cancellation. `check` on every `didChange` also parses every query with `pg_query`; a
16 MiB document split into 256 × 64 Ki (or 500 × 32 Ki) queries is that many C parser calls per
publish. P6 is a 250 ms test with 512 **position-less** refusals, not a budget for hundreds of
parsed statements.

`pg_query` is a C library; `catch_unwind` catches Rust panics, not a SIGSEGV. The design's §11
S2 test proves no `execve` via `/proc/…/children`, not memory safety, and §11's "a document is
parsed, never executed" plus §17.1's "a malformed file cannot exhaust the boot … no recursion
outside the grammar" are false for the C parser. A `Blastfile` in a cloned repository is fed to
this parser in the developer's unsandboxed process; that is a supply-chain DoS, and a
libpg_query memory-safety bug would be code execution.

**Fix.** Cache the `Manifest`/sites keyed by the tracked document version and reuse them for
hover/codeAction; add a per-document parse/time budget (or refuse a `from_blastfile_str` over a
smaller editor cap); state the C-parser residual and consider a worker with a timeout for the
editor path. Add a pathological 64 Ki SQL fixture and a 16 MiB valid-document timing test.

## S9 — C3's literal fixture is not a legal Blastfile

> C3 | `postgres { query 'SELECT id FROM t WHERE email = ''a@b.com''' }` (a literal, the `'…'`
> spelling) | one action; `newText` holds `$?` and no `@`; applied → clean
> — `blastfile-lsp-zed.md` §7A.5

> A statement containing a `'` goes in a `"…"` string, as in the trait example; `'…'` has no
> escapes.
> — `docs/reference/blastfile.md` (Grants)

> `RawStr` | `'…'` | every byte verbatim up to the next `'`; no escapes …
> — `blastfile-dsl.md` §5.1

The fixture terminates at the first `'` after `email = `; the next `'a@b.com'` starts a second
value. It is a parse error, not a `LiteralQuery` with a quick fix. C2 tests `$1`, not a literal,
so the **only** positive test for the literal path — the path that can broaden a dangerous call
(S1) — cannot run. A test that cannot fail is not a proof.

**Fix.** Use `query "SELECT id FROM t WHERE email = 'a@b.com'"` (the spelling a real statement
with a `'` must use), assert one action, `$?`, no `@`, and a clean re-check. Add a second C-case
with `pg_read_file('/etc/passwd')` expecting `[]` after S1's fix.

## S10 — `COPY … FROM PROGRAM $?` is not parseable; the "legal here" example is false

> A template that is parseable but dangerous — `COPY x FROM PROGRAM $?`, `pg_read_file($?)`,
> `dblink(…)`, `lo_import($?)` — is **legal here** …
> — `blastfile-dsl.md` §13.4.1

PostgreSQL's `COPY … FROM PROGRAM` takes a string constant (`Sconst`), not a parameter;
`COPY x FROM PROGRAM $1` is a syntax error. The only `COPY … PROGRAM` form has a quoted literal,
which `check_queries` refuses as `LiteralQuery` (ADR-0003 decision 2). So the production path
cannot grant `COPY … PROGRAM` at all, and the R31 example is false. A cheap implementer writing
the R31 test against `COPY x FROM PROGRAM $?` will either see it fail or be tempted to special-case
the validator — the second is a real weakening.

**Fix.** Use `pg_read_file($?)` and `dblink($?)` for the "parse, not policy" proof; state that
`COPY … PROGRAM` requires a literal and is refused by the literal gate, or test `check_template`
directly on `COPY … FROM PROGRAM 'cmd'` and explain that the manifest gate, not the template,
refuses it.

## S11 — `QuerySite.raw` is pre-redaction text on a `Debug`-derived model

> `pub raw: String` — The raw decoded statement, before `normalize` — the text `check_template`
> and `parameterize` run on.
> — `blastfile-dsl.md` §13.4.5

`PgGrant` and `Manifest` derive `Debug` (`model.rs:164,894`); a new field will too. A
`from_blastfile_str` on a `LiteralQuery` document records the literal in `raw`; a future
`format!("{:?}", manifest)` or a test failure message prints it, violating "never log query
strings … SQL parameter values" (`security-invariants` §5). No current path does this, hence
MINOR.

**Fix.** Make `raw` private with an accessor, or give `QuerySite` a redacting `Debug`/a
`Sensitive<String>` newtype. Add a test that `format!("{:?}", Manifest)` contains no literal from
a `LiteralQuery` fixture.

## S12 — Offset 0 contradicts the accepted pages' "at that token"

> `Syntax`'s offset is **pinned to `0`**: the first character of the decoded statement.
> — `blastfile-dsl.md` §13.4.4

> a typo, a missing `FROM` or a stray `)` is a `Blastfile:line:col` diagnostic **at that token**
> — `docs/reference/blastfile.md` (Grants)

> In a `query` it sits on the SQL token that is wrong …
> — `docs/local.md` `## Editors`

For any typo after the first token the squiggle is at the query's first character, so the pages
are false. The only thing offset 0 buys is the token echo, which S2 must remove; keeping the
false promise after removing the token would leave neither a correct site nor a safe message.

**Fix.** Expose `cursorpos` if a safe binding is possible, or change the pages (MB.34.1) to "the
statement's first token" and drop the "at that token" wording. Do not claim the token site the
design does not have.

## S13 — `pg_query`'s other error variants are not specified

> It never prints the statement, never `panic`s, never `unwrap`s …
> — `blastfile-dsl.md` §13.4.1

§13.4.4 enumerates `Error::Parse`, `Scan` and `Split` only. `pg_query::Error` also has
`Conversion(NulError)`, `Decode`, `InvalidJson` and `InvalidPointer`
(`pg_query-6.2.1/src/error.rs:5-26`). A `'…'` raw string may contain a NUL byte (`RawStr` is
"every byte verbatim"), so `pg_query::parse` can return `Conversion` on a legal Blastfile. A
cheap implementer matching only `Parse` and `unwrap`/`expect`-ing the rest panics; `template.rs`
lives in `blastit-sqlpolicy`, outside `blastfile/mod.rs`'s `#![deny(unwrap_used, …)]`.

**Fix.** Enumerate every `pg_query::Error` variant and map all of them to a fixed `Syntax` (or a
new `kind`) with no text; put the same `deny(unwrap_used, expect_used, panic)` lints at the top of
`template.rs`; add tests for a NUL-bearing query and a `pg_query` error that is not `Parse`.

## Checked and sound (so the owner does not re-litigate these)

- **The template validator is a parse, not a second allowlist.** It refuses no table, column,
  function or literal; `pg_read_file($?)` and `dblink($?)` are legal here, as intended. The
  policy leak is the **Quick Fix** (S1), not `check_template`. The `COPY … PROGRAM $?` example is
  the one false part (S10).
- **Numbering reuse cannot change a grant.** `normalize` folds every `$n` to `$?`
  (`normalize.rs:56-62, 140-146`), the grant stores `normalize(raw)` (`lower.rs:687-689`), and
  `check_template` returns `()` — the expanded text never escapes the function, so `$1` + `$?`
  duplicate numbering is parse-only. Recommend a test that pins the `()` return and that the
  stored query is `normalize(raw)`, not the expanded text.
- **The rewrite caps** (`MAX_TEMPLATE_CHARS`, `MAX_TEMPLATE_PARAMS`, `u32::checked_add`, the
  4 × + 8 buffer) bound the rewrite itself. The span walk (S5) and `parameterize` (S6) are
  outside them.
- **The LSP protocol additions are narrow.** `context.diagnostics` is ignored, there is no
  `workspace/applyEdit`/`codeAction/resolve`, `codeActionProvider` is the only new key, the client
  applies the edit, `caps::hold_none()` runs before `lsp::run()`, and the four stderr literals do
  not carry document text. The holes are the Quick Fix's semantics (S1/S3) and the parser/CPU
  path (S8).
- **Production stays fail-closed.** `QueryTemplate` is a `ManifestError`; a bad query refuses the
  boot; `blastit dev` and production both go through `Manifest::validate`; the `blastit`/
  `blastit dev` split is unchanged. A `pg_query` C crash is a crash (no listener), not a
  fail-open, but it is a DoS and S8's claim needs softening.
- **`sql_string` is not a false-red source at the token level.** The grammar's only comment extra
  is `#[^\n]*` (`editors/tree-sitter-blastfile/grammar.js:21,141`); `sql_operator`/`sql_other` at
  precedence 2/1 cover every other `[^']` character, and the corpus's `query-hash.txt` and
  `query-sql.txt` (`-- note`) measure it. The "exactly `'[^']*'`" union argument is sound. A
  `#` immediately before the closing quote is the one missing corpus case; add it as polish, not
  a blocker.
- **`query` leaving the shared `keyword` rule** is measured and documented; the contextual-name
  cases (`view query`, `trait query`, `postgres query`) are unchanged. No new strictness.

VERDICT: CHANGES
