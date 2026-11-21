# ADR-0013: `blastit lsp` and the Zed Blastfile extension

- **Status:** Accepted; amended 2026-11-18 by `MB.34.2` (decision 4's `QuerySite`; that
  amendment is **Accepted**, the node is done) and again 2026-11-18 by `MB.35.3` (decision 1's
  single artifact, and decision 6's resolution order; that amendment stays `Proposed` until its
  node is accepted), and the text above stays as the record. Decision 4 said a warning's position is "computed by one
  CST walk … not by a field on `ManifestWarning` and not by a span field on `Manifest`"; a
  `query`'s inner offset is the one place where that recomputation is impossible, so the
  model carries a site (`QuerySite`, `blastfile-dsl.md` §13.4.5) — a named exception to that
  clause, not to the warning rule.
- **Date:** 2026-11-17
- **Node:** MB.22
- **Owner:** designer
- **Analysis:** `docs/design/blastfile-lsp-zed.md` (the full design),
  `docs/local.md` `## Editors` + `docs/reference/cli.md` `## blastit lsp`
  (the accepted DX, MB.22.1), `docs/research/2026-11-16-zed-blastfile-lsp.md`
  (every protocol and extension fact).

## Context

A `Blastfile` is the one file every Blastit developer edits by hand, and until
0.2.x the editor showed it as plain text: a mistyped directive was found by
running `blastit check`, not by looking at the line. The accepted pages promise
the opposite — a squiggle in the words the CLI prints, at the same line and
column, canonical layout on save after one JSON line, and a hover that says what
a grant actually allows — for the extensionless `./Blastfile`, with no `npm`,
Tree-sitter CLI, `cargo` or `wasi-sdk` on the user's machine.

That promise needs three things: a language server, a grammar, and a Zed
extension to wire them up. The constraints are Blastit's: zero new
dependencies, MSRV 1.88, `CARGO_NET_OFFLINE`, one shipped artifact per
installation, and a manifest crate that already owns the one parse, the one
formatter and the one error vocabulary.

## Decision

1. **`blastit lsp` is a subcommand of the existing binary**, ~700 lines in
   `crates/blastit/src/lsp.rs`, hand-rolled `serde_json` + `std::io`, blocking,
   single-threaded. No async runtime on that path and no LSP crate. `serde_json`
   is already a dependency of `crates/blastit`, so the dependency count does not
   move. It reaches both installations through `cli::main` — the Python-free
   standalone binary and the wheel's console script — with no extra artifact,
   name or release step.
2. **The subset is the one Zed needs, and nothing more**: `initialize`/
   `initialized`, full sync (`change: 1`), `didOpen`/`didChange`/`didClose`,
   push `publishDiagnostics`, whole-document `textDocument/formatting`,
   `textDocument/hover`, `shutdown`/`exit`. `definition`, `documentSymbol`,
   completion, pull diagnostics and incremental sync are out of v1.
3. **Positions are converted, never assumed**: the manifest's `Position` is
   1-based in `char`s, LSP's is 0-based in **UTF-16 code units**, and Zed
   ignores whatever the server declares. The conversion is
   `sum(utf16_len(c) for c in line[:col-1])`, computed in one function over a
   line-offset table built once per publish.
4. **The editor never gets a second opinion.** A diagnostic's `message` is
   `ManifestError::message()` — the CLI's sentence without the location prefix —
   and a formatting reply is `blastfile::fmt`'s bytes, so the editor and the CLI
   can never disagree. Warnings reuse their existing `Display` text, and their
   positions are computed by one CST walk (`blastfile::sites`) that
   `blastfile::check` returns beside the warnings — not by a field on
   `ManifestWarning` and not by a span field on `Manifest`, because both are
   constructed in protected tests and in `model.rs`, which this node does not own.
5. **One extension and one grammar, both in this repository**:
   `editors/zed/` (with an empty `[workspace]` so the root workspace and lockfile
   stay untouched) and `editors/tree-sitter-blastfile/` (with generated
   `parser.c`, `grammar.json`, `node-types.json` and `src/tree_sitter/parser.h` committed, because Zed never
   runs `tree-sitter generate`). `[grammars.blastfile]` pins a 40-hex commit and
   `path = "editors/tree-sitter-blastfile"`.
6. **The extension resolves the server without executing repository code**:
   `Worktree::which("blastit")` then `Worktree::which("uv")` with
   `run --frozen --no-sync --quiet blastit lsp`, and `Worktree::shell_env()`
   handed to the child, because a GUI-launched Zed does not inherit a terminal
   `PATH`. `--no-sync` is not a detail: a bare `uv run` syncs the project, which
   runs the worktree's declared build backend with the developer's environment in
   `environ` — opening a cloned repository's `Blastfile` would execute that
   repository's build code. No `process:exec`, no capability prompt.
7. **Format-on-save is one documented line in `.zed/settings.json`**, committed
   in this repository, because `format_on_save` defaults to `off` and an
   extension cannot set a per-language default. CRLF buffers are normalised to
   LF on save — the formatter already writes LF and `blastit fmt` writes the same
   bytes.

## Consequences

- One release artifact, one name for the extension to find, one place to fix a
  protocol bug; the standalone binary and the wheel prove the same session.
- ~700 lines of protocol code and ~90 lines of grammar/extension wiring are
  ours to test, with no third-party audit surface and no async runtime to leak
  tasks.
- The manifest crate gains exactly one visibility change (`MAX_SOURCE_BYTES`), one
  new method (`ManifestError::message()`), one new module (`blastfile::sites`,
  ~120 lines) and one new entry point (`blastfile::check`); `model.rs` and
  `validate.rs` are untouched, so every protected test compiles unchanged.
- The binary's `Lsp` arm drops every file capability before it reads a byte, so a
  `setcap`'d deployment binary is still a safe editor server.
- A second editor (Helix, Neovim) costs a client config, not a second server;
  a VS Code client would be a later `editors/vscode` node.
- `definition` and semantic context inside strings wait for
  `blastfile::symbols()`, a bounded CST walk that is a follow-up node, not a
  rewrite.
- The grammar is a colourist, deliberately looser than the parser: it must never
  paint a red node on a file `blastit` accepts, and the LSP owns every red.

## Alternatives rejected

| Alternative | Why not |
|---|---|
| **`tower-lsp` (+ `tokio`)** | ~40 transitive crates and an async runtime for a single-threaded, strictly sequential protocol whose total traffic is one request at a time. It would also put the LSP on `tokio` while the rest of the binary's blocking model stays put, and it enlarges the offline/MSRV/audit surface for no capability we lack |
| **`lsp-server` + `lsp-types`** | Two crates whose value is re-deriving message shapes we need eight of; `lsp-types` would also become the second definition of `Position` next to the manifest's, which is exactly the conversion error this design exists to prevent |
| **A separate `blastit-lsp` binary or crate** | A second artifact to build, sign, publish and keep in step; a second name for the extension to resolve and for the docs to explain; and it would still link `blastit-manifest`, so the only thing it buys is distance from the code it serves. The subcommand also keeps the Python-free standalone promise for free |
| **A separate `tree-sitter-blastfile` repository** | Two rev pins to keep in sync (extension ↔ grammar), two release ceremonies, and a grammar that drifts from the parser it exists to colour. One repository with `path =` gives one commit that contains both halves, which is the pin `MB.22.5` proves |
| A grammar-only extension (no Rust) | Zed would colour the file and nothing else — no diagnostics, no formatting, no hover — contradicting the accepted pages |
| A shell-out from the extension to probe `blastit --version` | Gated by `process:exec` **and** a user-granted capability, turning a zero-config install into a permission prompt |

## References

- `docs/design/blastfile-lsp-zed.md` §0–§12 (the design this records).
- `docs/local.md` `## Editors`, `docs/reference/cli.md` `## blastit lsp` (MB.22.1).
- `docs/design/blastfile-dsl.md` (R2 `line:col` in `char`s, R6 `fmt` idempotence)
  and `ADR-0012` (the DSL).
- `crates/blastit/src/{lsp,cli}.rs`, `crates/blastit-manifest/src/error.rs`,
  `crates/blastit-manifest/src/blastfile/{mod,sites}.rs`, `editors/zed/**`,
  `editors/tree-sitter-blastfile/**` (`MB.22.3`–`MB.22.5`).

## Amendment 2026-11-18 (`MB.34.2`): spans inside a `query`, one protocol method, and the SQL grammar boundary

**Status:** Accepted (`MB.34.2`, done). **Node:** MB.34.2.
**Design:** `docs/design/blastfile-lsp-zed.md` §0.2, §4.6, §7.6, §7A, §9.2a, §10.2, §13;
`docs/design/blastfile-dsl.md` §13.4–§13.5, §18.1. **Accepted DX:** `docs/reference/blastfile.md`
`query` section, `docs/local.md` `## Editors`, `docs/ci.md`, `docs/troubleshooting.md`
(MB.34.1).

The accepted pages gained one promise the 2026-11-17 decision did not cover: a `query` is
validated as the PostgreSQL statement it claims to be, the diagnostic sits on the SQL token
*inside* the string, a Quick Fix turns a pasted statement into `$?` form, hover explains
`$?`/`...`, and the editor colours the SQL. Three additions follow; **decisions 1–3 and
5–7 above stand unchanged**, and so does every refusal they record.

1. **A `query` refusal now carries a site, and the site is stored, not re-derived.** The
   validator answers with an offset into the **raw decoded** statement, and
   `blastfile::lower` records, per deduped query entry, the origin `Position` (the first
   character *inside* the opening quote), the interior's source byte range, and the
   decoded→source map for the `"…"` spelling's four escapes (`QuerySite`,
   `blastfile-dsl.md` §13.4.5). This is the exception decision 4's predecessor clause
   anticipated: `sites.rs` computes a warning's position from the CST on every call, which
   works because a warning names a *token*; a statement's offset cannot be recomputed
   without the source spelling, so the model carries the site once, beside the text it
   describes. `RedactedQueryPlaceholder` and `LiteralQuery` gain the same `at`, and
   `prefix(Position::NONE) == ""` keeps every programmatic-grant rendering byte-identical
   (`critical_manifest_011.rs` compiles with one added field and no changed assertion).
2. **Exactly one protocol method is added: `textDocument/codeAction`.** `codeActionProvider:
   true` (boolean, no `codeActionKinds`), one `quickfix` — `Parameterize SQL for Blastfile
   ($?)` — whose single `TextEdit` replaces the whole string interior, `context.diagnostics`
   never trusted, no `workspace/applyEdit`, no `codeAction/resolve`, no new error code. Hover
   grows two tokens (`$?`, `...`) inside a `query` only. Everything else stays a non-goal:
   `definition`, `documentSymbol`, completion, pull diagnostics, incremental sync,
   semantic tokens, `injections.scm`.
3. **The grammar gains one node and one rule of provenance.** `sql_string` colours the
   interior of a `query`'s `'…'` value with seven child node types, ordinary rules only —
   no external C scanner, no second grammar, no second Zed extension, and
   `editors/zed/languages/blastfile/highlights.scm` is its only consumer. Two mechanisms are
   part of the decision because the alternatives are silent breakage: `query` leaves the
   shared `keyword` rule (with it in, the longest-match lexer always prefers the one-token
   `raw_string` and `sql_string` is unreachable), and every interior token's lexical
   precedence is above the extras' (or a `#` in the literal is lexed as a `#…` comment and
   eats the closing quote). The node's language is exactly `raw_string`'s `'[^']*'`, so the
   colourist stays strictly looser than the parser.

**Consequences.** One new public shape (`TemplateError`, `QuerySite`, `query_sites()`, two
small `blastit-sqlpolicy` functions), one new error variant, one new RPC, nine grammar
rules (`query_decl`, `sql_string` and its seven child node types) and four highlight rules
with no new capture name. `blastit-manifest` still does not depend on `pg_query`
(`crates/blastit-manifest/Cargo.toml` stays empty of it); the parse lives in
`blastit-sqlpolicy`, which `crates/blastit` already depends on, so the editor adds no
dependency either. (The server today lives in `crates/blastit/src/lsp/{mod,hover}.rs`;
decision 1's `lsp.rs` is the 2026-11-17 plan that `MB.22.3` split — the accepted text stays
as the record.) `pg_query 6.2.1` does **not** expose `cursorpos` (the C struct has it at
`libpg_query/pg_query.h:15`; the Rust wrapper reads only `message`,
`src/query.rs:35`), so a `Syntax` refusal's offset is pinned to the **first non-trivia code
character** (spaces, newlines and comments skipped) — precise for the leading-token case, honest elsewhere, and re-pointable without a
contract change if the crate ever exposes the field. The message is never rendered: it quotes the offending lexeme, which can be a
value (security S1).

**Alternatives rejected.** *Deriving the inner `line:col` in `lsp.rs` from the document text
and a second scan* — a second reader of the string spellings, which is what decision 4
forbids. *A `workspace/applyEdit`/server-side apply* — a second authority over the buffer for
no capability Zed lacks. *An external scanner or an `injections.scm` over a bundled SQL
grammar* — a C build step or a second grammar repository to keep in step, for colour only.
*Refusing `#`-bearing or two-statement `query` text in the grammar* — the grammar must never
be stricter than the parser; the LSP owns the red.

**References.** `crates/blastit-sqlpolicy/src/template.rs`,
`crates/blastit-manifest/src/{blastfile/lower,validate,error,model}.rs`,
`crates/blastit/src/lsp/{mod,hover}.rs`, `editors/tree-sitter-blastfile/grammar.js`,
`editors/zed/languages/blastfile/highlights.scm` (`MB.34.3`–`MB.34.6`).

## Amendment 2026-11-18 (`MB.35.3`): a portable `blastit-lsp` crate, and `blastit` stops being the server's only home

**Status:** Proposed (accepted with `MB.35.3`'s review; the four lenses' verdicts are in
`plan/nodes/MB/MB.35/MB.35.3/RESULT.md`). **Node:** MB.35.3 (epic `MB.35`).
**Design:** `docs/design/blastfile-lsp-zed.md` §1, §1.3, §8.3a, §14.
**Research:** `docs/research/2026-11-17-cross-platform-blastit-lsp.md` (`VERDICT: GO`).
**Accepted DX:** `docs/reference/cli.md` `## blastit-lsp`, `docs/local.md` `## Editors`,
`editors/zed/README.md` (`MB.35.2`).

**The premise moved, so one recorded decision is reversed — not reinterpreted.** The
2026-11-17 decision was made on the premise that the editor exists only where Blastit itself
runs: a Linux machine that already has `blastit` (or `uv add blastit`) installed. The owner's
`0.2.0` mandate removes that premise:

> *"it'd be nice if there were osx and windows releases for the plugin and blastit binary (even
> if they ONLY embed the LSP server and not the actual blastit mechanics, which only work in
> Linux...). Or maybe the LSP server is another different blastit-lsp binary which gets
> published on all platforms while the main blastit binary is just available for Linux"*

**Sentences superseded in the accepted text:** `## Consequences` :84 (two artifacts; `blastit-lsp` resolves first) and :94 (a `setcap`/`sudo` invocation is unsupported; `getcap` is asserted empty on every Linux asset).

`blastit`'s serving mechanics stay Linux-only for reasons no packaging change touches: per-route
uids, `setresuid`/capabilities, `seccomp`, `openat2`, `SCM_CREDENTIALS`, `fanotify`, and the
host's NTS `libphp` (`ARCHITECTURE.md` §2.7, `ADR-0010` §4). The **editor**, however, is a text
tool: it reads the developer's own buffer on stdin. Decisions 2, 3, 4, 5 and 7 above stand
unchanged, and so does every refusal and every assertion they record.

### The decision

1. **`crates/blastit-lsp` is a crate of its own**, with `[[bin]] name = "blastit-lsp"` and
   `[lib] name = "blastit_lsp"`; the public surface is `blastit_lsp::run() -> u8`, unchanged in
   signature from today's `lsp::run()`. `main.rs` is `#![forbid(unsafe_code)]` plus one
   `fn main() { std::process::exit(blastit_lsp::run() as i32) }` — and the module's
   `#![deny(...)]` header, panic hook and
   `catch_unwind` move with it byte for byte, plus `#![forbid(unsafe_code)]` on both the library
   and the binary: an `extern "C"` declaration needs `unsafe`, so the attribute is what keeps the
   crate's "no privilege crate" argument from being bypassed by one declaration.
2. **Its dependency set is exactly `blastit-manifest` and `serde_json`**, and nothing else.
   `blastit-sys`, `rustix`, `nix`, `libc`, `pyo3`, `blastit-php-sapi` and `std::os::unix` are
   forbidden on its source and in its normal dependency closure; `MB.35.6`'s
   `scripts/check_lsp_portable.py` is the check (`make lsp-portable`, inside `make check`), and
   `cargo tree -p blastit-lsp --edges normal` is its evidence. `pg_query 6.2.1` still arrives
   through `blastit-manifest` → `blastit-sqlpolicy`; its `cc`/`bindgen` are **build** deps, which
   is why the five targets need a C toolchain and `libclang` and nothing runtime.
   One line of `blastit-manifest` makes the two-dependency rule true rather than aspirational:
   the Quick Fix's rewrite (`blastit_sqlpolicy::parameterize`, decision 2 of the `MB.34.2`
   amendment) is **re-exported** by `blastit-manifest` beside the `TableGrant`/`Columns` it
   already re-exports (`crates/blastit-manifest/src/lib.rs:108`), on the same argument as
   `IngressPolicy` there: a caller that asks the manifest for a `query`'s canonical `$?` text
   should not have to name a second crate. `MB.35.4` owns that one line, and its `paths` must name
   `crates/blastit-manifest/src/lib.rs` — the plan's list does not yet, and this amendment records
   the correction rather than leaving the leaf to choose between a third dependency and a
   lane violation.
3. **`blastit lsp` stays, as one delegation.** `crates/blastit/src/lib.rs` replaces `pub mod lsp;`
   with `pub use blastit_lsp as lsp;`, so `cli.rs`'s arm — including
   `blastit_sys::caps::hold_none()` before the call — compiles untouched and Linux keeps the one
   server it documents. There is **one** `run()` behind both binaries; the *assertion bodies* of
   the protected `crates/blastit/tests/critical_lsp.rs` stay unchanged against `blastit lsp` —
   **four** `include_str!` path literals follow the move (two source paths in the test, two doc
   paths in `hover.rs`), because that code reads repository paths at compile time — and the new
   `crates/blastit-lsp/tests/critical_lsp_stdio.rs` replays the minimum session against
   `blastit-lsp`. The identity of the two binaries is checked structurally (no
   `crates/blastit/src/lsp/` directory, `pub use blastit_lsp as lsp;` present, one `blastit-lsp`
   edge in `crates/blastit/Cargo.toml`), because a copy of the server would pass a behavioural
   test. Nothing in the protocol changes: no message, no sentence, no bound.
4. **The asset contract** (`MB.35.5` builds it, `MB.33` codes against it):
   `blastit-lsp-<target>.tar.gz` on Linux and macOS, `blastit-lsp-<target>.zip` on Windows, one
   archive per target, each holding one top-level directory `blastit-lsp-<target>/` whose single
   executable is `blastit-lsp` (`blastit-lsp.exe` on Windows), beside `LICENSE` and a
   `SHA256SUMS` over those files — the same staging shape `scripts/build-standalone.sh` uses,
   minus that script's `README.md` (the LSP has no page of its own; `docs/reference/cli.md`
   `## blastit-lsp` is it). The five targets and the exact names are in design §8.3a; the counts
   and the runner per leg are declared once, by `MB.35.5`, in a new `lsp` family in
   `scripts/platform_matrix.py`, together with its `--matrix`/`--summary`/`--check` branches and
   the clause noun the count gate reads, so R74's proof is a real gate and not a green line about
   other families.
5. **Decision 6's resolution order is replaced** (design §8.3a, implemented by `MB.33`):
   `LspSettings` → `which("blastit-lsp")` / `which("blastit") lsp` → one GitHub Release download
   by `current_platform()`. `uv` leaves the editor path entirely: the owner's *"is it gonna get
   the LSP from uv? because that's super wonky? especially the package isn't published yet"* is
   the defect, and the `--frozen --no-sync` argv was a patch over a hop that should not exist.
   Nothing about decision 6's safety finding is retracted — a bare `uv run` in an editor would
   still execute a cloned repository's build backend, which is exactly why the step is gone
   rather than merely re-worded. Two limits keep the new order from reopening that finding:
   `binary.env` is **not** honoured (the server reads no environment, so the field has no
   purpose and is the one loader-hijack vector a settings file could add), and the child receives
   an allowlist — `PATH`, plus `SYSTEMROOT` on Windows — instead of the whole shell environment.
   What the extension cannot filter is the *layer*: `for_worktree` returns merged settings and the
   API cannot say which layer a key came from, and no research fact establishes that Zed withholds
   `lsp` keys from project settings. The design therefore does not rely on that: if a repository's
   `.zed/settings.json` can name a binary or its arguments, opening a clone can run it — Zed's own
   trust model for `lsp` settings, the same file that already chooses a formatter or a task — and
   design §8.3a states it plainly rather than implying the clone is harmless.
6. **The four leaf briefs are corrected by the design, not by this ADR.** `MB.35.4`'s `paths`
   must name `crates/blastit-manifest/src/lib.rs` and `crates/blastit/tests/critical_lsp.rs`, and
   its "byte-identical" sentence becomes "assertion bodies unchanged, four `include_str!` literals
   moved"; `MB.35.5` gains the archive-extraction assertion, the `manylinux_2_28` floor and the
   `lsp` matrix family with its own count noun; `MB.35.6`'s token list, negative twins and evidence
   states are design §1.3/§14.2's; `MB.33` declares **no** `[[capabilities]]` block, uses a
   relative download destination, calls `make_file_executable` unconditionally and ignores
   `binary.env`. Design §14.4 is the table; this node owns no plan file, so the lead applies it.

### What the old comparison table's costs became

| Old row (`§1.1` of the design) | Was a cost because | Now |
|---|---|---|
| "a second artifact to build, sign, publish and keep in step" | the editor ran only on Linux, so the artifact bought nothing | **the artifact is the product.** A Mac or Windows developer has no `blastit` to speak for them; the five legs join the release job that already uploads `dist/*` and publishes checksums, and the count is one family in `scripts/platform_matrix.py`. What is left is CI time, not a second thing to explain |
| "a second name for the extension to resolve and for the docs to explain" | `blastit` was already on the PATH of every user who had the editor | **the second name is the first name.** `blastit-lsp` wins by settings or by `PATH`, and `blastit lsp` is the Linux fallback — so a Linux user's accepted page is still true, and the name the docs explain is one |
| "it would still link `blastit-manifest`, so the only thing it buys is distance from the code it serves" | distance from the code was worth nothing | **distance is the whole point**: the crate's closure is portable by construction, which a crate that also links `blastit-sys` can never be, and on macOS/Windows there is no `blastit` to be close to |
| "the subcommand also keeps the Python-free standalone promise for free" | it did | `blastit` keeps it; `blastit-lsp` has **no Python in its graph at all** — a stronger statement than "no Python linked", and `MB.35.6` fails the build if that changes |
| privilege surface: "the same call in a second binary — splitting the binary buys nothing here" | capabilities belong to the process image | **the portable artifact has nothing in it that drops or uses a privilege, and it is supported unprivileged only.** No crate in its graph wraps `capset`, `setresuid`, `seccomp` or `execve`, and `#![forbid(unsafe_code)]` keeps an `extern "C"` declaration from adding one; `blastit lsp` keeps `hold_none()` exactly as before. What the design does **not** claim: that a `sudo`/`setcap` invocation is inert. It is unsupported — `MB.35.5` asserts `getcap` is empty for every Linux asset and no page installs it that way — and the linked `blastit-manifest` file API and the `libpg_query` C parser are in the image, which is why the claim is "the server's own code path opens no path, no socket and no child", not "the image cannot" |

### The platform verdict (`MB.35.1`), and what a missing target loses

| Target | Evidence | CI leg (`MB.35.5`) |
|---|---|---|
| `x86_64-pc-windows-msvc` | **proven upstream**: `pg_query.rs`'s own CI ran `cargo build` **and** `cargo test` green on `windows-latest` at the `6.2.1` tag, no `LIBCLANG_PATH` set; this epic pins `LIBCLANG_PATH=C:\Program Files\LLVM\bin` for determinism | `windows-latest`, build + `cargo test -p blastit-lsp` + smoke transcript |
| `x86_64-unknown-linux-gnu` | **proven** by the matrix (`MB.3.18`) | built in `quay.io/pypa/manylinux_2_28_x86_64` with a measured `--max-glibc 2_28` floor, then smoke-tested on Debian 12 (glibc 2.36) — the same rule the serving archives follow, because a binary built on `ubuntu-24.04` carries `GLIBC_2.39` and refuses to start on a Debian 12 workstation |
| `aarch64-unknown-linux-gnu` | **declared, unbuilt** — accepts the target, never calls it proved | `quay.io/pypa/manylinux_2_28_aarch64` (native arm runner), build + `cargo test -p blastit-lsp` + same floor and smoke |
| `aarch64-apple-darwin` | **buildable, unproven for the Rust wrapper** — the C library builds on `macos-latest` (arm64); `pg_query.rs` has no macOS job | `macos-latest`, build + `cargo test -p blastit-lsp` + smoke; `LIBCLANG_PATH="$(xcode-select -p)/usr/lib"` is the fallback if clang-sys's hardcoded paths miss |
| `x86_64-apple-darwin` | **buildable, unproven** — Tier 2 with host tools; `macos-15-intel` is the only `cargo test`-grade evidence (cross + Rosetta is not) | `macos-15-intel`, build + `cargo test -p blastit-lsp` + smoke; same `LIBCLANG_PATH` fallback |

No target is dropped today, and no page installs `blastit-lsp` with `setcap` or `sudo`. **If a leg
fails at `MB.35.5`**, it is dropped from the `lsp` family in `scripts/platform_matrix.py` with
`dropped_because`, and — in the same change set, which is a doc edit by whoever owns those files — from every place that states the targets: this
amendment's platform table, design §8.3a's asset table and §14.3's five bullets,
`docs/reference/cli.md` `## blastit-lsp`'s table, `docs/troubleshooting.md`'s "no asset for that
pair" row, and `ADR-0010` §4's row. What that
target loses is **the whole server** — no diagnostics, no formatting, no hover, no Quick Fix —
while the grammar still colours the file, because the grammar is wasm and independent of the
server. The extension then says "no `blastit-lsp` for `<os>/<arch>`" with the release URL.
**Never** a `#[cfg(target_os)]` stub that registers and validates nothing: a validator that
approves everything is worse than an absent one.

Targets this epic deliberately does not build: 32-bit x86, Windows on ARM, and `*-musl`. The first
two get neither Blastit nor its editor. `*-musl` is different and must not be described with them:
it is a **serving platform** — `musllinux_1_2` wheels and `blastit-*-unknown-linux-musl.tar.gz`
are in `ADR-0010` §4 and in the matrix — so it keeps `blastit`, `dev`, `check` and `fmt`, and keeps `blastit lsp` too — that arm is not
feature-gated — while it gains no separate `blastit-lsp` asset in 0.2.0.

### Consequences

- One `run()`, one protocol, one set of sentences: `blastit lsp` and `blastit-lsp` cannot drift,
  and the protected suites prove it from both sides — the delegation's assertions unchanged, and
  a structural check that no second `lsp` module exists under `crates/blastit/src/`.
- The portable artifact **cannot become a serving binary by accident**: `blastit`, `blastit-sys`,
  `blastit-dbproxy`, `blastit-scheduler`, `blastit-http`, `blastit-view`, `blastit-py` and
  `blastit-php-sapi` are not in its dependency graph, so there is no `bind`, no `fork`, no
  `setuid` and no `execve` to reach; `MB.35.6` also forbids `std::process`, `std::net`,
  `std::fs`, `std::path`, `include!`, `#[path` and unsafe items in every file under its `src/**`
  except the pinned `main.rs` (with
  `main.rs` pinned to its attribute plus one call) — the exhaustive token list and file set are
  design §1.3's third row — so it cannot spawn, listen or read its way there either. The
  remaining C in the image is `libpg_query`'s parser, which is also what Linux runs.
- **In the boot a `Blastfile` is policy; in the editor it is untrusted data** — a clone's
  manifest is written by someone else. The only parsers it reaches are `blastit-manifest`'s safe
  Rust and `libpg_query`'s parse-only C (the statement is never executed, no interpreter is
  linked), `pg_query` stays pinned at `6.2.1`, and `MB.35.6` records the normal closure; 0.2.0 has **no** advisory gate, which
  is stated rather than implied. `docs/design/blastfile-lsp-zed.md` §11 says so in its first paragraph.
- Five more CI legs, one more family in the matrix, and a release job that must grow with them:
  a third `download-artifact` with `pattern: lsp-*`, a `--count lsp` assertion, the archive glob
  narrowed (today's `blastit-*.tar.gz` also matches `blastit-lsp-*.tar.gz`), the LSP archives
  added to the final `SHA256SUMS`, and the per-leg `dist/SHA256SUMS.<target>` rename the
  standalone job already does.
- **Trust root stated, not implied.** The `SHA256SUMS` inside an archive is for a human who
  unpacked it; it authenticates nothing. The extension's only control is HTTPS to `github.com`
  — the release-level `SHA256SUMS` asset is for CI and for a reader who verifies by hand, and the
  wasm never fetches it — and 0.2.0 ships **no signature**, an honest gap recorded
  in `ADR-0013`'s old rejected-table word "sign", which this amendment does not pretend is met.
  macOS quarantine applies to a browser download, not to Zed's own fetch.
- `ADR-0010` §4 gains the `lsp` family row; `ARCHITECTURE.md` §2.7 gains the crate row and the
  sentence this amendment rests on: the editor surface is cross-platform, serving is Linux-only.

### Alternatives rejected

| Alternative | Why not |
|---|---|
| **Keep one binary and ship no macOS/Windows editor** (the 2026-11-17 decision, unchanged) | It is the premise the owner removed. A Mac or Windows developer then has no editor experience at all, and the accepted pages would have to retract the colours, the SQL diagnostics and the Quick Fix |
| **Cross-compile `blastit` itself for macOS and Windows** | It links `blastit-sys` (uids, seccomp, `fanotify`), `rustix`, `pyo3` and the PHP SAPI; there is no honest build, and a "portable" server that cannot drop a uid is exactly the artifact the zero-trust story forbids |
| **A second crate that re-implements the server for portability** | Two protocols, two sentence tables, two sets of bounds. The extraction is a `git mv`; the protected suite is the proof that nothing changed |
| **A `#[cfg(target_os)]` reduced server on an unsupported target** | Registers, answers `initialize`, validates nothing — a green squiggle on a broken manifest. The extension's one-line error is the honest ending |
| **Keep `uv run blastit lsp` as the editor's fallback** | The owner's report, and a safety defect: a bare `uv run` syncs the project, i.e. runs the clone's build backend with the developer's environment, on every editor start |
| **Declare a `download_file` capability for the extension's download** | The gate is a **user setting** (`host = "*"` by default) checked on the pre-redirect URL; the manifest is never consulted, so a block is inert and naming the CDN host would fail closed. `MB.33` declares none (research §3) |

### References

- `docs/research/2026-11-17-cross-platform-blastit-lsp.md` §1 (per-target evidence), §2 (Zed's
  `Zip`/`which`/`make_file_executable` facts), §3 (release mechanics, the capability gate), §4
  (the dependency closure).
- `docs/design/blastfile-lsp-zed.md` §1, §1.3 (the crate boundary and its guard), §8.3a (the
  three-step resolution and the five assets), §14 (R69–R76 and the test mapping).
- `docs/reference/cli.md` `## blastit-lsp`, `docs/local.md` `## Editors`, `editors/zed/README.md`
  (`MB.35.2`); `ADR-0010` §4; `ARCHITECTURE.md` §2.7.
- `crates/blastit/src/{cli,lsp/mod}.rs`, `crates/blastit/tests/critical_lsp.rs`,
  `crates/blastit-manifest/src/lib.rs`, `scripts/platform_matrix.py`,
  `scripts/build-standalone.sh` (`MB.35.4`–`MB.35.6`, `MB.33`).
