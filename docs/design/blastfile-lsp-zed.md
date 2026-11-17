---
status: In review
owner: designer
node: MB.22.2
reviewers: [security, implementability, testability, consistency]
---
# `blastit lsp` and the Zed Blastfile extension: protocol, spans, layout, hover, grammar, queries

This document turns the accepted editor DX (`docs/local.md` `## Editors`,
`docs/reference/cli.md` `## blastit lsp`, MB.22.1) into the thing `MB.22.3`–`MB.22.5`
build. Every protocol, extension-schema and theme-capture statement below is copied
from `docs/research/2026-11-16-zed-blastfile-lsp.md` (verified 2026-11-16) — nothing
here re-derives a Zed fact. Where the research left a decision open this document
decides it and says which test proves it: the **CRLF formatter ending** (§6), the
**`Worktree` resolution of the server** (§8), the grammar's **error tolerance** (§9),
and Zed's **`format_on_save` / `formatter` route** (§6.5, proved by §12.6). One
research number is stale and this document uses the code's: §7 of the
research quotes the pre-`MB.21` 1 MiB `MAX_SOURCE_BYTES`, while the parser's cap is
16 MiB (`crates/blastit-manifest/src/blastfile/mod.rs:202`,
`docs/design/blastfile-dsl.md` §4.5), and `MAX_FRAME_BODY_BYTES` is derived from it.

Two laws govern every row:

1. **Invariant** — a claim a test can make is written as an assertion, in the words
   the test asserts, not as prose about intent.
2. **One canonical answer** — for every question the editor can ask there is exactly
   one sentence: the sentence `blastit check` prints for an error (§5.1–§5.3), the
   sentence the **boot** prints for a warning (§5.4 — `check` discards warnings at
   `check.rs:136`, so the boot is their one printer), or the layout `blastit fmt`
   writes (§6). The editor is never a second opinion.

The server is ~700 lines of `serde_json` + `std::io` in the binary that already
exists. No dependency is added, no async runtime joins the LSP path, no LSP crate is
used, no second binary and no second grammar repository exist. `ADR-0013` records
those four refusals; this document is the detail.

## 0. Developer Experience Target (Local, CI/CD, Deploy)

The three moments are already written and accepted; this design owes each one the
behaviour in the right-hand column and nothing else.

| Moment | What the developer does (accepted page) | What this design must make true | §
|---|---|---|---|
| **Local** | installs **Blastfile** in Zed ≥ 0.205 (or `zed: install dev extension` on `editors/zed`), opens `./Blastfile`, types `body = json` | colours, bracket matching, an outline, and a squiggle at the `=` whose text is exactly `` `=` is not Blastfile syntax; write the value after the keyword: `body json` `` — the `blastit check` sentence without the `Blastfile:<line>:<col>: ` prefix | §4, §5 |
| **Local** | merges `{ "languages": { "Blastfile": { "format_on_save": "on" } } }` into `.zed/settings.json` and saves a mis-indented line | one whole-document `TextEdit` from `blastfile::fmt`, so the line snaps back; an already-canonical buffer returns `[]`; a broken buffer returns `null` (never an editor error dialog) | §6 |
| **Local** | hovers `unrestricted` or `body raw` | a markdown paragraph copied from `docs/reference/blastfile.md`, including "warns at boot" | §7 |
| **Local** | uses any other editor | `blastit lsp` on stdin/stdout with no arguments, the extensionless file name registered by the client, no `npm`/Tree-sitter CLI/`cargo`/`wasi-sdk` on the user's machine | §2, §8, §12 |
| **CI/CD** | runs `blastit check`, `blastit fmt --check`, the grammar corpus and the static extension guard | the editor's sentence and the CLI's sentence are one string (§5); the grammar and query files are checked by `tree-sitter test`/`query` and by `scripts/check_zed_extension.py`, not by a human reading them (§12) | §5, §9, §10, §12 |
| **Deploy** | runs production `blastit`; never runs `blastit lsp` | `blastit lsp` drops every file capability, reads no `BLASTIT_*` variable of its own, opens no `/run` path and binds no listener, and is unreachable from the stage-0 boot path; a `Blastfile` is policy, never input, so the editor executes nothing | §1, §11 |

## 0.1 Goals and non-goals

**Goals.** One subcommand that speaks the subset of LSP 3.17 Zed needs; diagnostics
that are the CLI's own sentences at the CLI's own line and column; canonical
formatting that cannot disagree with `blastit fmt`; hover for the directive
vocabulary; one in-repo grammar and four query files that make `./Blastfile` a
first-class Zed language; every one of those facts asserted by a test that fails when
the fact is removed.

**Non-goals (v1).** Completion, `textDocument/definition`, `documentSymbol`,
`codeAction`, pull diagnostics (`diagnosticProvider`), incremental sync, semantic
tokens, `injections.scm` for SQL, a formatter for any other language, a VS Code
extension, a second grammar repository, and any token in a `Blastfile` that the
lexer of `docs/design/blastfile-dsl.md` does not already accept.

## 1. Module boundaries

Four crates' worth of surface, three files new in `crates/` (`lsp.rs`, `blastfile/sites.rs`,
`tests/critical_lsp.rs`), one visibility change, **no path
widening**. Nothing else moves.

| # | Code site | Owns | Does **not** own |
|---|---|---|---|
| 1 | `crates/blastit/src/lsp.rs` (**new**, ≤ 750 lines) | the frame reader/writer (§3), the lifecycle (§2), dispatch, the `documents` map, UTF-16 conversion (§4), diagnostics assembly (§5), formatting (§6), hover (§7) | manifest syntax, layout, or the meaning of any directive |
| 2 | `crates/blastit/src/lib.rs` | one line: `pub mod lsp;` | — |
| 3 | `crates/blastit/src/cli.rs` | `Command::Lsp` (+ `from_argv` arm `Some("lsp")`), `LSP_USAGE`, `parse_lsp`, the `main` arm, one added `USAGE` line, `LSP_FAILURE = 2`, and **one capability line**: the `Lsp` arm calls `blastit_sys::caps::hold_none()` before `lsp::run()` (§11 S3) | the protocol: `run_lsp` is `lsp::run()` and returns `u8` |
| 4 | `crates/blastit/src/main.rs` | **unchanged** — `cli::main` already dispatches every subcommand | — |
| 5 | `crates/blastit-manifest/src/error.rs` | `ManifestError::message()` — the `Display` sentence with the `prefix(position())` removed (§5.2) — and the replacement of `position()`'s wildcard arm with the explicit position-less list (§1 note) | any new variant, any new field on `ManifestWarning` |
| 6 | `crates/blastit-manifest/src/blastfile/sites.rs` (**new**, ≤ 120 lines) | `sites(cst, source, warnings) -> Vec<Position>`: the CST site of each warning, **in `warnings`' order** (§5.5) | the warning triggers (they stay in `validate::warnings`) |
| 7 | `crates/blastit-manifest/src/blastfile/mod.rs` | `MAX_SOURCE_BYTES` goes `pub(crate)` → `pub` (§3.1), and one new entry point `check(source, base)` that returns the manifest, its warnings **and** their sites (§5.5) | — |
| 8 | `crates/blastit-manifest/src/model.rs` | **unchanged** — no span field, no new method (§5.5) | — |
| 9 | `crates/blastit-manifest/src/validate.rs` | **unchanged**: triggers and emission order stay exactly as they are, so boot log lines and every existing test are untouched | — |

The exact signatures the two halves share:

```rust
// crates/blastit-manifest/src/blastfile/mod.rs
/// A `Blastfile` read the way an editor needs it: the model, its warnings, and
/// the source position of each warning, in the same order.
pub struct Checked {
    pub manifest: Manifest,
    pub warnings: Vec<ManifestWarning>,
    pub sites: Vec<Position>,
}

/// Parses, lowers, validates, and locates each warning. `check(source, base)` is
/// `Manifest::parse_blastfile` plus the CST walk that `parse_blastfile` drops.
pub fn check(source: &str, base: u32) -> Result<Checked, ManifestErrors>;

// crates/blastit-manifest/src/error.rs
impl ManifestError {
    /// The `Display` sentence without the `"<line>:<col>: "` prefix.
    pub fn message(&self) -> String;
}

// crates/blastit/src/lsp.rs
pub fn run() -> u8;                                   // reads stdin, writes stdout
fn write_message(out: &mut impl Write, value: &serde_json::Value) -> std::io::Result<()>;

// crates/blastit/src/cli.rs
fn parse_lsp(arguments: &[String]) -> Result<Subcommand<()>, String>;   // same shape as parse_fmt
```

`ManifestError::position()` exists (`error.rs:1403`) but its `match` ends in
`_ => Position::NONE` (`error.rs:1468`), so a new `at`-carrying variant would compile
without a decision. `MB.22.3` replaces that one arm with explicit arms for **every**
variant that carries no `at` field — which is two families, both listed in §4.2:

- the whole-file refusals and `InvalidApp` (`TooLarge`, `TooManyTokens`,
  `TooManyTrivia`, `TooManyRoutes`, `UidBaseTooHigh`, `NotRegularFile`, `Unreadable`,
  `InvalidApp`), plus the older `Location`-less route/mount refusals
  (`DuplicateRouteName`, `InvalidRouteName`, `EmptyPath`, `InvalidMethod`,
  `EmptyMethods`, …); and
- the **eighteen `Location`-carrying validation variants** (`UnknownGroup`,
  `GrantOnUnknownProxy`, `UnrestrictedGroup`, `UnrestrictedWithQueries`,
  `UnrestrictedWithContent`, `UnusableTable`, `RedactedQueryPlaceholder`,
  `LiteralQuery`, `StaticMountPrefixInvalid`, `StaticFallbackInvalid`,
  `StaticRootInvalid`, `StaticRootUnusable`, `StaticIndexInvalid`,
  `StaticImmutablePattern`, `StaticDeadDeclaration`, `PhpEntryInvalid`,
  `PhpWorkersInvalid`, `PhpDeadDeclaration`).

Every such arm returns `Position::NONE`, so the change is a compile-time guard: a new
variant cannot be added without a decision, and no rendered sentence changes.

### 1.1 Why a subcommand, not a second crate or binary

| Property | `blastit lsp` (chosen) | a `blastit-lsp` binary |
|---|---|---|
| Release artifacts | one binary + one wheel, already shipped | a second artifact to build, sign, publish and keep in step |
| Server resolution | `blastit` on `PATH`, or `uv run blastit lsp` — the extension needs **one** name | a second name for the extension, its error message and the docs to resolve |
| Code sharing | `blastit-manifest` is already a dependency of `crates/blastit` | the same dependency, plus a crate to plumb it |
| Privilege surface | the `Lsp` arm drops every file capability **before** it reads a byte (`blastit_sys::caps::hold_none()`, the call `--role=dbproxy` already makes at `cli.rs:638`), so the editor's bytes are parsed by a process holding nothing (§11 S3) | the same call in a second binary — the capabilities belong to the process image either way, so splitting the binary buys nothing here |
| Python-free standalone | `make rs-no-python` already produces a `blastit` that speaks it | a second `--require-no-python` promise to keep |

The subcommand is reachable from both installations by construction:
`python/blastit/__main__.py` calls `_blastit.main(sys.argv[1:], …)`, which is
`cli::main` with `argv = ["lsp"]`; `main.rs` calls the same function with
`Host::binary()`. **Assertion:** `blastit lsp --help` exits `0` and its first line is
`usage: blastit lsp`; a test in `crates/blastit/tests/critical_lsp.rs` spawns the test
binary's `lsp` subcommand and asserts the handshake (§12.1).

### 1.2 What each boundary may not do

- `lsp.rs` may not name `crate::stage0`, `crate::stage1`, `crate::host::Host`,
  `crate::config`, `blastit_sys`, or `std::env` — one assertion in `critical_lsp.rs`
  over `include_str!("../src/lsp.rs")` (`the_lsp_module_is_a_text_tool`) checks every
  forbidden substring from a fixed list.
- `lsp.rs` may not read `./Blastfile`, any other file, a directory entry or a socket:
  the only input is stdin. The list it is grep-checked against is
  `["std::fs", "std::path", "std::process", "std::os::unix::net", "libc", "rustix",
  "blastit_sys", "crate::stage0", "crate::stage1", "crate::host", "crate::config",
  "std::env", "include!"]` — `libc`/`rustix` are already dependencies of
  `crates/blastit`, and `std::process` is how a "no child process" claim would be
  broken.
- `blastit-manifest` may not learn what an editor is: no `Uri`, no LSP type, no
  `version`, and no formatting option crosses that boundary. `message()` returns a
  `String`; a site is a `Position`; `blastfile::check` is the only new entry point.

R-ids for this section: **R1** (subcommand in `cli.rs`), **R2** (three new files, one
visibility change, **no path widening**, no dependency), **R3** (`lsp.rs` names no
privilege/config/env/filesystem module), **R3a** (`position()` has no wildcard arm).

<!-- APPEND -->
## 2. The exact message set, the capability document, and the error codes

### 2.1 Frames

Every message, in both directions, is

```
Content-Length: <N>\r\n
\r\n
<N bytes of UTF-8 JSON>
```

`N` is the **UTF-8 byte** length of the body — never a `char` count (§3.1 asserts
this with a body containing `✓`). No other header is written. The reader accepts
exactly one `Content-Length` line (name compared ASCII case-insensitively, one space,
value all ASCII digits); every other header line is ignored.

### 2.2 The ten messages, and the three tolerated ones

`id` is kept as `serde_json::Value` for the whole session and echoed **verbatim**:
a numeric id comes back a number, a string id a string. **The notification rule is one
rule, stated once:** a message is a *notification* — and is **never** answered — exactly
when it is a well-formed JSON-RPC 2.0 message with a `method` string and **no** `id` key.
Everything else that arrives with a body gets a reply:

- an explicit `"id": null` is an invalid request → `-32600` with `id: null` (§2.4), so a
  client that sends `{"id":null,"method":"initialize"}` is answered instead of hanging;
- an id-less object with **no** usable `method` (an array, `jsonrpc != "2.0"`, no
  `method` string) is an invalid request → `-32600` with `id: null`;
- an id-less object with a `method` and unusable params is still a notification: dropped,
  never answered (item 4 below).

| Method | Kind | Params read | Behaviour | Result |
|---|---|---|---|---|
| `initialize` | request | none (everything in it is ignored, incl. `rootUri`, `capabilities.general.positionEncodings`, `initializationOptions`) | sets `initialized`; a second `initialize` answers `-32600` | the capability document (§2.3) |
| `initialized` | notification | — | ignored | — |
| `workspace/didChangeConfiguration` | notification | — | ignored (Zed sends it once, right after `initialized`) | — |
| `window/workDoneProgress/create` | request | — | ignored | `null` |
| `textDocument/didOpen` | notification | `textDocument.{uri, text}`, `version` (absent ⇒ `0`); `languageId` ignored | **inserts** the document (§2.5) and publishes its diagnostics | — |
| `textDocument/didChange` | notification | `textDocument.{uri, version}`, `contentChanges` | requires **exactly one** change object carrying `text` and no `range`; **replaces** an existing document (never inserts) and publishes. An unknown `uri` or a change carrying `range`/`rangeLength` is refused visibly (§2.5); zero or more than one change object is dropped silently (§2.5) | — |
| `textDocument/didClose` | notification | `textDocument.uri` | drops the document, publishes `[]` with **no** `version`; an unknown `uri` is ignored | — |
| `textDocument/formatting` | request | `textDocument.uri`; `options` ignored (§6.4) | §6 | `[]`, one `TextEdit`, or `null` |
| `textDocument/hover` | request | `textDocument.uri`, `position` (UTF-16) | §7 | `Hover` or `null` |
| `textDocument/definition` | request | — | **declined**: not advertised, and a probe is not a protocol violation (§7.4) | `null` |
| `shutdown` | request | — | sets `shutdown`; later requests answer `-32600` | `null` |
| `exit` | notification | — | leaves the loop | process exits `0` iff `shutdown` was received, else `1` |
| `$/cancelRequest`, `$/setTrace` | notification | — | ignored | — |
| any other `$/…` | request | — | answers `-32601` | error |
| any other string | request or notification | — | request ⇒ `-32601`; notification ⇒ ignored | error / — |

Four deliberate differences from the research's "tolerate/ignore" list, each with its
reason:

1. `window/workDoneProgress/create` is a **request**; a request with no reply can hang
   a client. It is answered `{"result":null}`, which is the spec's result.
2. A **notification** whose method is unknown is dropped silently; answering it would
   be a protocol violation and a stray frame on `stdout`.
3. `$/setTrace` and `$/cancelRequest` are ignored even when they carry a value,
   because every request in this server completes before the next frame is read, so
   there is nothing to cancel (`-32800` is therefore never emitted).
4. Two `didChange` shapes are refused **visibly** because the client can act on them —
   an unknown `uri`, and a change carrying `range`/`rangeLength`: each publishes one
   severity-2 diagnostic at `(0,0)` (§2.5) and drops the `uri` from the map, so the next
   `hover`/`formatting` answers `null` instead of a stale answer. A `didChange` whose
   `contentChanges` holds **zero or more than one** object is a broken client, not a
   state the server can recover into: it is **dropped silently**, publishes nothing, and
   the document keeps its last stored text.

### 2.3 The capability document, byte for byte

```json
{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"positionEncoding":"utf-16","textDocumentSync":{"openClose":true,"change":1,"save":false},"documentFormattingProvider":true,"hoverProvider":true},"serverInfo":{"name":"blastit","version":"0.2.0"}}}
```

- `positionEncoding` is `"utf-16"` **explicitly**: Zed offers only that, and the spec
  lets a server that received `["utf-16"]` return only `utf-16`. It is also the value
  Zed ignores (§4.1) — being explicit is for other clients (Helix, Neovim).
- `textDocumentSync` is an **object**, not `1`: omitting it (`None`) makes Zed send no
  `didChange` at all, and `change: 1` is full sync.
- `save: false` — no `textDocument/didSave` is ever sent or handled.
- `serverInfo.version` is `env!("CARGO_PKG_VERSION")` (today `0.2.0`); **assertion:**
  the value equals the crate version and `name` equals `"blastit"`.
- No `diagnosticProvider` (push only), no `completionProvider`, no
  `definitionProvider`, no `documentSymbolProvider` (§7.4 defers definition).
- No `initialize` `clientInfo`/`processId` is stored: nothing uses the pid, and
  killing a pid read from stdin would be a privilege this tool never takes (§11).

### 2.4 JSON-RPC error codes

| Code | When | `id` | Session after |
|---|---|---|---|
| `-32700` | the body is not UTF-8, or is not JSON at all (§3.4 row 3, and row 4's not-JSON half) | `null` | **keeps serving** |
| `-32600` | not a JSON object; `jsonrpc != "2.0"`; no `method` string; `initialize` twice; any request after `shutdown` | echoed, `null` when absent | keeps serving |
| `-32601` | unknown method, or a `$/…` **request** | echoed | keeps serving |
| `-32602` | known method, unusable params: missing `textDocument.uri`/`text`; `hover` without a numeric `position`; `formatting` without `uri` | echoed | keeps serving |
| `-32603` | reserved; **never emitted** in v1 — no path in `lsp.rs` can fail without a more specific code | — | — |
| `-32002` | any **request** before `initialize` completed | echoed | keeps serving |
| `-32800` | reserved; **never emitted** (§2.2 row 3) | — | — |

The error object is `{"jsonrpc":"2.0","id":<echoed>,"error":{"code":N,"message":"<one line>"}}`.
`message` is a fixed ASCII sentence from a `&[(i32, &str)]` table in `lsp.rs`; it never
contains document text, a URI, or a `serde_json` error string (§11.3).

**The malformed-frame rule, verbatim:** a frame whose header block is well-formed and
whose body is not JSON answers `-32700` with `id: null` and **keeps serving**; the
loop continues at the next `Content-Length`. A frame the reader cannot resynchronize
from (rows 1–2 of §3.4) answers `-32700` with `id: null` and then ends the session
with exit `1`, because the byte offset of the next frame is unknown.

**Process exit codes** (a different table from the JSON-RPC codes above):

| Exit | When |
|---|---|
| `0` | after `shutdown`, whatever ended the stream afterwards (EOF or `exit`) |
| `1` | EOF or `exit` without `shutdown`; an unframeable stream (§3.4 rows 1–2); or the `Lsp` arm could not prove the process holds no capability (`caps::hold_none()` failed), which writes the `internal error` sentence of §11.3 (§11 S3) |
| `2` | `blastit lsp` was given an argument it does not take (`LSP_FAILURE`, the `FMT_FAILURE` convention) — a CLI error, not a session |

A test cannot observe a `clippy` deny at run time, so the two source-shape invariants
(`position()`'s wildcard arm, `lsp.rs`'s forbidden names) are asserted by grep over
`include_str!`; every code in the table above is asserted by a real session (§12.1).

### 2.5 The document map

```rust
struct Document { uri: String, version: i64, text: String }
struct Server   { documents: BTreeMap<String, Document>, initialized: bool, shutdown: bool }
```

- `uri` is an opaque `String`: compared, echoed, never parsed, never turned into a
  path, never opened (§11.1).
- `version` is `i64`; a `didOpen` without one is version `0` (Zed always sends `0`). A
  `didChange` with a lower version is still applied — the server never reorders
  (a notification cannot be answered, and the text is what the client has).
- **`didOpen` inserts; `didChange` replaces.** Two `didChange` shapes are refused with
  a visible notice and no insertion: an unknown `uri` publishes
  `blastit lsp has no open document for this file; reopen it`, and a change carrying
  `range`/`rangeLength` publishes `incremental sync is not supported; reopen the file`
  **and** removes the `uri` from the map (its old text's diagnostics must not survive
  the refusal). Both are severity 2 at `(0,0)`. The map therefore has exactly as many
  entries as `didOpen`s that were accepted, and no notification other than `didOpen`
  can add one.
- `MAX_DOCUMENTS = 32` (§3.1) bounds the **count**: a `didOpen` that would open the
  33rd document is not stored and publishes
  `[{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},"severity":2,"source":"blastit","message":"blastit lsp tracks at most 32 open Blastfiles; close one"}]`
  for that `uri` — a visible, actionable bound rather than silent memory growth.
- `MAX_DOCUMENT_BYTES = MAX_SOURCE_BYTES` bounds **each entry**: a `didOpen` or
  `didChange` whose `text` is longer than the parser's own cap is not stored at all.
  Its diagnostics are still published — `Manifest::parse_blastfile` refuses from the
  length before lexing, so the `TooLarge` sentence appears at `(0,0)` — and the `uri`
  is removed from the map, which means the next `hover`/`formatting` answers `null` and
  the developer must reopen the file. 32 × 33 MiB is therefore not reachable: the
  worst case is 32 × 16 MiB, and the notice makes the refusal visible.
- The three visible notices (33rd document, oversized text, unhonourable `didChange`)
  are published **without** a `version` key: they describe the session's state, not a
  text the client sent, and a version would invite the client to trust them as a stale
  set for a document the server no longer holds.

R-ids: **R4** (`Content-Length` counts UTF-8 bytes), **R5** (id echoed verbatim, no
reply to a notification, explicit `null` id is `-32600`), **R6** (capability document
exactly as §2.3), **R7** (the error-code table, including `-32700` + `id: null` and the
keep-serving rule), **R8** (`MAX_DOCUMENTS` + `MAX_DOCUMENT_BYTES` with their visible
notices, and the insert-only-by-`didOpen` rule).

## 3. Bounds and I/O

### 3.1 Constants (one table, `lsp.rs` top)

| Constant | Value | Why this value | Assertion |
|---|---|---|---|
| `MAX_FRAME_HEADER_BYTES` | `8_192` | Zed writes one 19-byte header; 8 KiB is 400× headroom and `O(1)` memory. The cap **includes** the `\r\n\r\n` terminator | a block of exactly 8 192 bytes (terminator included) is accepted and a 8 193-byte one is refused (§3.4 row 1) — both boundaries asserted |
| `MAX_FRAME_BODY_BYTES` | `2 * MAX_SOURCE_BYTES + 65_536` = `33_619_968` | a legal 16 MiB `Blastfile` is newline-heavy, and JSON escapes `\n` to two bytes; `2×` plus one chunk admits every legal document **without** being derivable by guessing: the constant is written from the parser's own `pub const MAX_SOURCE_BYTES` | `MAX_FRAME_BODY_BYTES == 33_619_968`, asserted as a literal so a change to the parser cap is a test failure, not a silent resize |
| `MAX_DOCUMENTS` | `32` | one open `Blastfile` per buffer; 32 is 6× the realistic maximum and bounds the map's **count** | the 33rd `didOpen` publishes §2.5's notice |
| `MAX_DOCUMENT_BYTES` | `MAX_SOURCE_BYTES` = `16_777_216` | bounds each entry's **text**, so 32 buffers cannot reach `32 × MAX_FRAME_BODY_BYTES`; the parser refuses this length anyway, so storing more would be dead weight | §2.5's oversized-text row: a 16 777 217-byte document publishes `TooLarge` at `(0,0)` and is not stored |
| `MAX_DIAGNOSTICS` | `512` | a broken file past 512 squiggles is unreadable; the list is bounded before it is serialized | §5.7 |
| `READ_CHUNK` | `8_192` | the fixed scratch buffer for header bytes; **no allocation is sized by `Content-Length` before the cap check** | a refused body allocates 0 bytes of body (§3.4 row 2) |
| `HEADROOM` | `65_536` | one chunk of slack for the frame around a maximal document, which is the JSON-RPC envelope plus a `uri` of up to 64 KiB; a longer `uri` is refused, which is acceptable because every client's `uri` is a filesystem path | folded into `MAX_FRAME_BODY_BYTES` |

### 3.2 The read algorithm

1. Read into a `[u8; READ_CHUNK]`-backed buffer **until `\r\n\r\n`**, refusing as soon
   as the bytes read reach `MAX_FRAME_HEADER_BYTES` without a terminator (§3.4 row 1). The
   cap counts the terminator itself, so a block of exactly 8 192 bytes ends in
   `\r\n\r\n` and is accepted. The header text is then parsed by scanning
   `\r\n`-terminated lines.
2. Exactly one `Content-Length` line is required. Its value is parsed with ASCII-digit
   checks into `usize`; more than 10 digits, a sign, a space inside, or a **second**
   `Content-Length` line is a frame error (§3.4 row 2).
3. `N > MAX_FRAME_BODY_BYTES` ⇒ refuse **before allocating** (§3.4 row 2).
4. Otherwise `let mut body = vec![0u8; N]` and `read_exact` the whole body — read by
   length, **never** by newline: a `Blastfile` contains newlines inside `'…'` strings
   and a body must not be split on them.
5. `String::from_utf8(body)` then `serde_json::from_str::<Value>`; both failures are
   `-32700` (§3.4 rows 3–4).

`vec![0u8; N]` is the **only** allocation whose size comes from the client, and it
happens after the cap check. Every other buffer is `READ_CHUNK` or grows with the
document the client already sent.

### 3.3 The write algorithm

`write_message(out, &value)`:

1. `let body = serde_json::to_vec(value)?` — every response is built by this crate
   from a literal shape, so its size is `O(document)` only for `publishDiagnostics`,
   which is already bounded by `MAX_DIAGNOSTICS` and `MAX_FRAME_BODY_BYTES`.
2. `write_all(b"Content-Length: ")`, the decimal length, `b"\r\n\r\n"`, the body.
3. `flush()` after every message: Zed reads a frame at a time and a buffered reply
   would read as a hung server.

**stdout carries frames and nothing else.** `lsp.rs` starts with

```rust
#![deny(clippy::print_stdout)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::todo)]
#![deny(clippy::indexing_slicing)]
```

so a stray `println!`, an `unwrap()` on a `serde_json` shape, or a `v[i]` cannot
compile. stderr carries at most **four** fixed lines in a whole session, each
`blastit lsp: <reason>` with a reason from a `&'static str` table: `the input ended
inside a frame`, `the header block is too long`, `the body is too long`, and
`internal error`. No document text, no URI, no JSON, no value of any kind (§11.3).

The fourth line is the **panic hook**, installed once at the top of `run()`:

```rust
std::panic::set_hook(Box::new(|_| eprintln!("blastit lsp: internal error")));
```

and every call into `blastit-manifest` for one document is wrapped in
`std::panic::catch_unwind(AssertUnwindSafe(…))`. The default hook would print the
panic payload, and a `ManifestError`'s `Debug` carries quoted document text — so the
hook is replaced *before* any document is parsed, and a caught panic becomes a
severity-1 diagnostic at `(0,0)` (`` blastit lsp: internal error; run `blastit check` ``)
instead of a dead session (§11 S5, S9). The deny list above is `lsp.rs`'s own;
`blastit-manifest`'s crate-level lints are the workspace's `warn`, which is exactly
why the guard exists rather than a promise.

**Assertion:** a complete session with a valid file writes **zero** bytes to stderr,
and `include_str!("../src/lsp.rs")` contains `set_hook` and the literal
`internal error`.

### 3.4 The five endings

| # | Input | Answer | Then |
|---|---|---|---|
| 1 | no `\r\n\r\n` within `MAX_FRAME_HEADER_BYTES` | `-32700`, `id: null` | stderr `the header block is too long`, exit `1` — the offset of the next frame is unknown |
| 2 | no `Content-Length`, two of them, a non-digit value, or `N > MAX_FRAME_BODY_BYTES` | `-32700`, `id: null` | for `N > MAX_FRAME_BODY_BYTES`: stderr `the body is too long`, exit `1` **without reading or allocating the body**; for the other three the stream is unframeable, exit `1` |
| 3 | body is not UTF-8 | `-32700`, `id: null` | keeps serving (the length was known, so the next frame starts at a known offset) |
| 4 | body is not JSON, or is JSON that is not an object | `-32700` / `-32600` | keeps serving |
| 5 | EOF at any point | nothing | exit `0` iff `shutdown` was received, else `1` |

Exit status is `0` **only** after `shutdown`; every other ending is `1`. A broken
document never ends the session (§5, §6.3): the documented sentence "`1` only if the
client disappears without one" covers rows 1, 2 and 5 — a stream that ends, or that can
no longer be framed, is no longer a peer. A bad **command-line** argument is not a
session at all: it exits `2` (the `check`/`fmt` CLI-error convention, §11 S10).

R-ids: **R9** (the constants, with `MAX_FRAME_BODY_BYTES` asserted as `33_619_968`), **R10**
(header cap before allocation, body read by length, `READ_CHUNK` scratch), **R11**
(`print_stdout`/`unwrap`/`panic`/`indexing_slicing` denied in `lsp.rs` and therefore
enforced by `cargo clippy -- -D warnings`, the gate `MB.22.3` runs), **R12**
(stdout is frames only; stderr is fixed reasons and never document text), **R13**
(the five structural rows, including `-32700` + `id: null` + keep-serving for rows 3–4
and exit `1` for rows 1–2).

<!-- APPEND -->
## 4. Positions: 1-based `char`s in, 0-based UTF-16 out

### 4.1 The one fact that makes this section necessary

`blastit-manifest`'s `Position` is **1-based `line`, 1-based `col` counted in
`char`s** (`error.rs:57`, `docs/design/blastfile-dsl.md` R2). LSP's `Position` is
**0-based `line`, 0-based `character` counted in UTF-16 code units**. Zed offers only
`general.positionEncodings = ["utf-16"]` **and never reads the server's
`positionEncoding`** — it converts with `point_to_lsp` over a `PointUtf16`
(research §7). Therefore:

- a `char` count in `character` is a **bug** for any character outside the BMP
  (`😀` is one `char`, two code units), and
- a **byte** count is a bug for any non-ASCII character at all (`✓` is 1 byte `char`
  count, 3 UTF-8 bytes, 1 code unit).

`character` past the end of a line clamps to that line's length; a `line` past the last
line clamps to `(0,0)`.

### 4.2 The formula, once

`Lines::point()` is called once per diagnostic, and spelling it as `text.split('\n').nth(i)`
would rescan the document per call — on a legal 16 MiB file with 512 diagnostics that
is gigabytes of scanning **per keystroke**. So the lines are found once per publish:

```rust
/// The byte offset of the start of every line, in one O(n) pass.
struct Lines<'a> { text: &'a str, starts: Vec<u32> }

impl<'a> Lines<'a> {
    fn new(text: &'a str) -> Self {
        let mut starts = vec![0u32];
        for (at, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                // `at + 1` is a boundary: the byte after a '\n' starts a line.
                starts.push(at as u32 + 1);
            }
        }
        Self { text, starts }
    }

    /// One 1-based `Position` in the editor's coordinates, by binary search.
    fn point(&self, at: Position) -> (u32, u32) {
        if at.is_none() {
            return (0, 0);                       // Position::NONE → (0,0)
        }
        let line_index = at.line() - 1;
        let Some(&start) = self.starts.get(line_index as usize) else {
            return (0, 0);                       // a line past the end clamps to (0,0)
        };
        // The last line has no following start: it ends at the document's end.
        let end = self
            .starts
            .get(line_index as usize + 1)
            .copied()
            .unwrap_or(self.text.len() as u32)
            .max(start);
        let line = self.text.get(start as usize..end as usize).unwrap_or_default();
        let line = line.strip_suffix('\n').unwrap_or(line);   // the terminator is not the line
        let chars = at.col().saturating_sub(1) as usize;
        let character: usize = line.chars().take(chars).map(char::len_utf16).sum();
        (line_index, character as u32)
    }
}
```

The bound is stated as an assertion: **one pass over the document per publish, and
`Lines::point()` is `O(log lines + col)`** — independent of `MAX_DIAGNOSTICS` (§4.5 P6
asserts the timing).

- **Line split rule:** the document is split on `'\n'` and the terminator is not part
  of the line. A CRLF buffer therefore leaves one `'\r'` at the end of each line, and
  it counts as **one** UTF-16 unit — which is correct, because the lexer's `bump()`
  resets `line`/`col` only on `'\n'`, so a `\r` before it was already counted as one
  column (`lexer.rs:168`). No normalisation happens here.
- `Position::NONE` is the answer for **every** variant with no `at` field, in two
  kinds:
  - variants that describe the file as a whole — `TooLarge`, `TooManyTokens`,
    `TooManyTrivia`, `TooManyRoutes`, `UidBaseTooHigh`, `NotRegularFile`,
    `Unreadable`, `InvalidApp`, and the older `Location`-less route/mount refusals
    (`DuplicateRouteName`, `InvalidRouteName`, `EmptyPath`, `InvalidMethod`,
    `EmptyMethods`, …); and
  - variants whose locator is a semantic `Location` (`view "cart"`, `trait "x"`)
    rather than a line and column: `UnknownGroup`, `GrantOnUnknownProxy`,
    `UnrestrictedGroup`, `UnrestrictedWithQueries`, `UnrestrictedWithContent`,
    `UnusableTable`, `RedactedQueryPlaceholder`, `LiteralQuery`,
    `StaticMountPrefixInvalid`, `StaticFallbackInvalid`, `StaticRootInvalid`,
    `StaticRootUnusable`, `StaticIndexInvalid`, `StaticImmutablePattern`,
    `StaticDeadDeclaration`, `PhpEntryInvalid`, `PhpWorkersInvalid`,
    `PhpDeadDeclaration`.
  Both render `(0,0)-(0,0)`: a zero-width squiggle on the first character, which is
  what "the file as a whole" (and "this named view, somewhere") looks like. Giving a
  `Location` a real span is a follow-up with the same `sites.rs` mechanism (§5.5),
  not a v1 promise — and §5.3 says which sentences therefore land at `(0,0)`.
- `char::len_utf16` is the whole conversion. The formula is
  `sum(utf16_len(c) for c in line[:col-1])`, written as `take(col-1).map(len_utf16)`
  so it cannot be `col - 1` by accident. Each `line` is one `&str` slice of the
  original document, so no copy is made per diagnostic.

### 4.3 The squiggle width

The range is `start = lines.point(error.position())` and `end = start + width`, where the
width is measured **in UTF-16 units on the same line**:

1. If the character at `start` matches `[A-Za-z0-9_-]`, the width is the length of the
   maximal run of `[A-Za-z0-9_-]` from there to the first other character or the end
   of the line (a name, a keyword, a method, an env-var name).
2. Otherwise the width is exactly **one** unit (an `=`, a `,`, a `{`, a quote).
3. `end` never crosses the line's last character: the run stops at `'\n'`.

`Range.end` is exclusive; a `(0,0)-(0,0)` range is legal and is the `Position::NONE`
shape.

### 4.4 Hover's inverse conversion

`textDocument/hover` gives a UTF-16 `character`; finding the word under it needs the
`char` index:

```rust
fn char_index(line: &str, character: u32) -> usize {
    let mut units = 0u32;
    let mut index = 0usize;
    for c in line.chars() {
        if units >= character { break; }
        units += c.len_utf16() as u32;
        index += 1;
    }
    index
}
```

so a hover past the middle of a surrogate pair resolves to the code point that starts at
it. §7.2's word scan then runs on `char`s and the reply's `range` is converted back
with `Lines::point()`.

### 4.5 The assertions

All of them live in `crates/blastit/tests/critical_lsp.rs` (protected) except the pure
functions, whose cases sit in `lsp.rs`'s `mod tests`:

| # | Fixture | Assertion | What it kills |
|---|---|---|---|
| P1 | `"# ✓ ok\nview v \"/x\" {\n  body =\n}\n"` | `Equals` at `Position::at(3,8)` → range `(2,7)-(2,8)`; at top level `body` would be refused at `2:1` as `NotADirectiveHere`, so the `=` must sit in a block | an off-by-one in line or column — **not** the unit: `✓` is on line 1, which `point()` never converts |
| P2 | `"view v \"/😀\" { body = }\n"` | the `=` diagnostic's start is `{"line":0,"character":20}` | the `col - 1` bug (19) and the byte bug (22): the only unit oracle |
| P3 | `"view v \"/é\" { body = }\n"` | start is `(0,19)` and `col-1` is also 19 — asserted to document that BMP characters do **not** discriminate | (documents why P2, not P3, is the oracle) |
| P4 | `"# x\n"` × 4 194 304 followed by `"a"` — exactly `16_777_217` bytes, refused by `parse` from its length before lexing | the `TooLarge` diagnostic range is `(0,0)-(0,0)` | mapping `NONE` to the last line |
| P5 | `"viewx \"x\"\n"` | the `UnknownKeyword` range is `(0,0)-(0,5)` — the word's width | a one-unit squiggle on a word |
| P6 | a 16 MiB fixture with 512 refusals, then one with 1 | each `didChange`→publish round trip completes in < 250 ms (`std::time::Instant`) and the two are within 2× of each other | `Lines::point()` rescanning the document per diagnostic (§4.2) |

R-ids: **R14** (the `Lines::point` formula, one line-offset pass per publish, line
split on `'\n'`, `char::len_utf16`), **R15** (`Position::NONE` → `(0,0)-(0,0)`),
**R16** (the squiggle width rule, both branches), **R17** (hover's UTF-16 → `char`
inverse conversion), proven by P1–P6.

## 5. Diagnostics

### 5.1 The notification

```json
{"jsonrpc":"2.0","method":"textDocument/publishDiagnostics","params":{"uri":"file:///…/Blastfile","version":3,"diagnostics":[…]}}
```

One notification per publish, containing the **whole** set for that `uri`: Zed
replaces a URI's set on every push, so a shrinking file needs no deletion message.
`version` is the version the published text came from (`didOpen`'s, or the
`didChange`'s) — it is how a client drops a stale set — and it is **absent** from the
`didClose` publish.

### 5.2 One diagnostic, exactly

```json
{"range":{"start":{"line":2,"character":5},"end":{"line":2,"character":6}},
 "severity":1,"source":"blastit",
 "message":"`=` is not Blastfile syntax; write the value after the keyword: `body json`"}
```

| Field | Value | Assertion |
|---|---|---|
| `range` | §4.2 + §4.3 | P1, P2, P4, P5 |
| `severity` | `1` for every `ManifestError`; `2` for every `ManifestWarning` | D1 |
| `source` | the constant `"blastit"` | D1 |
| `code` | **absent** — the enum has no stable key, and a `code` invented here would be a second vocabulary | D1 asserts the key is missing, not `null` |
| `message` | `error.message()` for errors; `warning.to_string()` for warnings | D2, D7 |
| `tags`, `relatedInformation`, `data` | never emitted | — |

`ManifestError::message()` is defined once, in `error.rs`:

```rust
pub fn message(&self) -> String {
    let rendered = self.to_string();
    let prefix = prefix(self.position());          // "2:10: " or ""
    match rendered.strip_prefix(prefix.as_str()) {
        Some(body) => body.to_owned(),
        None => rendered,
    }
}
```

so `Display` keeps its one existing spelling (`<line>:<col>: <msg>`, or `<msg>` when
position-less), the CLI keeps printing `<path>:<line>:<col>: <msg>`, and the editor
gets `<msg>`. **Assertion (D2):** for the `body = json` fixture,

```
format!("{}{}", prefix(error.position()), error.message()) == error.to_string()
```

and `error.message()` is exactly
`` `=` is not Blastfile syntax; write the value after the keyword: `body json` `` —
the same string `report_manifest_errors` prints after the path and position
(`cli.rs:554`). This is the whole DX contract of §0: one sentence, two readers.

A `ManifestWarning`'s `Display` already carries no position prefix and **must keep
carrying none**: the boot's log lines are unchanged by this design (**D8** asserts a
warning's `to_string()` contains no `:<digit>` pattern and the two boot event lines
are byte-identical to today's).

### 5.3 Which document state is reported

`lsp.rs` computes, on every `didOpen` and `didChange`, with the entry point of §5.5:

```rust
match blastit_manifest::blastfile::check(&doc.text, UidLayout::DEFAULT_BASE) {
    Err(errors) => errors.errors().iter().map(error_diagnostic).collect(), // severity 1
    Ok(checked) => checked
        .warnings
        .iter()
        .zip(checked.sites)
        .map(|(warning, site)| warning_diagnostic(warning, site)) // severity 2
        .collect(),
}
```

- Errors and warnings are **not** merged: a file with a parse error has no lowered
  model, so there is nothing for `validate` to warn about. A clean file is the only
  file that can carry a warning, and that is exactly when a warning is useful.
- `UidLayout::DEFAULT_BASE` (`20_000`, `uid.rs:32`) is the parser's own default; the
  only two checks that read `base` (`TooManyRoutes`, `UidBaseTooHigh`) then fire in
  the editor for a file that a different deployment base would accept. That is
  deliberate: the editor reports what the default boot refuses, and both are
  position-less errors shown at `(0,0)` per §4.2.
- The three families the CLI can produce are therefore all present, and each squiggles
  where its own shape allows:
  - **lexer/parser** refusals carry `at:` and squiggle at the token;
  - **validate** refusals split in two: the eighteen `Location`-carrying variants land
    at `(0,0)` (`UnknownGroup`, `GrantOnUnknownProxy`, `UnrestrictedGroup`,
    `UnrestrictedWithQueries`, `UnrestrictedWithContent`, `UnusableTable`,
    `RedactedQueryPlaceholder`, `LiteralQuery`, `StaticMountPrefixInvalid`,
    `StaticFallbackInvalid`, `StaticRootInvalid`, `StaticRootUnusable`,
    `StaticIndexInvalid`, `StaticImmutablePattern`, `StaticDeadDeclaration`,
    `PhpEntryInvalid`, `PhpWorkersInvalid`, `PhpDeadDeclaration`), while the
    position-less route/mount refusals do too (`InvalidRegex`, `TooManyRoutes`,
    `UidBaseTooHigh`, …) — §4.2 lists both kinds;
  - **lowering** refusals are mixed (`PhpEntrypointInvalid`, `StaticRootMissing`,
    `InvalidName` carry `at`; `InvalidApp` does not). A
  `Location`-carrying error at `(0,0)` is the known cost of not re-deriving spans in
  this node, and it is the follow-up named in §4.2 — not a promise this design breaks.
  Availability and coverage findings (`missing_grant`, `dead_grant`,
  `unrouted_request`) stay with `blastit check`: they need a suite that ran, and the
  accepted page says so.

### 5.4 The four warnings the node names — and the two it does not

`ManifestWarning` has six variants. The node names four; the other two have a CST site
too, so `sites()` (§5.5) locates all six and every warning gets a real position. No
variant maps to `Position::NONE` in v1 — and the table says why each one has a site.

| Variant | Severity | The sentence (`Display`, unchanged) | Its CST site | Site exists because |
|---|---|---|---|---|
| `UnattachedGroup { group }` — unused `trait` | 2 | `trait "…" is attached to no view and no global traits: it grants nothing` | the name token of `trait <name> {` | the declaration is the thing to delete or `use` |
| `UngrantedProxy { proxy }` — store with no grant | 2 | `store "…" has no grant in any view or trait: it is unusable` | the keyword/name token of `postgres [<name>]` | the declaration is the thing to grant or delete |
| `UnrestrictedRoute { route, proxy }` — unrestricted `postgres` | 2 | `view "…" may run any statement on store "…" (unrestricted)` | the `unrestricted` token in that view's grant | that one word is the decision |
| `RawIngress { route }` — unacknowledged `body raw` | 2 | `view "…": body raw passes the request body through unvalidated` | the `raw` token of the view's `body` directive | that one word is the decision |
| `EgressRoute { route }` — `egress any` | 2 | `view "…": egress any opens any outbound address and port` | the `egress` token | that one word is the decision |
| `EntrypointNotReached { entry }` — a PHP front controller no view forwards to | 2 | ``entrypoint php "…" is reached by no view: declare a `view` that forwards to it`` | the `entrypoint` keyword | the declaration is what the view is missing |

The two rows the node does not name are here because a warning published at line 1
would be a lie about where to look; leaving them out instead would silently drop a
sentence the boot prints, and §0's rule is that the editor and the CLI never
disagree about a warning either (§0's "one canonical answer" names the boot as a
warning's printer, because `check.rs:136` discards warnings).

### 5.5 Where the warning positions live: `sites.rs`, not a field on the variants

**Not** as an `at: Position` field on each variant, and **not** as a span field on
`Manifest`. Two reasons, both checked:

1. The variants are constructed by struct literal in protected tests —
   `tests/critical_manifest_011.rs` (`RawIngress`, `EgressRoute`),
   `tests/critical_static_manifest.rs` (`UngrantedProxy`),
   `tests/critical_php_manifest.rs`, `tests/critical_blastfile_body.rs` — so a field
   would force edits to files this epic must not touch (`tools/critical_guard.py`).
2. `Manifest` lives in `model.rs`, which `MB.22.3` does not own, and its fields are
   private, so a span field cannot be declared from within the node's paths.

So the positions are computed where the CST still exists, and never stored:

```rust
// crates/blastit-manifest/src/blastfile/sites.rs (new; MB.22.3 owns blastfile/**)
/// The 1-based CST site of every warning, in `warnings`' order. A warning whose
/// declaration the walk cannot find gets `Position::NONE`.
pub(crate) fn sites(cst: &Cst, source: &str, warnings: &[ManifestWarning]) -> Vec<Position>;
```

one walk of the tree already in memory, keyed by the name each warning prints:

| Warning | Lookup |
|---|---|
| `UnattachedGroup { group }` | the `trait <name>` whose name equals `group` → the name token |
| `UngrantedProxy { proxy }` | the `postgres [<name>]` whose lowered name equals `proxy` (`db` for the unnamed form, `lower::DEFAULT_STORE`) → the keyword |
| `UnrestrictedRoute { route, proxy }` | inside `view <route>`, that store's grant block → the `unrestricted` token |
| `RawIngress { route }` | inside `view <route>`, a `body` directive whose codec is `raw` → the `raw` token |
| `EgressRoute { route }` | inside `view <route>` → the `egress` token |
| `EntrypointNotReached { entry }` | the `entrypoint` keyword whose lowered path equals `entry` |

and the one entry point that gives the LSP all three at once, in
`crates/blastit-manifest/src/blastfile/mod.rs`:

```rust
pub struct Checked {
    pub manifest: Manifest,
    pub warnings: Vec<ManifestWarning>,
    pub sites: Vec<Position>,          // `sites.len() == warnings.len()`
}

/// Parses, lowers, validates, and locates each warning: `Manifest::parse_blastfile`
/// plus the one walk that `parse_blastfile` drops.
pub fn check(source: &str, base: u32) -> Result<Checked, ManifestErrors> {
    let cst = parser::parse(source)?;
    let manifest = lower::lower(&cst, source)?;
    let warnings = manifest.validate(base)?;
    let sites = sites::sites(&cst, source, &warnings);
    Ok(Checked { manifest, warnings, sites })
}
```

`check` is three lines of the four `parse_blastfile` already runs, plus `sites()`; it
is a second entry point because the LSP is the only caller that keeps the tree. A
field on `Manifest` was the alternative and is rejected for the two reasons above.

**Assertion (D7):** two fixtures cover all six warnings, because no single file can
(`EntrypointNotReached` fires only when the manifest has no routes, `validate.rs:1036`,
while the other three need views):

```text
fixture A — no views, no traits: three warnings
1  entrypoint php "public/index.php"      # EntrypointNotReached → line 1
2
3  postgres $DATABASE_URL                 # UngrantedProxy (db)        → line 3
4  postgres analytics $ANALYTICS_URL      # UngrantedProxy (analytics) → line 4
```

```text
fixture B — three views and an orphan trait: four warnings
 1  postgres $DATABASE_URL
 2
 3  trait session {                        # UnattachedGroup    → line 3
 4      postgres { transactions }          #   grants db, so no UngrantedProxy
 5  }
 6
 7  view open "/a" {
 8      body raw                          # RawIngress         → line 8
 9  }
10
11  view wide "/b" {
12      postgres unrestricted             # UnrestrictedRoute  → line 12
13  }
14
15  view outbound "/c" {
16      egress any                        # EgressRoute        → line 16
17  }
```

The trait in B grants the default store, so B cannot also demonstrate `UngrantedProxy`;
that is why A has no trait: its two stores are granted by nobody, and its `entrypoint php`
is reached by no view because A has none. (`UngrantedProxy` would vanish from a fixture
that also granted the stores, which is the rule `validate` applies: a grant in any view
*or trait* counts.)

The assertion is written **per variant**, from the fixture's own text, so no hand-count
can drift and no fixture has to yield a particular total:
> for the six variants, expressed as seven fragments (one `contains` fragment each:

> `is reached by no view`, `store "db" has no grant`, `store "analytics" has no grant`,
> `is attached to no view`, `passes the request body through unvalidated`,
> `may run any statement`, `egress any opens any outbound address`), the published set
> contains **exactly one** diagnostic matching each fragment, with `severity: 2` and
> `source: "blastit"`, and its `range.start.line` equals the 0-based line **the test
> finds by searching the fixture for the word that site is on** (`egress any` → the line
> containing `egress any`; `is reached by no view` → the line containing
> `entrypoint php`; `store "db" has no grant` → the line containing
> `postgres $DATABASE_URL`).

That shape is what gives it teeth: a stub that publishes everything at `(0,0)` fails
every line assertion, and a `sites()` that returns the same position for two warnings
fails the distinctness check (`assert_ne!` over the seven sites). It also survives a
`validate` change that adds or drops a warning the design did not anticipate: the test
asserts the six fragments it cares about, not a total.

The fallback is equally testable without any `Manifest` constructor: `check("",
UidLayout::DEFAULT_BASE)` returns no warnings, and `checked.sites` for a warning whose
declaration the tree does not contain is `Position::NONE` — `tests/warning_sites.rs`
calls the public `check` and asserts both fixtures; the `Position::NONE` fallback (a warning
whose declaration is absent from the tree) is a `mod tests` case inside `sites.rs`,
where the private function is reachable, and asserts
[Position::NONE] for that one warning.

### 5.6 Replacement and clearing

| Event | Publish |
|---|---|
| `didOpen` | the document's full set, with `version` |
| `didChange` | the new text's full set, with the new `version` (the previous set is replaced, never merged) |
| `didClose` | `{"uri": …, "diagnostics": []}` — **no** `version` |
| a file that is now clean | `{"…","diagnostics":[],"version":n}` (the previous squiggles must disappear) |

**D4:** a session that opens a broken file and then changes it to a clean one sends
two `publishDiagnostics`, the second with `diagnostics: []` and the second version.
**D5:** `didClose` publishes `[]` with no `version` key and drops the document from the
map (a later `hover` for that `uri` answers `null`, not an error).

### 5.7 The cap

`MAX_DIAGNOSTICS = 512`. When the error list is longer, the first 511 are published
and the 512th is

```json
{"range":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}},
 "severity":2,"source":"blastit",
 "message":"<n> more problems; run `blastit check` for the full list"}
```
where `<n> = total - 511`. **D6:** the fixture is 600 views that each grant a store
nobody declares — `view v{n} "/x{n}" { postgres nosuchstore { select x } }`, 600 times —
because `parser::parse` returns on the **first** error (`parser.rs:41`) and `lower`
returns on the first unresolved `use` (`lower.rs:743`), so only `validate` accumulates
(`validate.rs:74`), and `GrantOnUnknownProxy` is what it accumulates per view. It
publishes exactly 512 diagnostics: `[0..511]` are `GrantOnUnknownProxy` at `(0,0)` (a
`Location`-carrying variant, §4.2), `[511].message` is
`` 89 more problems; run `blastit check` for the full list ``, `[511].range` is
`(0,0)-(0,0)`, and the session stays alive (the next `hover` answers).

R-ids: **R18** (notification shape and replacement semantics), **R19** (the
severity/source/code/message contract of §5.2), **R20** (`message()` equals the CLI
sentence without the prefix), **R21** (warnings use `Display` unchanged and boot lines
are byte-identical), **R22** (all six warning sites from §5.4 via `sites()`),
**R23** (`didClose` publishes `[]` with no version), **R24** (`MAX_DIAGNOSTICS` and its
truncation notice).

<!-- APPEND -->
## 6. Formatting: one whole-document edit, or nothing

### 6.1 The three outcomes

`textDocument/formatting` runs `blastit_manifest::blastfile::fmt(&doc.text)`
(`print.rs:50`, already public, already the bytes `blastit fmt` writes) and returns:

| `fmt` result | Reply | Why |
|---|---|---|
| `Ok(text)` where `text == doc.text` | `[]` | a no-op `TextEdit` still marks the buffer dirty and would fight `format_on_save`; `[]` is the protocol's "nothing to do" |
| `Ok(text)` where `text != doc.text` | one `TextEdit` covering the whole document (§6.2) | one edit, one undo step |
| `Err(_)` | `null` | **never** a JSON-RPC error: an error reply makes Zed show "language server error" on every save of a broken file. The diagnostics already carry the reason (§5), and `null` is "the server declines" |

`null` is also the answer for a `uri` that is not open (a formatting request for a
document this server never received is not an error worth a squiggle). **F4:** after a
`null` for a broken buffer the very next `hover` answers — the declining path leaves
the session alive.

### 6.2 The edit, and its exact `end`

```json
[{"range":{"start":{"line":0,"character":0},"end":{"line":4,"character":0}},"newText":"app \"shop\"\n\nview v \"/x\" {\n    body none\n}\n"}]
```

`end` is `buffer.max_point_utf16()` — **not** `line_count - 1`, **not** the last
non-empty line:

```
end.line      = number of '\n' in the buffer
end.character = sum(utf16_len(c) for c in the text after the last '\n')
```

| Buffer | `end` | What a wrong rule would say |
|---|---|---|
| `""` | `(0,0)` | `(0,0)` |
| `"a\nb"` | `(1,1)` | — |
| `"a\n"` | `(1,0)` | `(0,1)` for "last non-empty line"; a `trim_end()` implementation would drop the final newline and the buffer would stay dirty |
| `"app \"shop\"\nview v \"/x\" {\n  body none\n}\n"` (four newlines) | `(4,0)` | `(3,1)` for "last non-empty line" |
| `"a\n😀b"` | `(1,3)` | `(1,2)` for a `char` count, `(1,5)` for a byte count |

Zed compares this `end` against `max_point_utf16` to take its fast diff path; another
`end` still applies through the generic edit path, but the rule is stated so the fast
path is the tested one. **F1** asserts all five rows above as literals (the fifth is
the only one with a non-BMP character after the last newline, so it is the only oracle
for `end.character`'s UTF-16 rule); **F2** asserts `newText` is `fmt`'s output and
`end.line` equals the `'\n'` count of the **input** buffer (the edit replaces the
region the old text occupied, so the count is the old text's).

### 6.3 CRLF: normalise to `\n` on save — the decision, and its proof

**Decision: the LSP does not touch line endings; `blastfile::fmt` already writes LF,
so saving a CRLF `Blastfile` normalises it to LF, and that is the intended outcome.**

Reasons, in order:

1. `blastit fmt` already does exactly this: `print.rs`'s test
   `a_crlf_file_is_written_with_lf` (line 741) asserts
   `formatted("app \"shop\"\r\nview v \"/x\" {\r\n  body json\r\n}\r\n") ==
   "app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n"`. Any other editor behaviour
   would make the editor and `blastit fmt` disagree — the one thing §0 forbids.
2. The lexer accepts `\r\n` as one `Newline` token and refuses a lone `\r`
   (`CarriageReturn`), so a CRLF file is legal input *and* already non-canonical:
   `blastit fmt --check` exits `1` on it. Format-on-save turning it canonical is a fix,
   not a surprise, and it is one undo step.
3. Preserving the buffer's endings would require the formatter to re-insert `\r`
   after every newline it writes — a second canonical layout, chosen per save, that
   no test in `blastfile` covers and that `blastit fmt` would then contradict on the
   next CI run.

The cost is explicit and small: a Windows-checked-out `Blastfile` is rewritten once,
and `git diff` shows the whole file until `.gitattributes` normalises it. The docs
already promise "Formatting is the canonical layout"; this is that promise.

**F5 (the test that proves it):** open
`"app \"shop\"\r\n\r\nview v \"/x\" {\r\n    body json\r\n}\r\n"` (already canonical
except for the endings), request formatting once, and assert (a) exactly one
`TextEdit`, (b) `newText` contains **no** `'\r'`, (c) `newText` equals
`blastfile::fmt(lf_normalised_input)` and equals the bytes `blastit fmt` would write
for that file, (d) `end` is `(5,0)` (the fixture has five `\n`); then request formatting again with `newText` as
the buffer and assert the reply is `[]` (the fixed point is reached in one save). A
CRLF test that only asserted "an edit came back" would pass with a `col-1` bug or a
preserving formatter; (b), (c) and (d) are what give it teeth.

### 6.4 `options` are ignored, and why that is not rudeness

Zed sends all five `FormattingOptions` (`tabSize: 4`, `insertSpaces: true`,
`trimTrailingWhitespace: true`, `insertFinalNewline: true`,
`trimFinalNewlines: true`). They are ignored, because `Blastfile` has **one** layout
(four spaces per level, one directive per line, one blank line between sections,
final newline) and honouring `tabSize` would make the editor produce a file that
`blastit fmt --check` then fails. If a client asks for 2 spaces, the server still
answers the canonical layout: the alternative is two truths. No field of `options` is
deserialised, so a malformed or missing `options` is not an error — only
`textDocument.uri` is required (`-32602` when absent).

### 6.5 `formatter: "auto"` reaches us

Zed's `formatter` setting defaults to `"auto"`, which resolves to the primary language
server when Prettier is not allowed, so declaring `documentFormattingProvider: true`
is sufficient for `editor: format` and for `format_on_save` — the extension does not,
and cannot, declare a formatter. The one line the developer merges into
`.zed/settings.json` (because `format_on_save` defaults to `off` and an extension
cannot set a per-language default) is the accepted page's:

```json
{ "languages": { "Blastfile": { "format_on_save": "on" } } }
```

and `MB.22.4` commits the same object — nothing else — as `.zed/settings.json` in this
repository, so a contributor gets it for free. **F6:** `scripts/check_zed_extension.py`
asserts three things: the committed `.zed/settings.json` is valid JSON whose only
top-level key is `languages` and whose `languages.Blastfile.format_on_save` is `"on"`;
the JSON fence under `## Editors` in `docs/local.md` parses to the same object, so the
page and the repository cannot drift; and its own negative self-test changes `"on"` to
`"off"` in a temp copy and requires a red run. `MB.22.5` owns the guard and every
negative case; `python/tests/test_docs_assets.py` is not touched (§12.4).

R-ids: **R25** (three outcomes, `null` never an error), **R26** (`end` =
`max_point_utf16`, the **five** literal rows), **R27** (CRLF normalisation, with F5's
four assertions), **R28** (`options` ignored, `uri` required), **R29** (the documented
line and the committed `.zed/settings.json`, both asserted by the guard).

## 7. Hover: the directive's own sentence, where the developer is typing

### 7.1 The word

1. Take the requested line (`text.split('\n').nth(line)`), clamp `character` per §4.4.
2. Expand left while the preceding character is `[A-Za-z0-9_-]`, then include one
   leading `$` when the character before that run is `$`; expand right while the
   following character is `[A-Za-z0-9_-]`. So `$DATABASE_URL` is one word, `my_view`
   is one word, and `x$y` yields `$y`, which is neither in `HOVER_TABLE` nor a valid
   `[A-Z_][A-Z0-9_]*` name, so the reply is `null`.
3. Remember whether the word started with `$`, and drop it for the table lookup.
4. Nothing under the cursor (whitespace, `{`, `=`) → `null`.

### 7.2 The lookup order

| Word | Reply |
|---|---|
| in `HOVER_TABLE` | its markdown value |
| starts with `$` and matches `[A-Z_][A-Z0-9_]*` | the one `$IDENT` paragraph, with the name interpolated (§7.3) |
| anything else | `null` |

The table is a `const HOVER_TABLE: &[(&str, &str)]` in `lsp.rs`, **≤ 48 rows**, each
value ≤ 600 bytes, each key matching `[A-Za-z0-9_]+` or `[A-Z]+` (methods), sorted so
the sortedness is a test; a duplicate key is a test failure. Every value is copied from
`docs/reference/blastfile.md` and says nothing that page does not say — the page is the
source, the table is a transcription, and `lsp.rs`'s own `mod tests` asserts that every
key of the table appears in that page's text via
`include_str!("../../../docs/reference/blastfile.md")` (three `..`, because the file
lives at `crates/blastit/src/lsp.rs`). Wording lives in that unit test, not in
`critical_lsp.rs`: a later one-word fix to the reference page must not require a
`critical_guard` authorization for a protected file.

### 7.3 The table

| Key(s) | Markdown value (one paragraph; backticks as written) |
|---|---|
| `unrestricted` | `` `postgres unrestricted` allows every statement on that store, so `check` can no longer tell you what the view needed. It is not a fix for a missing grant, and it warns at boot. `` |
| `raw` | `` `body raw` passes the request body through unvalidated — the loud opt-out. `blastit dev` serves it with a warning, `check` reports `raw_ingress`, and a production boot refuses the file until you add `unvalidated`. `` |
| `unvalidated` | `` `body raw unvalidated` is the acknowledgement: production serves it, with a boot warning. The two legitimate uses are a file upload and a raw-signature webhook. `` |
| `egress`, `any` | `` `egress any` opens any IPv4/IPv6 address and port, with no allowlist, and warns at boot. `` |
| `transactions` | `` `transactions` allows `BEGIN`, `COMMIT`, `ROLLBACK` and savepoints on that store. `` |
| `functions` | `` `functions st_distance, unaccent` allows function calls beyond the built-in safe set; `unaccent` reads a rule file, so it must be named. `` |
| `select`, `insert`, `update`, `delete` | `` `select products(id, name)` allows reading those columns; a bare `products` reads any column. `insert`/`update`/`delete` grant those columns or that table the same way, and `update` also allows row locks. `` |
| `query` | `` `query 'SELECT …'` allows one exact statement; repeat the directive for more. It is trusted as written: read it before pasting. `` |
| `body`, `json`, `form`, `none` | `` The ingress codec: `json`, `form`, `none` (the default) or `raw`. A body sent to a view that declares no `body` is a `413`; `json` and `form` are re-encoded canonically, and anything else is a `400`. `` |
| `prefork` | `` `prefork` forks the worker at startup instead of on the first request. `` |
| `app` | `` `app "shop"` names the application. One per file, optional; it is a label, never a path or a callable. `` |
| `entrypoint` | `` `entrypoint python "shop.wsgi:application"` names the callable behind every view; `entrypoint php "public/index.php"` names the front controller. One per file. `` |
| `preload` | `` `preload "shop.urls", "shop.signals"` imports each module once before the worker forks, so every request shares one copy. The block holds `preload` only. `` |
| `postgres` | `` `postgres $DATABASE_URL` declares the default store; `postgres analytics $ANALYTICS_URL` a second one. The file holds the variable name, never the value. `` |
| `host`, `port`, `user`, `password`, `database` | `` A store slot: `host $DB_HOST`. Each input variable keeps its name and gets the local value; `password` and `ca_cert` have no local value and are removed. `` |
| `sslmode`, `ca_cert` | `` A store slot for the upstream TLS leg: `sslmode $DB_SSLMODE` names an `$IDENT` holding `disable`, `require`, `verify-ca` or `verify-full` (the app gets `disable`); `ca_cert $DB_CA_CERT` names the PEM. `` |
| `trait` | `` `trait session { postgres { … } }` holds store grants only. A view attaches it with `use session`; a trait used nowhere warns at boot. `` |
| `use` | `` `use session, catalog` attaches those traits. The grant is the union of the view's own blocks and every attached trait. `` |
| `view` | `` `view products GET "/products", "/products/{id:int}"` is one sandbox: one worker, one set of permissions. Views are tried in the order written, and the first match wins. `` |
| `static` | `` `static "/assets" { root "public/assets" }` serves regular files under a prefix before any view answers; the longest matching prefix wins. `` |
| `root`, `index`, `fallback` | `` `static` directives: `root` is the directory served (required, relative, no `..`), `index` answers a directory request (default `index.html`), `fallback` answers a path that is no file, on `GET` or `HEAD`. `` |
| `immutable` | `` `immutable "/assets/*"` adds `Cache-Control: public, max-age=31536000, immutable`; `*` stops at `/`, so a nested tree needs one glob per depth. `` |
| `regex` | `` `regex '/v[0-9]+/items'` matches the whole path in linear time, with no lookaround or backreferences. `` |
| `GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS` | `` A method token. `HEAD` is answered wherever `GET` is; a view that declares no method answers every method, including ones you did not intend. `CONNECT` and `TRACE` cannot reach a view. `` |
| `$IDENT` (computed, not a row) | `` `$NAME` names an environment variable: the value stays in the environment and never in the `Blastfile`, so the file is safe to commit. A quoted string where `$IDENT` is required is a syntax error. `` |

Not in the table, on purpose: `select`/`insert`/`update`/`delete`/`query` inside a
`query '…'` string are SQL, and the raw-text hover shows the grant sentence. That is a
**known deviation** with a named successor: `blastfile::symbols()` (§7.5) gives the CST
context that would let hover distinguish the two, and hover inside strings and comments
is then decided by node, not by word.

### 7.4 The reply, and definition deferred

```json
{"contents":{"kind":"markdown","value":"`prefork` forks the worker at startup …"},
 "range":{"start":{"line":4,"character":4},"end":{"line":4,"character":11}}}
```

`range` is the word's own span (§7.1) converted back with `Lines::point()`; a `null` reply is
the whole result (`{"jsonrpc":"2.0","id":n,"result":null}`), not an empty string — Zed
drops empty text but a `null` is unambiguous.

**Definition is deferred, with the mechanism named.** `textDocument/definition` is
answered `null` in v1 and **not advertised** (`definitionProvider` absent), because
`use <trait>` → `trait <name>` and `postgres <store>` → `postgres <name> $URL` need the
*declaration* span, and today the model exposes only `Manifest::route_lines()`
(`model.rs:916`) — the header lines of views. Re-scanning lines inside `lsp.rs` would
duplicate grammar knowledge and go wrong the first time a name is reused as a column.
The follow-up is mechanical and bounded:

```rust
// crates/blastit-manifest/src/blastfile/symbols.rs (new, follow-up node)
pub struct Symbol { pub kind: SymbolKind, pub name: String, pub position: Position }
pub enum SymbolKind { Trait, Store, View }          // non_exhaustive
pub fn symbols(source: &str) -> Result<Vec<Symbol>, ManifestErrors>;  // one CST walk, ≤ MAX_TOKENS
```

and the reply shape the follow-up emits is the minimum Zed accepts:
`{"uri": <the didOpen uri, echoed>, "range": {"start": {"line": s.position.line()-1,
"character": <§4.2 at col 1>}, "end": <same>}}` — a line-only selection range, which
Zed accepts. `symbols()` is ≤ 40 lines because `sites::sites` (§5.5) already walks the
same tree; it is a separate node so v1 ships without it. **Declining is not erroring**:
`-32601` is for an *unknown* method, and a client probing an unadvertised capability
must get a `null` result, which is what §2.2's row says.

### 7.5 The assertions

The **contract** assertions live in `crates/blastit/tests/critical_lsp.rs`; the
**wording** assertions live in `lsp.rs`'s `mod tests`, because the table is there and a
`later one-word fix to `docs/reference/blastfile.md` must not turn into a
`critical_guard` authorization.

| # | Fixture | Assertion | Tier |
|---|---|---|---|
| H1 | `view v "/x" { postgres unrestricted }` hover on `unrestricted` | `result` is non-null, `range` covers `unrestricted` (12 units: u-n-r-e-s-t-r-i-c-t-e-d), and the value is `HOVER_TABLE["unrestricted"]` verbatim | critical |
| H2 | hover on `{`, `=`, whitespace | `result` is `null` | critical |
| H3 | `postgres analytics $ANALYTICS_URL` hover at the `$` | the value contains `$ANALYTICS_URL` and `stays in the environment`, and never the value of a `DATABASE_URL` set in the environment (§11 m8) | critical |
| H4 | `view my_view "/x"` hover on `my`, then on the `view` keyword | on `my` the word is `my_view` (7 units) and the reply is `null` (a name is not a directive, §7.2); on `view` the value is `HOVER_TABLE["view"]` verbatim (4 units) | critical |
| H5 | `"😀 view"` hover at UTF-16 `character` **6** (the `w`), and `"view v \"/😀\" { body = }"` at **20** (the `=`) | at 6 the value is `HOVER_TABLE["view"]`; at 20 it is `null`. A `char_index` replaced by `character as usize` gives `null` at 6 and `null` at 20 — so **6** is the discriminating case | critical |
| H6 | `HOVER_TABLE` | ≤ 48 rows, sorted, no duplicate key, every value ≤ 600 bytes, every key present in `include_str!("../../../docs/reference/blastfile.md")`, and `unrestricted`'s value contains `every statement` and `warns at boot` | unit (`lsp.rs`) |

R-ids: **R30** (word rule and the `$` handling), **R31** (`HOVER_TABLE` bounds, the
transcription rule and H6), **R32** (`null` for an unknown word and for punctuation),
**R33** (definition deferred with `symbols()` + the exact `Location` shape),
**R34** (`hoverProvider: true` only; no `definitionProvider`).

<!-- APPEND -->
## 8. Extension anatomy: `editors/zed/`

### 8.1 The committed tree

```
editors/zed/
  extension.toml                 # §8.2
  Cargo.toml                     # name "blastit-zed", edition 2024, publish = false,
                                 # crate-type ["cdylib"], zed_extension_api "0.7.0", empty [workspace]
  src/lib.rs                     # §8.3, ~45 lines
  LICENSE -> ../../LICENSE       # a symlink INSIDE the extension directory
  .gitignore                     # target/, extension.wasm, grammars/
  languages/blastfile/
    config.toml                  # §8.4
    highlights.scm               # §10.2
    brackets.scm                 # §10.3
    indents.scm                  # §10.3
    outline.scm                  # §10.3
```

`languages/` is **discovered** by walking `languages/*/` for a `config.toml`; the
directory name is arbitrary and the language is `config.toml`'s `name`. Every query
file is optional and loaded with `.ok()`, so a missing `highlights.scm` costs colours
and nothing else. Zed writes `extension.wasm`, `target/`, `grammars/<name>/` and
`grammars/<name>.wasm` **into this directory** at install time; all four are
gitignored, and `scripts/check_zed_extension.py` fails on a committed one.
`tools/guard.py` is unchanged and is **not** the checker here: it bans wheels,
`.so`, `.pyc` and files over 5 MiB, so none of these paths is caught by it.

`LICENSE` must be inside the extension directory — the repository-root license "will
not work" when `path =` is used — and a symlink is accepted; `readlink -f
editors/zed/LICENSE` is `$PWD/LICENSE`.

### 8.2 `extension.toml`, complete

```toml
id = "blastfile"
name = "Blastfile"
version = "0.1.0"
schema_version = 1
authors = ["Offworld Nexus"]
description = "Blastfile: Blastit's routing table and permission model"
repository = "https://github.com/OffworldNexus/blastit"

[grammars.blastfile]
repository = "https://github.com/OffworldNexus/blastit"
rev = "<40 lowercase hex>"                          # §12.3
path = "editors/tree-sitter-blastfile"

[language_servers.blastit-lsp]
languages = ["Blastfile"]

[language_servers.blastit-lsp.language_ids]
Blastfile = "blastfile"
```

| Rule | Why | Assertion |
|---|---|---|
| `id`, `name`, `version`, `schema_version` present; `schema_version = 1` | no serde default: a missing one is a hard parse error | the guard reads the file with `tomllib` and asserts each key |
| `description` longer **in chars** than `name` | the registry validator rejects the reverse | `len(description) > len(name)` |
| `[grammars.blastfile]` key equals `grammar.js`'s `name` equals the exported `tree_sitter_blastfile` | Zed compiles `-Wl,--export=tree_sitter_blastfile` from that key | the guard asserts the three spellings and greps `tree_sitter_blastfile` out of `parser.c` |
| `rev` is 40 lowercase hex, never a branch, tag or `HEAD` | Zed shallow-fetches it; a branch name is not fetchable by `--depth 1` reliably and is not reproducible | §12.3's five commands |
| `path = "editors/tree-sitter-blastfile"` | the grammar lives in this repository | the guard asserts the literal |
| **no** `name = "…"` under `[language_servers.blastit-lsp]` | `LanguageServerManifestEntry` has no such field; the docs' key is silently ignored, so the table key is the server id | `"name" not in server_table` |
| `languages = ["Blastfile"]` equals `config.toml`'s `name`, case-sensitively | the server is attached to the language by that string | the guard asserts both literals |
| `language_ids` maps `Blastfile → blastfile` | without it Zed lowercases the language name for `didOpen.languageId`, and the id becomes a guess | the guard asserts the mapping |
| `version` is strict semver and never decreases | the registry refuses a decrease | the guard parses it with a three-integer regex |

`zed_extension_api = "0.7.0"` pins the published API to Zed `0.205.x`: the docs say
"Zed ≥ 0.205", and an older build refuses the extension rather than mis-running it.

### 8.3 `src/lib.rs`: resolve `blastit`, then `uv`, through the worktree

**The problem this section exists for.** Zed is normally launched from the desktop, not
from a terminal, so it does not inherit the `PATH` a login shell builds. A bare
`Command { command: "blastit" }` therefore fails on exactly the machine the docs
describe — a `uv add blastit` install whose `.venv/bin` (or `~/.local/bin`) is only on
the interactive `PATH`. `Worktree::which` searches the environment Zed captured from
the shell, and `Worktree::shell_env` is that environment; the extension uses both and
**nothing else**.

```rust
use zed_extension_api as zed;

struct Blastit;

impl zed::Extension for Blastit {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> zed::Result<zed::Command> {
        // The shell's environment, as Zed captured it: a GUI launch has no terminal PATH.
        let env = worktree.shell_env();
        let (command, args) = match worktree.which("blastit") {
            Some(blastit) => (blastit, vec!["lsp".to_string()]),
            None => match worktree.which("uv") {
                Some(uv) => (
                    uv,
                    ["run", "--frozen", "--no-sync", "--quiet", "blastit", "lsp"]
                        .iter()
                        .map(|arg| arg.to_string())
                        .collect(),
                ),
                None => {
                    return Err("blastit is not installed: run `uv add blastit` \
                                (or `uv sync` once in this project), or \
                                `uv tool install blastit`"
                        .to_string())
                }
            },
        };
        Ok(zed::Command { command, args, env })
    }
}

zed::register_extension!(Blastit);
```

| Rule | Why | Assertion |
|---|---|---|
| `worktree.which("blastit")` first | the installed CLI is the one artifact that must win | `scripts/check_zed_extension.py` asserts `which("blastit")` appears before `which("uv")` in `src/lib.rs` |
| `worktree.which("uv")` second, argv `run --frozen --no-sync --quiet blastit lsp` | the accepted page's fallback for a `uv add blastit` project. The page writes the human form, `uv run blastit lsp`; an **editor must not spawn that form**, because a bare `uv run` syncs the project first and runs the worktree's declared build backend with `shell_env()` in `environ` — cloning an untrusted repository and opening its `Blastfile` would execute that repository's build code with every token the developer exports. `--frozen --no-sync` reuses the environment `uv add blastit` already created and builds nothing. A human typing `uv run blastit lsp` in their own project has already accepted that build; an editor spawning it on every `didOpen` has not | the guard asserts the **six** argv literals in order, that `--no-sync` is present, and that the error string contains `uv sync` |
| `env: worktree.shell_env()`, assigned verbatim | the child must see the same `PATH` (and `UV_*`, `VIRTUAL_ENV`, `PYTHONPATH`) the resolution used; a GUI-launched Zed has none of it otherwise. No conversion, no filtering, no `BLASTIT_*` added | the guard asserts `shell_env()` appears exactly once and `Command` carries it |
| **no** `process::Command`, no `[capabilities]` entry, no `blastit --version` probe | executing from the wasm is gated by `process:exec` **and** a user-granted capability (`capability_granter.rs::grant_exec`), which would turn a zero-config install into a permission prompt. Zed spawns the returned `Command` itself, with cwd = the worktree root, stdin/stdout/stderr piped and `kill_on_drop` | the guard asserts the file contains no `std::process`, no `Command::new`, and no `[capabilities]` block anywhere under `editors/zed/` |
| the error string names the install and the sync | it is what an uninstalled user sees exactly once, and `--no-sync` means an unsynced project needs `uv sync` | a negative self-test greps both commands |
| nothing reads the manifest, `BLASTIT_*`, or a settings file | the server is spawned with no arguments and reads stdin only (§1.2) | the §1.2 grep assertion |

`Command::env` and `Worktree::shell_env()` are the same pair type in
`zed_extension_api` 0.7.0 (research §3), so `env` is assigned with no conversion. If a
pinned build ever exposes a struct instead, the one fix is
`worktree.shell_env().into_iter().map(|(name, value)| zed::EnvVar { name, value }).collect()`
inside the same field expression; the guard's `shell_env()` count (exactly one) still
holds.

### 8.4 `languages/blastfile/config.toml`, complete

```toml
name = "Blastfile"
grammar = "blastfile"
path_suffixes = ["Blastfile", "blastfile"]
line_comments = ["# "]
tab_size = 4
hard_tabs = false
autoclose_before = ",;]}"
brackets = [
  { start = "{", end = "}", close = true, surround = true, newline = true },
  { start = "(", end = ")", close = true, surround = true, newline = false },
  { start = "\"", end = "\"", close = true, newline = false, not_in = ["string"] },
  { start = "'", end = "'", close = true, newline = false, not_in = ["string"] },
]
```

`path_suffixes = ["Blastfile", "blastfile"]` is what makes the **extensionless**
`./Blastfile` a Blastfile: Zed tries the last dot-segment *and the whole file name*,
and a suffix matches when the candidate ends with `.<suffix>` **or equals** it
(`available_languages.rs::find_for_file`, research §6). So `Blastfile` matches by
equality and `prod.Blastfile` by suffix; `prod-Blastfile` does not, matching is
case-sensitive, and `blastfile` covers a lowercase copy. No globs, no
`first_line_pattern` (it only runs in the `Undetermined` branch over the first 256
bytes, and the file name is not undetermined). **Assertions:** both strings appear
literally; `grammar == "blastfile"`; `line_comments == ["# "]`; `tab_size == 4`.

`brackets` gives auto-close and newline-on-Enter; `brackets.scm` (§10.3) adds rainbow
brackets only. `autoclose_before` keeps `}` from auto-closing inside `,;]}`.

### 8.5 What the developer needs installed

| To **use** the extension | To **build** it (maintainers only) |
|---|---|
| Zed ≥ 0.205, and `blastit` on the shell `PATH` or `uv` in a project that has `blastit` | `cargo` and `clang` (Zed's builder installs the `wasm32-wasip2` target and wasi-sdk itself), and the pinned `tree-sitter-cli` to regenerate the grammar (§9.4) |

The user's machine needs no `npm`, no Tree-sitter CLI, no `cargo` and no `wasi-sdk` —
the accepted page's sentence — because `src/parser.c` is committed (§9.4).

R-ids: **R35** (the tree of §8.1, incl. the `LICENSE` symlink and the four ignored
paths), **R36** (the `extension.toml` table's nine rules), **R37** (`lib.rs`: `which`
order, the six argv literals, one `shell_env()` assignment, no process execution, no
capability), **R38** (`config.toml`'s **eight** keys, `path_suffixes` exact), **R39** (the
user/build split of §8.5).

<!-- APPEND -->
## 9. The grammar: `editors/tree-sitter-blastfile/`

### 9.1 Layout, and what is committed

```
editors/tree-sitter-blastfile/
  grammar.js                 # §9.2 — the only source of truth
  package.json               # records the pinned generator version; no package tree, no lockfile
  src/parser.c               # COMMITTED, generated
  src/tree_sitter/parser.h   # COMMITTED, generated (parser.c includes it)
  src/grammar.json           # COMMITTED, generated
  src/node-types.json        # COMMITTED, generated (the queries are validated against it)
  test/corpus/*.txt          # `tree-sitter test`
  test/fixtures/*.Blastfile  # the docs' complete example + one file per section
```

Zed never runs `tree-sitter generate`: it clones `[grammars.blastfile].repository` at
`rev`, checks out `path`, and compiles `src/parser.c` (plus `scanner.c` **only** if it
exists — `scanner.cc` is never compiled) with

```
clang -fPIC -shared -Os -Wl,--export=tree_sitter_blastfile \
      -Wl,--max-memory=4294967296 -I <src> <src>/parser.c -o grammars/blastfile.wasm
```

so the four generated files are the product, and `grammar.js` is a build input nobody
at Zed reads.

### 9.2 `grammar.js`, complete enough to transcribe

The grammar is a **colourist, not a validator**: the parser in `blastit-manifest`
decides what a `Blastfile` is, and the editor's red comes from the LSP. Two rules
therefore hold, and §9.5 tests them:

- **No false red.** Every file `Manifest::from_blastfile_str` accepts must parse with no
  `ERROR`/`MISSING` node. A grammar rule that is *stricter* than the language is a
  bug, because Zed paints a red tree node and the developer trusts it.
- **Deliberate looseness is listed.** Where the grammar accepts something the parser
  refuses, §9.3 says so line by line; every entry is a colouring artefact with no
  red, never a missed colour.

```js
module.exports = grammar({
  name: 'blastfile',

  // Not /\s/: a newline ends a statement. A lone \r is legal *between* tokens
  // because the parser accepts \r\n and the \r must not become its own token.
  extras: $ => [/[ \t\r]/, $.comment],
  word: $ => $.name,

  rules: {
    source_file: $ => repeat(choice($._terminator, $._section)),
    _terminator: $ => choice('\n', ';'),

    _section: $ => choice(
      $.app_decl,
      $.entrypoint_decl,
      $.store_decl,
      $.trait_decl,
      $.view_decl,
      $.static_decl,
    ),

    app_decl: $ => seq('app', $.string),
    entrypoint_decl: $ => seq('entrypoint', $.language, $.string, optional($.block)),
    language: $ => choice('python', 'php'),
    store_decl: $ => seq('postgres', optional($.name), choice($.env_var, $.block)),
    env_var: $ => /\$[A-Z_][A-Z0-9_]*/,
    trait_decl: $ => seq('trait', $.name, $.block),
    static_decl: $ => seq('static', $.string, $.block),
    // METHODS then PATTERNS, each comma-separated, the first pattern taking no
    // comma: `view products GET "/products", "/products/{id:int}" {`. `view
    // cart {` has neither, so both groups are optional at the call site (a
    // separate `_view_header` rule would match the empty string, which
    // tree-sitter refuses).
    view_decl: $ => seq('view', $.name, optional($._methods), optional($._patterns), $.block),
    _methods: $ => seq($.method, repeat(seq(',', optional($._terminator), $.method))),
    _patterns: $ => seq($._pattern, repeat(seq(',', optional($._terminator), $._pattern))),
    _pattern: $ => choice($.string, $.regex_raw_string),
    method: $ => /[A-Z]+/,

    block: $ => seq('{', repeat(choice($._terminator, $._statement)), '}'),
    _statement: $ => choice(
      $.directive,
      $.endpoint,
      $.grant,
      $.use_directive,
      $.table_grant,
      $.mode,
    ),

    // Values are optional: `prefork` has none, `egress any` has one.
    directive: $ => seq($.keyword, optional($._values)),
    // A comma separates list items, but `body raw unvalidated` writes two values
    // with no comma at all, so the separator is optional -- the lexer still ends
    // the statement at a newline because a keyword only starts a new directive.
    _values: $ => seq(
      $._value,
      repeat(seq(optional(seq(',', optional($._terminator))), $._value)),
    ),
    _value: $ => choice($.codec, $.env_var, $.string, $.raw_string, $.name, $.dotted_name),
    // A function name is `Word ('.' Word)*` (docs/design/blastfile-dsl.md §6.1),
    // and `blastit fmt` writes the qualified form: `functions pg_catalog.unaccent`
    // is one function in three tokens. `repeat1`, not `repeat`: a bare name is
    // `$.name` in the choice above, and a `repeat` here would make this rule
    // match the same input and collide with it.
    dotted_name: $ => seq($.name, repeat1(seq('.', $.name))),
    use_directive: $ => seq('use', $.name, repeat(seq(',', optional($._terminator), $.name))),
    // `postgres [name] { ... }` or `postgres [name] unrestricted`.
    grant: $ => seq('postgres', optional($.name), choice($.block, alias('unrestricted', $.keyword))),
    // The two `prec(1, ...)` rules resolve the grammar's only two overlaps: a
    // bare `transactions` is a mode rather than a valueless directive, and
    // `select products` is a table grant rather than a directive with a value.
    mode: $ => prec(1, alias(choice('transactions', 'functions', 'unrestricted'), $.keyword)),
    table_grant: $ => prec(1, seq(
      alias(choice('select', 'insert', 'update', 'delete'), $.keyword),
      $._table,
      repeat(seq(',', optional($._terminator), $._table)),
    )),
    // `prec(1)` is what makes `select products` a table grant rather than a
    // directive whose value is the name `products`. Only the four grant verbs
    // can reach a table grant, so `egress any` stays a directive.
    _table: $ => prec(1, seq(
      field('table', alias($.name, $.table)),
      optional(seq(
        '(',
        field('column', alias($.name, $.column)),
        repeat(seq(',', optional($._terminator), alias($.name, $.column))),
        ')',
      )),
    )),
    endpoint: $ => seq(
      $.method,
      repeat(seq(',', optional($._terminator), $.method)),
      $._pattern,
      optional($.block),
    ),

    keyword: $ => choice(
      'transactions', 'functions', 'unrestricted', 'select', 'insert', 'update', 'delete',
      'query', 'body', 'preload', 'root', 'index', 'immutable', 'fallback', 'egress', 'any',
      'prefork', 'host', 'port', 'user', 'password', 'database', 'sslmode', 'ca_cert',
    ),
    codec: $ => choice('json', 'form', 'none', 'raw', 'unvalidated'),

    // Two string nodes: "..." with escapes, '...' verbatim and possibly multi-line.
    string: $ => seq('"', repeat(choice($.escape, /[^"\\]+/)), '"'),
    escape: $ => token.immediate(/\\(["\\nt])/),
    raw_string: $ => /'[^']*'/,

    // `regex '...'` is its own node so @string.regex cannot also fire
    // @string.special on the inner literal.
    regex_raw_string: $ => seq('regex', alias($.raw_string, $.regex_body)),

    name: $ => /[a-z0-9][a-z0-9_-]*/,
    comment: $ => /#[^\n]*/,
  },
});
```

The block above is **verbatim `editors/tree-sitter-blastfile/grammar.js`** as the
`MB.22.4` artifact generates it (tree-sitter CLI 0.25.10, `tree-sitter test` 13/13 green),
not a sketch: four of its forms were forced by the generator or by `blastit fmt`, and each
is pinned here so the document and the code cannot drift.

1. **`view_decl` takes `optional($._methods), optional($._patterns)` inline.** A separate
   `_view_header` rule of two optionals matches the empty string, and the generator refuses
   a rule that can match empty; `view cart {` needs neither group. The first pattern takes
   **no** comma after the last method — the reference's own
   `view products GET "/products", "/products/{id:int}" {`.
2. **`table_grant`'s keyword set is the four grant verbs**, aliased to `keyword`, with
   `prec(1, …)` on `table_grant` and on `_table`. That is what makes `select products` a
   table grant while `egress any` stays a directive; without the precedence the generator
   reports an unresolved `_value`/`_table` conflict.
3. **`dotted_name: seq($.name, repeat1(seq('.', $.name)))`, in `_value`.** A function name
   is `Word ('.' Word)*` (`docs/design/blastfile-dsl.md` §6.1) and `blastit fmt` writes
   the qualified form, so `functions pg_catalog.unaccent` must be one node.
   **`repeat1`, not `repeat`**: a bare name is already `$.name` in the same choice, and
   `repeat` makes `dotted_name` match it too — an unresolved `_value`/`dotted_name`
   conflict the generator reports.
4. **`_values` separates items with `optional(seq(',', optional($._terminator)))`.**
   `body raw unvalidated` is two values with **no comma**, and that is the canonical
   output: `printf 'view v "/x" {\n    body raw unvalidated\n}\n' | blastit fmt --check`
   exits `0`. A comma is therefore a separator *when written*, not a requirement, and a
   newline still ends the statement because a keyword starts a new directive.

Three more decisions are the design's, not the generator's:

5. **`escape` is a child of `string`**, not a regex over the whole literal, so
   `(string (escape) @string.escape)` matches; `regex_raw_string` aliases its inner
   `raw_string` to `regex_body`, so `(raw_string) @string.special` cannot also fire inside
   a regex and the queries need no ordering rule.
6. **`mode` is exactly `transactions | functions | unrestricted`** (aliased to `keyword`):
   otherwise `prefork` and `egress` would parse as modes and be painted `@constant`
   instead of `@keyword`.
7. **A newline after a trailing comma** (`optional($._terminator)` after every `,`) is
   allowed in every list, or `immutable "/assets/*", "/fonts/*"` wrapped across two lines
   would be a red node in a file `blastit fmt` itself writes.

No rule declares a `field('name', …)` and no query uses a `name:` field: every query
addresses a name positionally, `(view_decl (name) @function)`, so a query cannot fail on a
field the grammar lacks. `placeholder` and `pattern` from the research's sketch are
**dropped**: a pattern is a `string` token in the real lexer, and tree-sitter cannot
re-lex inside a token, so `"{id:int}"` cannot be a node without a second pattern grammar
that would diverge from the parser's; a quoted pattern is `(string) @string` — the whole
pattern, placeholders included. `bind` is dropped too: `$?` occurs only inside a `query`
string.

### 9.3 The deliberate looseness, line by line

| Grammar accepts | The parser refuses | Effect |
|---|---|---|
| `functions a b` (two bare names, no comma) | `Unexpected`: the parser needs the comma | coloured as two directive values; accepted, because requiring the comma would make `body raw unvalidated` a red node (§9.2 decision 4) |
| any directive keyword followed by values of any `_value` shape (`body body body`) | `NotADirectiveHere`, `BodyNeedsCodec`, … | coloured, no red — the LSP's squiggle is the red |
| statements with no terminator between them (`{ body none … }` is fine; `{ a b }` matches too) | `Unexpected` | none; the block's `repeat` needs no terminator before `}`, which is exactly the language's "`}` closes the statement" |
| `postgres unrestricted` as a store *name* | `StoreNameReserved` | coloured, no red |
| a `query` that takes `"…"` as well as `'…'` | nothing — both are legal (a statement holding `'` uses `"…"`) | `(string) @string` applies |
| an unterminated string or block at EOF | `UnterminatedString` / `UnclosedBlock` | an `ERROR` node appears — but the parser refuses those files too, so this is not a false red |
| `$?` inside a `query '…'` | nothing — it is legal there | stays inside `raw_string`, so there is no `bind` node and no `@variable.parameter` capture |

One **strictness** row remains, and it has an owner:

| Grammar rejects | The parser accepts | Effect and owner |
|---|---|---|
| `method: /[A-Z]+/` — `M-SEARCH` or `GET2` | `is_method_word` takes any first byte `A`–`Z` and then word bytes | a red node on a legal file; it is the grammar's only known false red. §12.8 hands `MB.22.4` the one-line widening (`[A-Z][A-Z0-9_-]*`) plus an `M-SEARCH` corpus case, then a regeneration |

Nothing else in the file is stricter than the language: no rule requires a method before a
pattern, none requires a terminator after `}`, and none checks a name against a keyword
list. The `table_grant` row that an earlier revision carried — `egress any` colouring as a
function — is **gone**, because `table_grant` is restricted to the four grant verbs and
`egress any` now parses as a directive.

### 9.4 The pinned generator and the regeneration gate

`package.json` records the generator's **exact** version (the `0.25.x` the `MB.22.4`
author resolves, written into `RESULT.md`, `package.json` and the extension README). The
gate uses the **pinned release binary** of that version — no `npm`, no `npx`, no
`node_modules`, no lockfile:

```
$ cd editors/tree-sitter-blastfile
$ TS=$(sed -n 's/.*"tree-sitter-cli": "\([^"]*\)".*/\1/p' package.json)   # e.g. 0.25.3
$ test -n "$TS"                                                          # the pin is recorded
$ curl -fsSL -o /tmp/opencode/tree-sitter.gz \
      "https://github.com/tree-sitter/tree-sitter/releases/download/v$TS/tree-sitter-linux-x64.gz"
$ gunzip -f /tmp/opencode/tree-sitter.gz && chmod +x /tmp/opencode/tree-sitter
$ /tmp/opencode/tree-sitter --version   # must print $TS, then put it on PATH
$ export PATH=/tmp/opencode:$PATH      # so §9.5 and §10.4 can call `tree-sitter` bare
$ tree-sitter generate
$ git diff --exit-code -- src
$ tree-sitter test
```

Why the binary and not a package manager: `@0.25` is a **range** and `npx --yes` executes
whatever the registry serves with the runner's credentials, in a repository whose Rust
side is `CARGO_NET_OFFLINE`; and a machine that builds this grammar has `cargo`, `clang`
and `curl` but is not guaranteed to have `npm` at all (this host has none). The release
binary is one file, verified by its version string, so the gate needs no lockfile and no
package tree. `git diff --exit-code -- src` is the grammar's `blastit fmt --check`: if
`parser.c`, `grammar.json`, `node-types.json` or `src/tree_sitter/parser.h` is stale, the
diff is non-empty and the gate fails. Zed's own `LANGUAGE_VERSION` ceiling was not
measured (research, open questions); the defence is a dev install, which the guard cannot
automate, and §12.6 records as the manual pass.

### 9.5 The corpus: nine files, and the five traps

`test/corpus/*.txt` (`tree-sitter test`), each with a name that says what it protects:

| Corpus case | Protects |
|---|---|
| `sections.txt` — the reference's complete example | every section rule, one file |
| `contextual-keywords.txt` — `view view GET "/x"`, `trait session` + `view session`, `postgres view` | a keyword is legal as a name, because `word: $ => $.name` and the lexer is parse-state aware. A grammar that reserved keywords would reject files `blastit` accepts |
| `trailing-comma-newline.txt` — `immutable "/assets/*",\n "/fonts/*"` and a wrapped `functions st_distance,\n unaccent` | a newline after a comma continues the list |
| `multi-line-raw-string.txt` — `query 'SELECT count(*)\nFROM products'` | `'…'` spans lines and is byte-verbatim |
| `env-and-query.txt` — `postgres analytics $ANALYTICS_URL`, `query "SET TIME ZONE 'UTC'"`, `query '… WHERE price > $?'` | `env_var`, the `"…"` spelling of `query`, and `$?` staying inside the string |
| `qualified-function.txt` — `functions st_distance, pg_catalog.unaccent` | a dotted function name is one `dotted_name` node, the form `blastit fmt` writes (§9.2 decision 5, R43a) |
| `regex-pattern.txt` — `view v regex '/v[0-9]+/items' { }` | `regex_raw_string` / `regex_body`, and `@string.regex` |
| `one-line-block.txt` — `static "/" { root "public" }` | `}` terminates a statement; no trailing `;` needed |
| `method-tokens.txt` — `M-SEARCH "/x" { }` and `GET2 "/y" { }` (handed to `MB.22.4` with the `method` widening, §12.8) | `is_method_word` accepts any `[A-Z]`-led token, so the grammar must too; today it is the one known false red (§9.3) |

`test/fixtures/complete.Blastfile` is the reference page's full example;
`test/fixtures/captures.Blastfile` is the small file §10.4 (b)–(c) run against — one
`$ANALYTICS_URL`, one `postgres unrestricted`, one `transactions`, one `select products(id)`;
`test/fixtures/app.Blastfile`, `entrypoint.Blastfile`, …, `static.Blastfile` are one file
per section. All of them are parsed by the `query` runs of §10.4 and by the §9.2
"no false red" test, which must pass for **every** fixture — `grep -c` prints `0` and
exits `1` when there is no match, so the gate is written to treat "no match" as
success:
```
$ for f in test/fixtures/*.Blastfile; do
    test "$(tree-sitter parse "$f" | grep -cE 'ERROR|MISSING' || true)" -eq 0 || exit 1
  done
```

R-ids: **R40** (the tree and the four committed generated files, `parser.h` included),
**R41** (`name: "blastfile"` = `[grammars.blastfile]` key = `tree_sitter_blastfile`),
**R42** (newlines are not extras; `_terminator` is explicit), **R43** (two string nodes
plus `regex_raw_string` with the `regex_body` alias), **R44** (contextual keywords
allowed as names), **R45** (the five traps + `$IDENT` + `regex '…'` + one-line block in
the corpus), **R46** (the exact pinned release binary of §9.4, verified by `--version`, and the
`git diff --exit-code` gate), **R47** (no false red: every fixture under
`test/fixtures/` parses with zero `ERROR`/`MISSING`, and the loop above is the command).

## 10. Queries: four files, fifteen captures, capture by parent node where it matters

### 10.1 The capture rule

Only captures the **45-key theme intersection** contains are used (research §5: `One`,
`Ayu` and `Gruvbox` all style them). The design uses **fourteen of the 45**, plus one
documented exception; a capture outside that set renders as plain text and is not an
error, which is why the guard must reject it instead of trusting the editor to look
wrong. The exception is `variable.special`, which Ayu lacks — it is used only as the
**first half of the right-to-left fallback pair** `@variable.special @variable` on
`$IDENT`, which is the research's own rule and the only place the pair is needed. So the
allowlist the guard enforces is **fifteen names**, `variable.special` included.

| Capture | Node(s) |
|---|---|
| `@keyword` | the six section words, `(directive (keyword))`, `use`, the grant-block `postgres`, `query` |
| `@function` | `(table_grant (keyword))` — `select`, `insert`, `update`, `delete` |
| `@constant` | `(mode (keyword))` — `transactions`, `functions`, `unrestricted`; `(grant (keyword))` — `postgres unrestricted`; `(codec)` |
| `@type` | `(language)`, `(trait_decl (name))`, `(store_decl (name))`, `(grant (name))`, `(use_directive (name))` |
| `@function` (names) | `(view_decl (name))` |
| `@variable.special` + `@variable` | `(env_var)` — the pair, in that order |
| `@string` | `(string)` — every string, including a view's pattern and a `query "…"` |
| `@string.escape` | `(string (escape))` |
| `@string.special` | `(raw_string)` |
| `@string.regex` | `(regex_raw_string)` |
| `@attribute` | `(method)` |
| `@property` | `(table)`, `(column)`, `(dotted_name)` — a qualified function name colours like a table name |
| `@punctuation.bracket`, `@punctuation.delimiter` | `{ } ( )`, `, ;` |
| `@comment` | `(comment)` |

`@constant.builtin`, `@type.builtin`, `@function.method`, `@function.builtin` and
`@string.special.symbol` are **never** used: four of them are outside the intersection,
and the last has no node to attach to (§9.2 drops `placeholder`). There is no
`@variable.parameter` either, because there is no `bind` node (§9.3).

### 10.2 `highlights.scm`, complete

```scheme
; --- sections (literal words: they are contextual, but each is unambiguous here)
["app" "entrypoint" "postgres" "trait" "view" "static"] @keyword

; --- directives and grants, captured by parent so a view named `view` stays a name
(directive (keyword) @keyword)
(use_directive "use" @keyword)
(store_decl "postgres" @keyword)
(grant "postgres" @keyword)
(mode (keyword) @constant)
(grant (keyword) @constant)          ; `postgres unrestricted`
(table_grant (keyword) @function)    ; select, insert, update, delete

; --- literals
(codec) @constant
(language) @type
(method) @attribute
(string) @string
(string (escape) @string.escape)
(raw_string) @string.special
(regex_raw_string) @string.regex
(env_var) @variable.special @variable

; --- names
(view_decl (name) @function)
(trait_decl (name) @type)
(store_decl (name) @type)
(grant (name) @type)
(use_directive (name) @type)
(table) @property
(dotted_name) @property
(column) @property

; --- punctuation and comments
["{" "}" "(" ")"] @punctuation.bracket
["," ";"] @punctuation.delimiter
(comment) @comment
```

`(view_decl (string) @string)` is not needed: `(string) @string` already covers a
pattern, including the one in the header. The section words are matched as literals
because in a `_section` position only those six are possible — and a `view` that names
a view is a `name`, not the literal, so the two captures never fight. **Everything
else is captured by parent node**, which is the rule that survives a grammar refactor.

### 10.3 `brackets.scm`, `indents.scm`, `outline.scm`, complete

```scheme
; brackets.scm — rainbow brackets only; auto-close comes from config.toml
("{" @open "}" @close)
("(" @open ")" @close)
```

```scheme
; indents.scm — @indent is the required capture; without it Enter keeps the last indent
(block) @indent
```

```scheme
; outline.scm — @item + @name; cmd-shift-o becomes a section navigator
(app_decl) @item
(entrypoint_decl) @item
(entrypoint_decl (string) @name)
(store_decl) @item
(store_decl (name) @name)
(trait_decl) @item
(trait_decl (name) @name)
(view_decl) @item
(view_decl (name) @name)
(static_decl) @item
(static_decl (string) @name)
```

An unnamed `postgres $DATABASE_URL` is `(store_decl) @item` with no `@name`: Zed renders
the declaration's own text (`postgres $DATABASE_URL`). The store's *check* name is `db`
(`docs/reference/blastfile.md` §Stores), which is what `UngrantedProxy`'s sentence
prints when it matters. `@name` on a `(string)` node includes the quotes
(`"public/index.php"`), which Zed does not strip — accepted, because the quotes are the
literal the developer wrote.

**No `injections.scm` in v1.** Zed has no bundled PostgreSQL grammar to inject, and a
missing injection language is a silently dead query file that a reviewer must reason
about; `query '…'` strings stay `@string.special`. Revisit only if Blastit ships
`tree-sitter-sql` itself.

### 10.4 The query runs, and what they prove

Each file is parsed against the grammar **and** run against the fixture, so a node name
that does not exist (a typo, a renamed rule, a field the grammar lacks) fails in CI
instead of silently painting nothing:

```
$ cd editors/tree-sitter-blastfile
$ TS_HL=../zed/languages/blastfile
$ tree-sitter query $TS_HL/highlights.scm test/fixtures/complete.Blastfile
$ tree-sitter query $TS_HL/brackets.scm   test/fixtures/complete.Blastfile
$ tree-sitter query $TS_HL/indents.scm    test/fixtures/complete.Blastfile
$ tree-sitter query $TS_HL/outline.scm    test/fixtures/complete.Blastfile
$ tree-sitter query $TS_HL/highlights.scm test/fixtures/complete.Blastfile | grep -q 'variable.special'
$ tree-sitter query $TS_HL/highlights.scm test/fixtures/captures.Blastfile | grep -q 'constant.*unrestricted'
$ tree-sitter query $TS_HL/highlights.scm test/fixtures/captures.Blastfile | grep -q 'function.*select'
$ test "$(tree-sitter query $TS_HL/outline.scm test/fixtures/complete.Blastfile | grep -cE '^\s*capture: ([0-9]+ - )?item\b')" \
    -eq "$(grep -cE '^(app|entrypoint|postgres|trait|view|static)\b' test/fixtures/complete.Blastfile)"
```

`query` prints one line per match and exits non-zero on an unknown node name or field,
so the assertions are: (a) exit `0` for all four files; (b) `highlights.scm`'s output
contains `variable.special` (and `variable`) on the `env_var` range; (c) `unrestricted`
appears as `constant` while `select` appears as `function`; (d) `outline.scm` prints one
`item` per section (the count is the fixture's own, so no hand-numbering) and a `name` for every
named one. **Assertion (Q1..Q4):** (a) is each command's exit status, (b)–(d) are the
three `grep` lines above; all four belong in `MB.22.4`'s acceptance block, because
`scripts/check_zed_extension.py` is a static guard and cannot run `tree-sitter query`
(§12.8 records that hand-off).

R-ids: **R48** (the fourteen-in-the-intersection captures plus the `variable.special`
exception), **R49** (capture by parent node; no `name:` field, no dotted capture outside
the allowlist), **R50** (`outline.scm` `@item`+`@name`, `indents.scm` `@indent`,
`brackets.scm` `@open`/`@close`), **R51** (no `injections.scm`, with the reason),
**R52** (every query file parses and runs against the fixture, with (b)–(d) asserted).

<!-- APPEND -->
## 11. Security and failure modes

`blastit lsp` is a **text tool on the developer's own machine**, spawned by the
developer's editor, reading the developer's own buffer on stdin. It is not a trust
boundary, and this section's job is to prove it never becomes one. The manifest crate's
own doc says why that is sound: "A `Blastfile` is policy, not input: whoever can write
it can already grant `unrestricted`" (`blastfile/mod.rs:4`). The editor therefore
inherits the manifest's threat model and adds no new asset.

### 11.1 The properties, each with its test

| # | Property | Why it matters | Assertion (all in `critical_lsp.rs` unless named) |
|---|---|---|---|
| S1 | **No capability in the extension.** No `[capabilities]`, no `std::process`, no `Command::new` | executing from the wasm needs `process:exec` **and** a user grant: a permission prompt on a zero-config install | `scripts/check_zed_extension.py` greps the absence (§8.3) |
| S2 | **A document is parsed, never executed.** No `fork`/`execve`, no interpreter, no WASM engine, no path from a document | a `Blastfile` holds a `query '…'` string; running it would be a new attack surface | the *only* deterministic oracle: a document holding `view v "/x" {\n  postgres $DATABASE_URL\n  postgres { query "COPY x FROM PROGRAM 'sh'" }\n}` publishes **exactly** the `LiteralQuery` diagnostic (severity 1, `source: "blastit"`), stderr is empty, and `/proc/<server_pid>/task/<tid>/children` is empty before and after — `/proc/self/task` is the **test** process's thread table and proves nothing |
| S3 | **No privilege, no configuration on the LSP path.** The `Lsp` arm calls `caps::hold_none()` before it reads a byte, and `lsp.rs` reads no environment, no path, no socket | the binary may be `setcap`'d in a deploy; capabilities belong to the process image, not to a code path, so "the arm returns early" alone is not enough | (a) `the_lsp_arm_drops_capabilities` greps `cli.rs` for `hold_none()` occurring before `lsp::run()`; (b) a session spawned with `.env_clear()` completes the handshake and one diagnostic (so no environment is needed, `BLASTIT_EXEC` included) |
| S4 | **A `Blastfile` is never opened.** Only `stdin` is read; `uri`/`rootUri` are opaque strings | reading a path from a `uri` is a traversal surface with zero benefit | (a) the §1.2 grep for `std::fs`/`std::path`/`File::open`; (b) a document whose `uri` names a **FIFO with no writer** (`mkfifo`) — `open(2)` for reading blocks, so a reply within 2 s proves the path was never opened |
| S5 | **No panic escapes a document.** `deny(unwrap_used, expect_used, panic, todo, indexing_slicing)` in `lsp.rs` (enforced by `clippy -D warnings`), a `panic::set_hook` that writes one fixed line, and `catch_unwind` around every `blastit-manifest` call per document | Zed does not restart a server that dies on its own, and the default hook prints the payload — which can quote document text | (a) `include_str!("../src/lsp.rs")` contains `set_hook` and `catch_unwind`; (b) 2 000 framed bodies from a fixed xorshift generator (`serde_json` shapes, truncated UTF-8, `Content-Length` over the cap, every `id` type) leave the process alive with every reply one well-formed frame |
| S6 | **Bounded allocation.** Header ≤ 8 KiB **before** allocation; body allocated only after `N ≤ MAX_FRAME_BODY_BYTES`; a refused body costs one 8 KiB scratch | a `Content-Length: 4294967295` must not become a 4 GiB `Vec` | a `Content-Length: 33619969` frame answers `-32700` and exits `1` within 2 s (the `-32700` proves the header was parsed and the refusal happened before the read); the bound itself is the code path of §3.2 step 3, asserted by review and by `MAX_FRAME_BODY_BYTES == 33_619_968` in `lsp.rs`'s unit tests |
| S7 | **stdout is protocol.** `deny(clippy::print_stdout)`; the only writer is `write_message` | one stray line desynchronises the client permanently | a full session's stdout is split on `Content-Length` and reassembled to exactly the expected messages, byte for byte, and its declared lengths are re-checked as UTF-8 bytes |
| S8 | **No document text in logs.** stderr is the four fixed sentences (§11.3) and nothing else; no `uri`, no text, no value | a log line is still a log line, and a `Blastfile` holds SQL | two sessions. (a) The fixture whose SQL is `SELECT secret_token FROM api_keys`: stderr contains none of `secret_token`, `SELECT`, `api_keys`, the uri. (b) The **fuzz** session of S5, which necessarily writes `the input ended inside a frame`: every stderr line in both sessions is one of the four literals, so the alphabet is not vacuously true |
| S9 | **A broken document is not a broken server.** Parse/format failure → diagnostics and `null`; the loop continues | the accepted page's "`1` only if the client disappears" | F4 and the D-loop tests: after `null`, the next request answers |
| S10 | **Exit codes.** `0` only after `shutdown`; `1` for EOF, an unframeable stream, or a failed `caps::hold_none()` (§2.4); `2` for a bad argument (the `check`/`fmt` CLI-error convention) | the reference page documents `0`/`1` for the session; §12.8 hands it the two extra causes | seven sessions: clean → `0`; EOF mid-session → `1`; over-cap body → `1`; `exit` without `shutdown` → `1`; `shutdown` then EOF → `0`; `lsp --bogus` → `2`; a binary carrying a capability → `1` (rootless e2e only) |
| S11 | **No second reader.** The server never re-implements the lexer, the parser, `fmt` or the warning triggers | a second reader is a second set of refusals to keep in step | `lsp.rs` contains `blastfile::check` and `blastfile::fmt` and none of `"fn lex"`, `"fn parse"`, `"fn lower"`; the token-scan names are greped out |

**§1.2's grep list, verbatim** (one assertion, one list, used by R3 / `the_lsp_module_is_a_text_tool`):
`["std::fs", "std::path", "std::process", "std::os::unix::net", "libc", "rustix",
"blastit_sys", "crate::stage0", "crate::stage1", "crate::host", "crate::config",
"std::env", "include!"]` — and the positive half, that `lsp.rs` **does** contain
`blastfile::check` and `blastfile::fmt`.

### 11.2 The failure-mode table a reader should be able to answer from

| Situation | What the developer sees | Wrong-but-plausible alternative |
|---|---|---|
| `body = json` | one squiggle at the `=`, the CLI's sentence | an editor-only wording |
| a file over 16 MiB | one diagnostic at `(0,0)`: `the file is N bytes; Blastfile is limited to 16777216 bytes …`, and the buffer is not stored | nothing, or a crash |
| a file with 600 problems | 511 squiggles + `` 89 more problems; run `blastit check` for the full list `` | 600 squiggles on a 40-line file, or `600 more` when 89 is the count |
| a SQL grant refused by `validate` | one diagnostic at `(0,0)` (the `Location` family, §4.2) | a squiggle on the statement, which v1 cannot place |
| 33 open Blastfiles | the 33rd gets the `tracks at most 32` notice; the other 32 still answer | silent drop, or unbounded memory |
| an incremental `didChange` (a client that ignores `change: 1`) | `incremental sync is not supported; reopen the file`, and the old diagnostics are gone | stale diagnostics that show `unrestricted` as clean |
| save a mis-indented line | the line snaps back, one undo step | an error dialog |
| save a broken line | nothing happens; the squiggle stays | "language server error" |
| save a CRLF file | the file becomes LF, once (§6.3) | a file that `blastit fmt --check` then fails |
| `blastit` not installed | the extension's one-line error naming `uv add blastit` | silence, then "language server not running" |
| the editor writes garbage on stdin | `-32700` and, if unframeable, exit `1` | a panic, or a hung server |

### 11.3 The four stderr sentences, verbatim

```
blastit lsp: the input ended inside a frame
blastit lsp: the header block is too long
blastit lsp: the body is too long
blastit lsp: internal error
```

Each is ≤ 64 bytes, ASCII, and constant, and the fourth is the panic hook (§3.3). There
is no fifth: an I/O error on stdin or stdout ends the loop with the same "ended inside a
frame" line, because the stream is gone either way. **S8's assertion** is (a) every
stderr line is one of these four literals and (b) none contains the fixture's SQL, its
uri, or the string `SELECT`.

R-ids: **S1–S11** as numbered; they are cross-referenced from §12's matrix.

## 12. Testing matrix and release

Every row below names a command or a test that **fails when the property is deleted**.
Rows marked *hand-off* belong to a sibling node's acceptance block; §12.8 lists them
because `MB.22.2` may not edit another node's `NODE.md`.

### 12.1 `crates/blastit/tests/critical_lsp.rs` (protected, new file)

Spawns the test binary's `lsp` subcommand over real pipes
(`std::process::Command::new(env!("CARGO_BIN_EXE_blastit")).arg("lsp")`), writes
frames, reads frames, and asserts.

| Test | Asserts | § |
|---|---|---|
| `handshake_publishes_the_capability_document` | the exact §2.3 JSON (parsed and compared field by field, `version` = `CARGO_PKG_VERSION`) | R6 |
| `frame_length_is_utf8_bytes` | a `didOpen` whose `uri` is `file:///tmp/Blåstfile` and whose text is broken; the raw `publishDiagnostics` frame's declared `Content-Length` equals `body.as_bytes().len()` (the `å` is 2 bytes, 1 char) — the writer's bytes, not only the reader's | R4 |
| `ids_echo_verbatim` | numeric → number, string → string, absent → no reply, explicit `null` → `-32600` with `id: null` | R5 |
| `malformed_body_answers_the_right_code_and_serves` | `{` → `-32700`; invalid UTF-8 → `-32700`; `[]` → `-32600`; then a valid `hover` answers | R7, R13 |
| `error_codes_are_the_table` | `-32600` for `initialize` twice, for `jsonrpc != "2.0"`, for a request after `shutdown`; `-32601` for an unknown method and for a `$/…` **request**; `-32002` for a request before `initialize`; `-32602` for `hover` without `position` and `formatting` without `uri` | R7 |
| `work_done_progress_is_answered` | `window/workDoneProgress/create` → `{"result":null}`, no hang | R7 |
| `unknown_notification_is_dropped` | zero bytes on stdout for an unknown notification, and the session continues | R7 |
| `oversized_body_is_refused_before_reading` | `Content-Length: 33619969` → `-32700`, exit `1`, within 2 s; `Content-Length: 4294967295` with no body → `-32700`, exit `1`, also within 2 s (the answer precedes any read attempt) | R12, S6 |
| `header_cap_boundaries` | a block of exactly 8 192 bytes (terminator included) is served; 8 193 → `-32700`, exit `1` | R10 |
| `positions_are_utf16` | P2's `😀` fixture: `character` 20, not 19 | R14 |
| `positions_line_split_and_none` | P1, P4 | R14, R15 |
| `squiggle_covers_the_word_or_one_unit` | P5 and the `=` fixture | R16 |
| `diagnostics_carry_severity_source_and_message` | D1, D2: `severity` 1/2, `source` `"blastit"`, no `code` key, `message == error.message()` | R19, R20 |
| `warning_sites_are_their_declarations` | D7: seven diagnostics on seven (file, line) sites — A: 0, 2, 3; B: 2, 7, 11, 15 — each fragment at the line its word is on | R22 |
| `warning_display_is_unchanged` | `ManifestWarning`'s `Display` for all six has no `:<digit>:` and matches today's boot text byte for byte | R21 |
| `diagnostics_are_capped` | D6: 600 `GrantOnUnknownProxy` views → 512 diagnostics, `[511].message` starts `89 more problems` | R24 |
| `open_documents_are_capped` | 33 `didOpen`s: the 33rd gets the §2.5 notice at `(0,0)-(0,0)`, severity 2, and the 32nd still answers a `hover` | R8 |
| `oversized_document_is_not_stored` | a 16 777 217-byte document publishes `TooLarge` at `(0,0)`; a later `hover` for it answers `null` | R8 |
| `did_change_for_unknown_uri_does_not_grow_the_map` | 40 `didChange`s with fresh uris, then 32 `didOpen`s: the 33rd still gets the notice (the map holds 32, not 72) | R8 |
| `incremental_change_is_refused_visibly` | a `didChange` with `range` publishes one severity-2 `(0,0)` notice and a following `hover` answers `null` | R8 |
| `formatting_equal_is_empty` / `_diff_is_one_edit` / `_broken_is_null` | F1, F2, F4 | R25, R26 |
| `formatting_normalises_crlf_once` | F5 (a)–(d) | R27 |
| `hover_finds_the_word` / `_unknown_is_null` / `_env_var_paragraph` / `_inverse_utf16` | H1–H5 | R30–R32 |
| `definition_is_declined_and_unadvertised` | `textDocument/definition` → `null`, and the capability document has no `definitionProvider` | R33, R34 |
| `lsp_subcommand_is_reachable` | `lsp --help` exits `0` with `usage: blastit lsp` first; a bad argument exits `2` | R1 |
| `the_lsp_arm_drops_capabilities` | `include_str!("../src/cli.rs")` has `hold_none()` before `lsp::run()` in the `Lsp` arm | S3 |
| `the_lsp_module_is_a_text_tool` | `include_str!("../src/lsp.rs")` contains none of §11.1's list and contains `blastfile::check` + `blastfile::fmt` | R3, S1, S11 |
| `no_position_of_a_validation_error_is_invented` | `include_str!("../../blastit-manifest/src/error.rs")` does not contain `_ => Position::NONE` | R3a |
| `a_document_is_never_executed` | S2's `LiteralQuery` fixture, empty stderr, and `/proc/<server_pid>/task/<tid>/children` empty before and after | S2 |
| `no_path_from_a_document_is_opened` | S4's FIFO-with-no-writer `uri` answers within 2 s | S4 |
| `stdout_is_frames_only` | S7 | S7 |
| `stderr_never_holds_document_text` | S8 | S8 |
| `exit_codes` | S10's seven sessions, including `shutdown`-then-EOF → `0` and the capability refusal | R13, S10 |
| `fuzz_shaped_bodies_do_not_kill_the_loop` | S5's 2 000 bodies | S5 |

`lsp.rs`'s own `mod tests` covers the pure functions and the table:
`Lines::new`/`point` (P1–P6 arithmetic), `char_index`, `max_point_utf16` (F1's five
rows), `word_at`, `assert_eq!(MAX_FRAME_BODY_BYTES, 33_619_968)`, and
`hover_table_is_bounded_and_transcribed` (H6). Every string assertion about hover
*wording* lives there, never in the protected file.

### 12.2 `crates/blastit-manifest` tests (`unit` + existing critical files, unmodified)

| Assertion | Where |
|---|---|
| `message()` strips exactly `prefix(position())`, for one instance per error family | `src/error.rs` `mod tests` |
| `position()` has no wildcard arm | the `the_lsp_module_is_a_text_tool`-style grep (§12.1), because Rust cannot observe a missing match arm |
| `check()` returns the seven warning instances of D7's two fixtures, in `warnings`' order | **new** `tests/warning_sites.rs` |
| `sites()` returns `Position::NONE` for a warning whose declaration is absent, and `check("")` returns no warnings | `src/blastfile/sites.rs` `mod tests` |
| `MAX_SOURCE_BYTES` is `pub` and equals `16_777_216` | `src/blastfile/mod.rs` `mod tests` |
| boot warning text is byte-identical to today's | the **existing** `tests/validate.rs` / `critical_manifest_011.rs` pass unchanged — that is the proof, and it is why no variant gained a field (§5.5) |

### 12.3 The grammar `rev` pin — the procedure, in order

Leaf workers do not commit, so `MB.22.4` writes the all-zero placeholder and
`MB.22.5` replaces it:

```console
$ REV=$(git rev-parse HEAD)                       # the lead's commit of MB.22.4
$ git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"
$ test ${#REV} -eq 40 && printf '%s' "$REV" | grep -qE '^[0-9a-f]{40}$'
$ rm -rf /tmp/opencode/grammar-fetch && git init -q /tmp/opencode/grammar-fetch
$ git -C /tmp/opencode/grammar-fetch remote add origin "file://$PWD"
$ git -C /tmp/opencode/grammar-fetch fetch -q --depth 1 origin "$REV"
$ git -C /tmp/opencode/grammar-fetch checkout -q FETCH_HEAD
$ test -f /tmp/opencode/grammar-fetch/editors/tree-sitter-blastfile/src/parser.c
$ ! grep -rn '0000000000000000000000000000000000000000' editors/zed
```

| Rule | Reason |
|---|---|
| 40 lowercase hex, one commit | Zed fetches `--depth 1 <rev>`; a branch, a tag or `HEAD` is not a reproducible build input, and the registry re-clones on every install |
| the commit contains **both** `editors/tree-sitter-blastfile` and `editors/zed` | `path =` is resolved inside that commit; a grammar-only commit leaves the pin paired with a different extension |
| **no `||` fallback on the fetch** | a by-SHA fetch that fails is the failure the check exists for; `|| git fetch origin <branch>` would check out the branch tip and the `test -f` would pass. `git cat-file -e` (local object + path) is the gate that always runs; the shallow fetch is the step that fails on a server with `uploadpack.allowReachableSHA1InWant` off, and `MB.22.5` records that outcome rather than hiding it |
| a public fetch only succeeds after `conductor` pushes the release branch | the registry needs the commit on the remote; `MB.22.5` records the push as a precondition, not a proof |
| the placeholder appears nowhere | the last command; a forgotten placeholder is a registry-wide failure |

### 12.4 `scripts/check_zed_extension.py` (new, stdlib, in `make check` via `make zed-check`)

One line of output per property, from a **root argument** so its self-tests can point
it at crafted trees; it never reads the network and never runs `tree-sitter`. It
asserts: §8.2's nine rules; §8.4's **eight** keys (`path_suffixes` and `line_comments`
compared as literals); §8.1's files present and `LICENSE` resolving inside the
directory; `target/`, `extension.wasm` and `grammars/` absent **and** matched by
`.gitignore`; §9.1's four generated files (`parser.c`, `grammar.json`,
`node-types.json`, `src/tree_sitter/parser.h`) present and `tree_sitter_blastfile`
exported; no `src/scanner.c`, no `src/scanner.cc`, and no `externals` in `grammar.js`
(a hand-written scanner would be C running in the editor's grammar wasm); §10.1's
capture allowlist (the fifteen names, the `variable.special`/`variable` pair included,
nothing else); `outline.scm` `@item`+`@name`, `indents.scm` `@indent`, `brackets.scm`
`@open`/`@close`; `src/lib.rs`'s `which` order, the **six** argv literals with
`--no-sync`, exactly one `shell_env()`, no `std::process`, no `[capabilities]`;
`Cargo.toml`'s empty `[workspace]` and `crate-type = ["cdylib"]`; and F6's two
`.zed/settings.json` facts (the committed file, and the fence in `docs/local.md`).

`Makefile` gains `zed-check` as a **prerequisite of `check`** — a target nobody runs is
not a gate — and `mb22-check`-style wiring is not needed: one line in the `check:` list.

`scripts/test_check_zed_extension.py` removes each property from a temp tree and
requires a red run. Its cases are the twelve `MB.22.5` will name: a
branch-shaped `rev`; `description` not longer than `name`; no `[workspace]`;
`path_suffixes` typo; a missing `language_ids`; a capture outside the allowlist;
missing `@item`/`@indent`; `tree_sitter_blastfile` not exported; a committed
`extension.wasm`; two `shell_env()` calls; swapped `which` order; `"on"` flipped to
`"off"` in `.zed/settings.json`.

### 12.5 The live proof: both distribution paths

| Path | How it is driven |
|---|---|
| **standalone** (`target/debug/blastit lsp`, no Python linked, `make rs-no-python`) | `BLASTIT_LSP_COMMAND="$PWD/target/debug/blastit lsp"` on the protected Python test |
| **console script** (`uv run blastit lsp` → `_blastit.main(["lsp"], …)`) | `python/tests/critical/test_lsp_stdio.py`, spawned as `sys.executable -m blastit lsp` under `test-critical`'s staged `PYTHONPATH` |

The Python route needs the staged tree: `_blastit.main` stats
`blastit/_bin/blastit-dbproxy` and refuses before dispatch when it is missing
(`blastit-py/src/lib.rs:62`), even though the LSP never uses it. That is why the test
runs under `make test-critical`'s `PYTHONPATH=target/python-critical:python`, which
stages the dbproxy — a future reader must not "simplify" it to run from a bare
`python/` checkout.

`python/tests/critical/test_lsp_stdio.py` (new, protected) sends **one document through
three texts** on one `uri`, so all three §6.1 formatting outcomes are exercised:

1. `initialize`; assert the capability document.
2. `didOpen` the broken `"# ✓ ok\nview v \"/x\" {\n  body =\n}\n"`; assert exactly one
   diagnostic, its `line` = 2 and its `character` = 7, and that its `message` equals the
   captured `blastit check` line for the same file with the `Blastfile:<line>:<col>: `
   prefix removed. Then send `textDocument/formatting` for that **broken** buffer and
   assert the reply is **`null`** (§6.1's third outcome: a broken buffer is declined, not
   an error).
3. `didChange` to the canonical `"app \"shop\"\n\nview v \"/x\" {\n    body json\n}\n"`;
   `formatting` → `[]` (equal), and the diagnostic set is now `[]`.
4. `didChange` to `"app \"shop\"\n\nview v \"/x\" {\n  body json\n}\n"` (two spaces);
   `formatting` → exactly one `TextEdit` whose `end` is `(5,0)` and whose `newText` is
   the canonical text.
5. `shutdown` → `null`; `exit`; process status `0`.

So the three outcomes are `null` (step 2), `[]` (step 3) and one edit (step 4), all in
one session, one `uri`, three texts. A second session closes stdin before `shutdown` and
asserts status `1`. When `BLASTIT_LSP_COMMAND` is set, the test shell-splits it and drives
that command instead, so one protected suite proves both routes while `critical_lsp.rs`
proves the protocol.

### 12.6 What only a human can prove

One live Zed pass, recorded in `MB.22.5`'s `RESULT.md` as commands + observations: install
the dev extension, open `./Blastfile`, see colours/brackets/outline, type `body =` and
read the sentence, save a mis-indented line and watch it snap back, hover
`unrestricted`, and confirm Zed ≥ 0.205 loads the grammar wasm (the `LANGUAGE_VERSION`
ceiling is unverified in the research). This is the one row no `make check` target can
own.

### 12.7 Registry publication preconditions (human-only)

| Precondition | Source | Blocks |
|---|---|---|
| `LICENSE` inside `editors/zed/` (symlink accepted) | registry license requirements | install |
| `description` longer than `name`; `version` strict semver, never decreasing | `validate_extension_manifest` | PR |
| one PR per extension; `pnpm sort-extensions` before pushing | publishing guide | PR review |
| `extensions.toml`: `[blastfile] { submodule = "extensions/blastfile", path = "editors/zed", version = "<extension version>" }` | publishing guide | PR |
| the submodule's checked-out commit is on a **pushed** branch | §12.3's rev rule | registry clone |
| the submodule PR to `zed-industries/extensions` is opened by a human | Blastit's release policy (agents never push tags/PRs upstream) | publication |

None of these is automatable from this repository; `MB.22.5`'s `RESULT.md` records
which of them had been done when it ran.

### 12.8 Hand-offs: what `MB.22.3`–`MB.22.5` must change in their own text

These are **plan** facts, not design facts: this node cannot edit another node's
`NODE.md`, and a gate that is not in an acceptance block does not run. The lead applies
them; `RESULT.md` and the dispatch `FOLLOWUPS` list them.

| Node | Change | Why |
|---|---|---|
| `MB.22.3` | the smoke command must **read stdout**: `out=$(printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \| cargo run -q -p blastit -- lsp); printf '%s' "$out" \| grep -q '"positionEncoding":"utf-16"'` | `test $? -eq 1` passes with the `lsp` arm deleted (`from_argv` falls through to `Serve`, which exits 1) |
| `MB.22.3` | `! grep -q '_ => Position::NONE' crates/blastit-manifest/src/error.rs` | the wildcard removal has no observable test |
| `MB.22.3` | a test asserts a positioned *syntax* error's exact site, a `Location`-carrying validation error at `(0,0)`, and a whole-file refusal at `(0,0)` | "a validation/SQL error's site" is unimplementable while `Location` has no span (§4.2) |
| `MB.22.3` | `didChange` takes the **single** `contentChanges[0].text`; zero or more than one change object is **dropped silently** (§2.2 item 4, §2.5), not "takes the first" | §2.5 and §12.8 must state one behaviour; "takes the first" is a third |
| `MB.22.4` | `uv run --frozen --no-sync --quiet blastit lsp`, and the pinned `tree-sitter` release binary of §9.4 instead of `npx --yes tree-sitter-cli@0.25`, in its brief and acceptance | §8.3 and §9.4: the first executes the worktree's build backend, the second runs an unpinned package from the network |
| `MB.22.4` | the three `grep` assertions of §10.4 (b)–(d) and the fixture loop of §9.5 | `query`'s exit status alone proves only that the files parse |
| `MB.22.5` | `grep -Eq '^check:.*zed-check' Makefile` | a target that is not a prerequisite never runs |
| `MB.22.5` | drop the `\|\| git fetch origin release/0.2.0` fallback from the shallow-fetch line | the fallback checks out the branch tip, which is exactly what §12.3 forbids |
| `MB.22.4` and `MB.22.5` | delete the `tools/guard.py` claim from both acceptance blocks (`MB.22.4` item 7, `MB.22.5` item 6) and say `scripts/check_zed_extension.py` is the checker for `editors/**` | §8.1: `guard.py` bans wheels and oversized files and has no `editors/**` rule |
| `MB.22.4` and `MB.22.5` | say fifteen named captures, not "the 45-theme allowlist": §10.1's allowlist is the fifteen names this design uses, and the guard enforces exactly those | §10.1, §12.4 |
| `MB.22.4` | three edits to `editors/`: widen `method` to `[A-Z][A-Z0-9_-]*` (corpus line for `M-SEARCH`), add `(dotted_name) @property` to `highlights.scm` (§10.2), then regenerate `src/` with the §9.4 binary so `git diff --exit-code -- src` is empty | §9.3 names this as the grammar's one false red; `parser.rs`'s `is_method_word` accepts such methods |
| `MB.22.3` | drop "feed a non-ASCII diagnostic message" from bullet 1 and say the writer oracle is the non-ASCII **`uri`** (`Blåstfile`, §12.1) | every diagnostic message is ASCII by construction (§5.2, §11.3), so that instruction cannot be followed |
| `MB.22.3` and `MB.22.6` | say the `# ✓ ok` fixture (now wrapped in a `view`) proves the 0-based **line** and `col-1`; the UTF-16 unit oracle is P2's `😀` fixture | §4.5: `✓` is on a line the error is not on |
| `MB.22.5` | add `git cat-file -e "$REV:editors/zed/extension.toml"` beside the parser.c check | the pin must prove both halves of the commit (§12.3) |
| a follow-up docs node (MB.22.1 is closed) | `docs/reference/cli.md` gains **three** rows: `lsp --bogus` → exit `2`; exit `1` also covers a stream the server cannot frame and a process that cannot drop its file capabilities; and the editor's `uv` form is `uv run --frozen --no-sync --quiet blastit lsp` | §2.4/S10 and §8.3 make the page's "`0`/`1`" sentence incomplete, and its `uv run blastit lsp` is the syncing command §8.3 rejects. The page belongs to closed MB.22.1, so no open node can edit it |


### 12.9 The requirements this section owns (R53–R59)

Each is a claim §12 makes and §12.1–§12.8 prove; before this list they were cited by the
Requirement index and nowhere stated.

| R | Requirement | Proved by |
|---|---|---|
| **R53** | `crates/blastit/tests/critical_lsp.rs` exists and asserts every protocol property of §12.1's table, one test per row, each failing when its property is removed | §12.1's 34 rows, run by `cargo test -p blastit --test critical_lsp --locked` |
| **R54** | the manifest-side tests exist and the **existing** critical files pass unmodified, which is the proof that no `ManifestWarning` variant gained a field (the `check()`/`sites()` route) | §12.2; `tests/warning_sites.rs`, `error.rs`'s and `sites.rs`'s `mod tests`, `critical_manifest_011.rs` |
| **R55** | the grammar `rev` is a 40-hex commit that contains both `editors/tree-sitter-blastfile` and `editors/zed`, proven without a branch or tag and without a `\|\|` fallback | §12.3's nine commands |
| **R56** | `scripts/check_zed_extension.py` runs inside `make check` (it is a `check:` prerequisite), asserts the properties of §12.4, and its negative self-tests go red when each property is removed | §12.4; `make zed-check` |
| **R57** | `blastit lsp` serves the same session from both distribution paths — the Python-free standalone binary and the wheel's console script | §12.5's two rows |
| **R58** | the live Zed pass is recorded as a human step with its commands and observations, and is the only row no `make check` target owns | §12.6 |
| **R59** | the registry preconditions are written down, and `MB.22.5` records which of them held when it ran | §12.7 |
## Requirement index

| R | § | Proving tier |
|---|---|---|
| R1–R3, R3a | §1 | `lsp_subcommand_is_reachable`, the two `include_str!` greps; `cargo clippy -D warnings` for "no new dependency" |
| R4–R8 | §2 | `critical_lsp.rs` (ids, codes, documents, byte lengths) |
| R9–R13 | §3 | `critical_lsp.rs` (caps, stdout purity, stderr alphabet, exits) |
| R14–R17 | §4 | `critical_lsp.rs` + `lsp.rs` unit tests (P1–P6) |
| R18–R24 | §5 | `critical_lsp.rs`; `error.rs`/`warning_sites.rs` tests |
| R25–R29 | §6 | `critical_lsp.rs`; `scripts/check_zed_extension.py` for R29 |
| R30–R34 | §7 | `critical_lsp.rs` (contract) + `lsp.rs` unit tests (wording, H6) |
| R35–R39 | §8 | `scripts/check_zed_extension.py` + its negative self-tests |
| R40–R47, R43a | §9 | the pinned `tree-sitter` binary's `generate`/`test`/`parse`, `git diff --exit-code`, the fixture loop; `R43a` is the dotted `function` node of §9.2 |
| R48–R52 | §10 | the four `tree-sitter query` runs and (b)–(d)'s three `grep`s |
| R53–R59 | §12 | the commands of §12.1–§12.5; §12.6–§12.7 are human-only and recorded in `MB.22.5`'s `RESULT.md`; all seven are stated in §12.9 |
| S1–S11 | §11 | `critical_lsp.rs` and the guard, one row each (§11.1) |

Four fresh-context critics read this document and `ADR-0013` blind and wrote
`docs/design/blastfile-lsp-zed.review-<lens>.md`. Every finding is answered below:
**accepted → the section that changed**, **answered → why the finding does not hold**,
or **hand-off → a sibling node's acceptance block (§12.8) or a follow-up**. Nothing is
deferred silently.

### Security (`review-security.md`: 3 BLOCKER, 8 MAJOR, 9 MINOR)

| # | Grade | Resolution |
|---|---|---|
| B1 | BLOCKER | **accepted** → §1 row 3, §1.1, §11 S3, §12.1: the `Lsp` arm calls `blastit_sys::caps::hold_none()` before it reads a byte (the call `--role=dbproxy` already makes at `cli.rs:638`), so editor bytes are parsed by a process holding no capability. Capabilities belong to the process image, so "the arm returns early" was true and insufficient. |
| B2 | BLOCKER | **accepted** → §8.3, ADR §6: the `uv` argv is `run --frozen --no-sync --quiet blastit lsp`; a bare `uv run` syncs the project, i.e. runs the worktree's build backend with `shell_env()`. The error string now names `uv sync`, and the guard asserts the six literals and `--no-sync`. |
| B3 | BLOCKER | **accepted** → §2.2, §2.5, §12.1: `didOpen` is the only insertion point; a `didChange` for an unknown `uri` publishes the "no open document" notice and inserts nothing, so the map cannot pass `MAX_DOCUMENTS`. |
| M1 | MAJOR | **accepted** → §11 S3: the property is restated precisely (`lsp.rs` reads no environment; the `Lsp` arm runs before any privilege/config path) and names the two reads that happen before dispatch (`Host::binary()`'s `BLASTIT_EXEC`, `_blastit.main`'s dbproxy stat); the test is now `.env_clear()`, which kills both. |
| M2 | MAJOR | **accepted** → §1.2, §11 S2/S11: the oracle is `/proc/<server_pid>/task/<tid>/children`, and the grep list gained `std::process`, `libc`, `rustix`, `std::os::unix::net`, `std::fs`, `include!`. |
| M3 | MAJOR | **accepted** → §11 S4: a FIFO with no writer is the runtime oracle (`open(2)` for reading blocks), plus the `std::fs`/`std::path` grep. |
| M4 | MAJOR | **accepted** → §3.3, §11 S5/S8: `panic::set_hook` writes one fixed line and `catch_unwind` guards every per-document call; stderr is an alphabet of four literals, asserted line by line. |
| M5 | MAJOR | **accepted** → §2.5, §3.1: `MAX_DOCUMENT_BYTES = MAX_SOURCE_BYTES` bounds each entry; an oversized document is not stored (and its `TooLarge` is published), so 32 × 33 MiB is unreachable. |
| M6 | MAJOR | **accepted** → §4.2, §4.5 P6: `Lines` builds the line-offset table once per publish and `point()` binary-searches; the bound is stated (`O(log lines + col)`) and timed. |
| M7 | MAJOR | **accepted** → §9.1, §9.4, §10.4: the generator is the pinned **release binary** (no `npm`, no `npx`, no lockfile — this host has none), verified by `--version`; `package.json` records the exact version |
| M8 | MAJOR | **accepted** → §2.2, §2.5, §11.2: a `didChange` carrying `range` is refused **visibly** (notice plus removal from the map), so a client that ignores `change: 1` cannot leave stale diagnostics. |
| m1 | MINOR | **accepted** → §11.2 now reads `89 more problems`, matching §5.7's arithmetic. |
| m2 | MINOR | **accepted** → §2.5 and §5.1: the three session notices carry no `version`, and §5.1 says so. |
| m3 | MINOR | **accepted with a hand-off** → §2.4, §11 S10: a bad argument exits `2` (the `check`/`fmt` convention); the reference page's one-line `2` row is **hand-off** (§12.8, `docs/reference/cli.md` is not this node's path). |
| m4 | MINOR | **accepted** → §3.3: the page's "logs go to stderr" is satisfied (nothing else may go to stdout); a healthy session writes nothing, which is the assertion, not a promise about verbs. |
| m5 | MINOR | **accepted** → §3.1, §3.2: the cap includes `\r\n\r\n`, and both boundaries (8 192 accepted, 8 193 refused) are asserted. |
| m6 | MINOR | **answered**: `shell_env()` is handed over verbatim on purpose. The server reads no environment (§1.2, grep-proven), so the exposure is the same environ any command the developer's shell starts has; filtering would break `uv`'s own resolution (`HOME`, `XDG_CACHE_HOME`, `UV_*`, `VIRTUAL_ENV`, `PYTHONPATH` are exactly what it needs), which is a new failure mode on the documented zero-config path. What the editor *says* is constrained instead: H3 asserts no `DATABASE_URL` value ever appears in a hover reply. |
| m7 | MINOR | **accepted** → §12.4: the guard asserts no `scanner.c`/`scanner.cc` and no `externals` in `grammar.js`. |
| m8 | MINOR | **accepted** → §7.5 H3: a hover on `$DATABASE_URL` with the variable set asserts the reply never contains the value. |
| m9 | MINOR | **accepted** → §2.2: an explicit `"id": null` is `-32600` with `id: null`, so a client is never left waiting. |

### Implementability (`review-implementability.md`: 4 BLOCKER, 9 MAJOR, 15 MINOR)

| # | Grade | Resolution |
|---|---|---|
| 1 | BLOCKER | **accepted** → §1 rows 5–8, §5.5: the span mechanism moved out of `model.rs` entirely. `sites()` (new, `blastfile/sites.rs`) locates each warning in the CST and `blastfile::check()` returns `Checked { manifest, warnings, sites }`; `model.rs` and `validate.rs` are unchanged, so there is **no path widening**. |
| 2 | BLOCKER | **accepted** → §1: `position()` exists but ends in `_ => Position::NONE` (`error.rs:1468`); `MB.22.3` replaces that arm with the explicit position-less list, and §12.1 greps for its absence. |
| 3 | BLOCKER | **accepted** → §9.2: `_view_header` is `optional(_methods) optional(_patterns)` with `_pattern = string | regex_raw_string`, so `view products GET "/products", …` and `view v regex '…'` parse. |
| 4 | BLOCKER | **accepted** → §9.2, §10.2, §10.3: no `field('name', …)` is declared and every query is positional (`(view_decl (name) @function)`), so no query can name a field the grammar lacks. |
| 5 | MAJOR | **accepted** → §5.5: gone with `model.rs`; D7's fallback is `sites()` over an empty tree, not a `Manifest` constructor, and the six-warning fixture is written out line by line. |
| 6 | MAJOR | **accepted** → §2.2, §2.5 (same fix as security B3). |
| 7 | MAJOR | **accepted** → §8.1: `tools/guard.py` is explicitly **not** the checker for `editors/**`; the guard script is. |
| 8 | MAJOR | **accepted** → §6.5, §12.4: F6 moved to `scripts/check_zed_extension.py` (the committed file **and** the fence in `docs/local.md`); `python/tests/test_docs_assets.py` is not touched. |
| 9 | MAJOR | **accepted** → §1 row 3, §2.4, §11 S10: `LSP_FAILURE = 2`, stated in one place, with the `check`/`fmt` precedent. |
| 10 | MAJOR | **accepted** → §9.2, §9.3, §10: `bind` and `@variable.parameter` are deleted; `$?` stays inside `raw_string`. |
| 11 | MAJOR | **accepted** → §8.3: `Command::env`/`Worktree::shell_env()` are the same pair type in 0.7.0, with the one-expression fix if a pinned build differs; the guard asserts a single `shell_env()` call. |
| 12 | MAJOR | **accepted** → §11 S6, §12.1: the oracle is the `-32700` reply plus the exit, not `VmHWM`; the allocation bound is the code path of §3.2 step 3 plus the literal constant. |
| 13 | MAJOR | **accepted** → §1: `parse_lsp`, `lsp::run`, `write_message` and `check` now carry exact signatures. |
| 14–28 | MINOR | **accepted** → the `RawIngress` sentence now says "through" (§5.4); §11.2 says 89; `query` is `@keyword` (§10.2); `config.toml` is eight keys (§8.4, R38, §12.4); the `include_str!` path is `../../../../docs/reference/blastfile.md` from `crates/blastit/src/lsp.rs` (§7.2); §4.3's second branch drops the `-` example; `x$y` yields `$y` → `null` (§7.1); P4's fixture is `"# x\n"` × 4 194 304 + `"a"` (§4.5); `critical_blastfile_body.rs` is named (§5.5); the design says `_terminator` throughout (§9.2); clippy enforces the denies (§3.3, R11); the envelope/`uri` headroom is stated (§3.1); `outline.scm`'s quoted `@name` is accepted and documented (§10.3); equality wording is gone with `model.rs`. |

### Testability (`review-testability.md`: 3 BLOCKER, 18 MAJOR, 12 MINOR)

| # | Grade | Resolution |
|---|---|---|
| B1 | BLOCKER | **accepted** → §11 S2, §12.1: the test is `a_document_is_never_executed` — `LiteralQuery` **exactly**, empty stderr, and `/proc/<server_pid>/task/<tid>/children` empty before and after. |
| B2 | BLOCKER | **accepted** → §7.5 H5: the discriminating fixture is `"😀 view"` at `character` 6 (a `character as usize` cast answers `null`); `character` 20 keeps the punctuation case. |
| B3 | BLOCKER | **hand-off** → §12.8: `MB.22.3`'s smoke command asserts stdout (`positionEncoding`), because `test $? -eq 1` passes with the arm deleted. |
| M1 | MAJOR | **accepted** → §12.1: `{`/invalid UTF-8 → `-32700`, `[]` → `-32600`. |
| M2 | MAJOR | **accepted** → §6.3 F5(d): `(5,0)`. |
| M3 | MAJOR | **accepted** → §6.2: a fifth row `"a\n😀b"` → `(1,3)` is the UTF-16 oracle. |
| M4 | MAJOR | **accepted** → §12.1: the writer's byte length is asserted through a non-ASCII `uri` (`Blåstfile`). |
| M5 | MAJOR | **accepted** → §12.1: `error_codes_are_the_table` covers `-32600`, `-32601`, `-32602`, `-32002`; `work_done_progress_is_answered` and `unknown_notification_is_dropped` cover the tolerated table. |
| M6 | MAJOR | **accepted** → §12.1: `open_documents_are_capped`. |
| M8 | MAJOR | **accepted** → §5.7: the fixture is 600 `GrantOnUnknownProxy` views (only `validate` accumulates), with the exact 512/89 assertions |
| M9 | MAJOR | **accepted** → §5.5 (same fix as implementability 5). |
| M10 | MAJOR | **accepted** → §4.2, §5.3, §12.8: the `Location` family is listed and lands at `(0,0)`; the "validation error's site" acceptance is corrected by hand-off. |
| M11 | MAJOR | **accepted** → §10.4: (b)–(d) are three `grep` commands; `MB.22.4` gets them by hand-off (§12.8). |
| M12 | MAJOR | **accepted** → §9.5: the fixture loop uses `|| true` and covers every fixture. |
| M13 | MAJOR | **accepted** → §12.4: `zed-check` is a `check:` prerequisite (hand-off asserts the `Makefile` line) and the negative cases are the twelve named. |
| M14 | MAJOR | **accepted** → §12.3: the `||` fallback is removed and the reason is written out; `MB.22.5`'s acceptance change is a hand-off. |
| M15 | MAJOR | **accepted** → §6.5, §12.4 (same fix as implementability 8). |
| M16 | MAJOR | **accepted** → §11 S6 (same fix as implementability 12). |
| M17 | MAJOR | **accepted** → §2.2, §7.4, §12.1: `textDocument/definition` → `null`, not advertised, asserted. |
| M18 | MAJOR | **accepted** → §12.5: one document, three texts, and a `formatting` request on the broken buffer, so `null`, `[]` and one edit are all exercised. |
| m1–m12 | MINOR | **accepted** → P1's label says what it proves and P2 is the unit oracle (§4.5); P4's size is exact (§4.5); `EntrypointNotReached`'s backticks are restored (§5.4); the "wrong rule" column is relabelled (§6.2); `MAX_FRAME_BODY_BYTES == 33_619_968` is a named unit assertion (§12.1, §12.2); `ids_echo_verbatim` includes the `null` id (§12.1); `exit_codes` includes `shutdown`-then-EOF → `0` (§12.1); H6's bounds are a named unit test (§7.5, §12.1); hover *wording* left the protected file (§7.5); H1 asserts `every statement` (§7.5); the Python route's dbproxy precondition is stated (§12.5); R59's tier says "human-only, recorded in `MB.22.5`" (Requirement index). |

### Consistency (`review-consistency.md`: 0 BLOCKER, 9 MAJOR, 16 MINOR)

| # | Grade | Resolution |
|---|---|---|
| M1 | MAJOR | **accepted** → §9.2 (same fix as implementability 3). |
| M2 | MAJOR | **accepted** → §9.2: `query` takes `"…"` or `'…'`; `table_grant` takes a table list; `regex_raw_string` is reachable from `_pattern`; `bind` is deleted; `mode` is exactly three words. |
| M3 | MAJOR | **accepted** → §6.2: `(4,0)`, and the JSON example's `end.line` is 4. |
| M4 | MAJOR | **accepted** → §6.3 F5(d): `(5,0)`. |
| M5 | MAJOR | **accepted** → §12.1, §2.4: `[]` → `-32600`; the `-32700` row names row 3 and row 4's not-JSON half. |
| M6 | MAJOR | **accepted** → §11.2: `89 more problems`. |
| M7 | MAJOR | **accepted** → §0: the law now names three printers (check for errors, the boot for warnings, `fmt` for layout) and cites `check.rs:136`. |
| M8 | MAJOR | **accepted** → §10.1, §12.4, R48: fourteen captures in the intersection plus one exception, with a fifteen-name allowlist. |
| M9 | MAJOR | **accepted** → §6.5, §12.4 (same fix as implementability 8). |
| m1–m16 | MINOR | **accepted** → the `Content-Length` wording (§3.1); §1 row 3's exit reference (§2.4); the "no longer a peer" sentence (§3.4); `unrouted_request` (§5.3); the restored backticks (§5.4); eight keys (§8.4, R38, §12.4); four generated files, `parser.h` included (R40, §12.4); §10.1's summary cells (§10.1); `include_str!`'s path (§7.2); `sslmode`'s `$IDENT` and no "cloud metadata" (§7.3); `db`'s outline sentence (§10.3); nine commands (R55); the ADR's `§0–§12` and its consequences (ADR); `wasm32-wasip2` (§8.5); `MAX_FRAME_*` renaming (§3.1); the research's stale 1 MiB note (intro). |

### Round 3, and the independent fix round

Round 3 was capped, and two of its writes were lost; the verdicts this document first
recorded for `implementability` and `consistency` were written by the designer and **do
not count**. Two independent lenses then reviewed the revision and both returned
`FAIL` — one BLOCKER and six MAJORs (implementability), seven MAJORs (consistency) — and
their files are the state of record. Every one of those items is fixed in this document:

| Finding | Fix |
|---|---|
| the grammar had no `.` token (`functions pg_catalog.unaccent` a red node) | §9.2 (now verbatim from the shipped `grammar.js`) has `dotted_name: seq($.name, repeat1(seq('.', $.name)))` in `_value`, §9.5 has `qualified-function.txt`, §10.2 paints `(dotted_name) @property` |
| H1's `unrestricted` "(10 units)" | §7.5 says **12** |
| `diagnostics_are_capped` defined twice | one row, on the `GrantOnUnknownProxy` fixture (§12.1, §5.7) |
| `R53–R59` cited but never stated | §12.9 states all seven against named commands |
| the no-`id` rule contradicted the `-32600` row | §2.2 states one rule: no `id` **and** a `method` is a notification; an id-less invalid request is `-32600` with `id: null` |
| zero/>1 `contentChanges`: silent vs visible | §2.2 item 4, §2.5 and the §12.8 row all say **dropped silently** |
| the regeneration gate needed a lockfile the tree omits, on a host with no `npm` | §9.1/§9.4 use the pinned **release binary**, verified by `--version`; no `npm`, `npx`, `node_modules` or lockfile; R46 and §12.8 follow |
| §12.5's "three documents … three outcomes" | one document, three texts, with a `formatting` → `null` request on the broken buffer (§12.5) |
| the page's `uv run blastit lsp` vs §8.3's `--no-sync` | §8.3 distinguishes the human form from the editor form and says why; §12.8 hands the page row to a follow-up docs node |
| exit `1` for two causes the page does not list | §2.4 keeps one rule; §12.8's page row lists both causes and the `uv` form |

The two reviews' remaining MINORs are answered below: the ones that were one-line
corrections while a section was open (the missing §2.3 heading, "four deliberate
differences", the orphaned clause, "six variants across seven warnings", the three-`..`
path, the ADR's four generated files, the README row quoting the ADR's H1, the
`Location` parenthetical split, "six sessions", the method regex, the `table_grant`
colour as deliberate looseness, `Cargo.toml`'s block, §4.4's surrogate sentence) are
applied; the rest are listed below as accepted or deferred.
## Closing note

`status: In review` is the correct frontmatter state: the design needs the lead's
acceptance, and the two lenses that returned `FAIL` in round 3 are re-run once against
this revision (`review-implementability.md`, `review-consistency.md`). The `security` and
`testability` files stand as written. Nothing in this document is committed by the
designer, and no plan status was touched.

Applied in this round's one-line passes: §2.3's heading, "four deliberate differences",
the orphaned clause, "six variants across seven warnings", the three-`..` path, the ADR's
four generated files and its README row quoting the ADR H1, §5.3's `Location`
parenthetical, "six sessions", the method regex (`[A-Z][A-Z0-9_-]*`), §11.1's grep-list
attribution, the dropped `egress any` sentence, the §10 title, and §4.4's surrogate
wording.
