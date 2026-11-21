# Security review: `blastit lsp` and the Zed Blastfile extension

Lens: **security** (zero-trust, CVE resistance, bounds, secret/text leakage, fail-closed behaviour).
Documents read: `docs/design/blastfile-lsp-zed.md`, `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`.
Code sites checked: `crates/blastit/src/{cli,main,host,lib}.rs`, `crates/blastit-py/src/lib.rs`,
`crates/blastit-manifest/src/{error,model,uid}.rs`, `crates/blastit-manifest/src/blastfile/{mod,parser,print,lexer}.rs`,
`crates/blastit-sys/src/{caps,exec}.rs`, `crates/blastit/Cargo.toml`, `Cargo.toml`,
`docs/reference/cli.md`, `docs/local.md`, `docs/deploy/compose-traefik.md`, `tools/critical_guard.py`.

Settled decisions (not findings): hand-rolled `serde_json` + `std::io`, no new dependency, no async
runtime, no LSP crate, no separate binary, full sync (kind 1), in-repo grammar with committed
`parser.c`, hover in v1 with definition deferred, no PHP alternative transport.

Overall: the frame reader is genuinely bounded and the "stdout is protocol / stderr is three fixed
sentences" discipline is the right shape. Three things break it: the subcommand runs inside a
capability-carrying binary, the `uv` fallback executes repository code, and `didChange` can grow the
document map past `MAX_DOCUMENTS`. Eight more claims in §11 are phrased so that no test can assert
them, or name the wrong code site.

---

## BLOCKER

### B1 — §1.1 / §11 S3: `blastit lsp` runs with the file capabilities of the production binary

> §1.1: "Privilege surface | the arm returns before any privilege, uid, `/run` or `Host` code runs (§11)"
>
> §11 S3: "**No privilege, no configuration.** The `lsp` arm returns before `Host`, `stage0`, `stage1`, `config` run"

**Why it is live.** File capabilities belong to the process image, not to a code path. Blastit's own
deployment gives the binary `cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep`
(`docs/deploy/compose-traefik.md:157`; the same string is `blastit_sys::caps::SETCAP`). The extension
resolves `blastit` from `PATH` (§8.3), so on any machine where the deploy binary is installed,
`blastit lsp` parses editor-supplied JSON and `Blastfile` bytes **for the whole session** with
`CAP_DAC_OVERRIDE` and `CAP_SETUID` in its effective set. That inverts the project invariant that
untrusted bytes are parsed only by processes that hold no capabilities (stage 1, route workers), and
it is the exact argument §1.1 uses to prefer a subcommand over a second binary. "The arm returns
before privilege code runs" is true and irrelevant: nothing has to run for the capabilities to be
held.

**Replacement.** In `cli.rs`'s `Lsp` arm, *before* `lsp::run()`:

```rust
Command::Lsp => match parse_lsp(&argv[1..]) {
    Ok(Subcommand::Help) => { println!("{LSP_USAGE}"); SUCCESS }
    Ok(Subcommand::Run) => match blastit_sys::caps::hold_none() {
        Ok(()) => lsp::run(),
        Err(error) => { eprintln!("blastit lsp: {error}"); FAILURE }
    },
    Err(message) => { eprintln!("blastit lsp: {message}"); LSP_FAILURE }
},
```

`hold_none()` already exists and already refuses to serve when it cannot prove it holds nothing
(`crates/blastit-sys/src/caps.rs:623`). `lsp.rs` still names no `blastit_sys` (§1.2 holds).
Add to §11 S3: "`blastit lsp` holds no capability for the whole session: the `Lsp` arm calls
`caps::hold_none()` before it reads stdin."

**Assertion.** `critical_lsp.rs` greps `include_str!("../src/cli.rs")` for `hold_none()` occurring
before `lsp::run()` in the `Lsp` arm, and (in the rootless e2e image, where the capability can be
granted) spawns the binary under `capsh --addcap=cap_dac_override --` and asserts `CapEff` is
`0000000000000000` in the child's `/proc/<pid>/status` for the session.

### B2 — §8.3 / §11 S1 / ADR-0013 §6: the `uv` fallback executes the worktree's build backend

> ADR-0013 §6: "The extension resolves the server itself, through the worktree, **never by running
> anything**: `Worktree::which("blastit")` then `Worktree::which("uv")` (`uv run --quiet blastit lsp`)"
>
> §11 S1: "**No capability, no process, no network.**"

**Why it is live.** `uv run` without `--no-sync` resolves and **syncs** the project environment
before it runs the command, which builds the project with its declared build backend. So: clone an
untrusted repository that declares `blastit` (and any build backend), open it in Zed, open its
`Blastfile` — the extension spawns `uv run --quiet blastit lsp`, and the repository's build backend
runs with `worktree.shell_env()` in `environ`, i.e. with every token the developer exports
(`GITHUB_TOKEN`, `AWS_*`, `DATABASE_URL`). No build command was typed. That is a drive-by
code-execution class, and it contradicts the sentence "never by running anything" that the ADR uses
to reject the `process:exec` alternative.

**Replacement.** §8.3's argv becomes six literals:

```rust
vec!["run".into(), "--frozen".into(), "--no-sync".into(), "--quiet".into(), "blastit".into(), "lsp".into()]
```

and the `None` arm's error string gains "run `uv sync` once in this project". ADR-0013 §6 is reworded
to "resolves the server without executing repository code: `uv run --frozen --no-sync` reuses an
environment that already exists and never builds the project". If `--no-sync` is judged too strict,
the residual must be stated in §11 as a known exposure, not denied.

**Assertion.** `scripts/check_zed_extension.py` asserts the six argv literals in order, that
`--no-sync` is present, and that the error string contains `uv sync`.

### B3 — §2.2 / §2.5 / §3.1: `didChange` for an unknown `uri` bypasses `MAX_DOCUMENTS`

> §2.2: "`textDocument/didChange` | notification | `textDocument.{uri, version}`, `contentChanges` |
> requires **exactly one** change object carrying `text` and no `range`; replaces the whole text and
> the version, then publishes"
>
> §2.5: "`MAX_DOCUMENTS = 32` (§3.1): a **`didOpen`** that would open the 33rd document is not stored"

**Why it is live.** Nothing says what a `didChange` for a `uri` that is not in `documents` does. The
natural implementation — `documents.entry(uri).or_insert_with(...)` — inserts, and the cap is
enforced only on `didOpen`. A client (or a buggy editor) sending `didChange` with fresh uris then
grows the map without bound, at up to `MAX_BODY_BYTES` ≈ 32 MiB of text per entry: exactly the
"silent memory growth" §2.5 says the cap exists to prevent. A cheap implementer will write the
`entry()` form because the design never forbids it.

**Replacement.** Add to §2.2's row and to §2.5:

> A `didChange` whose `uri` is not in `documents` is dropped and publishes nothing. `documents` is
> inserted by `didOpen` only; `didChange` replaces.

**Assertion.** New `critical_lsp.rs` row `didChange_for_unknown_uri_does_not_grow_the_map`: send 40
`didChange` notifications with fresh uris, then 32 `didOpen`s; the 33rd `didOpen` still publishes
§2.5's notice (proving the map holds 32, not 72) and a `hover` for the last unknown uri answers `null`.

---

## MAJOR

### M1 — §11 S3: "no `BLASTIT_*` variable is read" is false at the boundary the design names

> §11 S3: "no `BLASTIT_*` variable is read; `/run`, `/proc/self`, uid checks untouched"

**Why it is live.** `Host` is constructed by the caller *before* `cli::main` is entered
(`crates/blastit/src/main.rs:14-19`), and `Host::binary()` reads `BLASTIT_EXEC`
(`crates/blastit/src/host.rs:178`, `EXEC` at `host.rs:56`). The wheel path is worse: `_blastit.main`
stats the dbproxy path and raises before dispatch (`crates/blastit-py/src/lib.rs:62`). So the
property as written is false and no behavioural test can assert it.

**Replacement.** Restate S3 as: "`lsp.rs` reads no environment, no path and no socket. The `Lsp` arm
runs before any privilege, listener, `/run` or config access. `Host::binary()` still reads
`BLASTIT_EXEC` and `_blastit.main` still stats the dbproxy path before dispatch; neither value is
used on the lsp path, and `lsp.rs` cannot reach them (§1.2)."

**Assertion.** Keep the §1.2 grep; add a session run with `BLASTIT_EXEC=/nonexistent` and
`BLASTIT_UID_BASE=0` that completes the §2.3 handshake and exits `0`.

### M2 — §11 S2: the "no child process" oracle counts threads, not children

> §11 S2: "produces no child process (`/proc/self/task` count unchanged)"

**Why it is live.** `/proc/self/task` lists **threads**. `std::process::Command::new("sh").spawn()`
leaves the task count unchanged, so the assertion passes for the exact code it is meant to forbid.
The §1.2 grep list also omits `std::process`, `libc`, `rustix` (already a dependency of
`crates/blastit`) and `std::os::unix::net`.

**Replacement.** Assert the child's `/proc/<pid>/task/<tid>/children` is empty after the fixture (or
that `waitpid(-1, WNOHANG)` returns `ECHILD`), and extend the grep to
`["std::process", "libc", "rustix", "std::os::unix::net", "std::fs", "include!"]`.

### M3 — §11 S4: "a `Blastfile` is never opened" is not proven by the named test

> §11 S4: "`critical_lsp.rs` opens a document whose `uri` is `file:///etc/shadow` and asserts the
> reply is a diagnostic set, not an `Io` error and not file content"

**Why it is live.** A server that *did* open `/etc/shadow`, discarded the bytes and then returned the
diagnostic set for the (empty) document text passes this test. The property "never opened" needs an
oracle that fails when the open happens.

**Replacement.** (a) Grep `lsp.rs` for `std::fs`, `File::open`, `Path::new`, `std::path`,
`read_to_string`. (b) Runtime oracle: point the `uri` at a FIFO with no writer —
`open(2)` for reading blocks — and assert the `hover` reply arrives within 2 s. A server that opens
the path never answers.

### M4 — §11 S5/S8: the panic path is unguarded outside `lsp.rs`, and it writes to stderr

> §11 S5: "**No panic on any input.** `#![deny(clippy::unwrap_used, expect_used, panic, todo, indexing_slicing)]`"
>
> §11 S8: "stderr has at most three fixed reasons; no `uri`, no text, no value"

**Why it is live.** The deny list is an inner attribute in `lsp.rs` only. `Manifest::parse_blastfile`
and `blastfile::fmt` live in `blastit-manifest`, whose lint configuration is `unwrap_used = "warn"`,
`expect_used = "warn"` (`Cargo.toml:16-18`) with no `deny` in `crates/blastit-manifest/src/lib.rs`.
A panic there is reachable from stdin bytes; the default hook prints the payload to stderr, and a
`ManifestError`'s `Debug` carries `quoted()` document text — which breaks S8 — while the process
death breaks S9's "a broken document is not a broken server".

**Replacement.** At the top of `run()`, install
`std::panic::set_hook(Box::new(|_| eprintln!("blastit lsp: internal error")))`, and wrap the
per-document `parse_blastfile`/`fmt` in `std::panic::catch_unwind`, publishing a fixed `(0,0)`
severity-1 diagnostic (`"blastit lsp: internal error; run `blastit check`"`) on a caught panic.
§11 S8 gains: "the panic hook writes one fixed line and never the payload".

**Assertion.** `include_str!("../src/lsp.rs")` contains `set_hook` and the literal
`blastit lsp: internal error`; the S8 test additionally asserts the session's stderr is exactly the
three-line alphabet (a `contains` check for every non-fixed line is not enough).

### M5 — §3.1 / §2.5: `MAX_DOCUMENTS` bounds the count, not the bytes

> §3.1: "`MAX_DOCUMENTS` | `32` | one open `Blastfile` per buffer; 32 is 6× the realistic maximum and **bounds the map**"

**Why it is live.** `MAX_BODY_BYTES` is 33 619 968, so one `Document.text` can be ~33 MiB. The
parser's 16 MiB `MAX_SOURCE_BYTES` refuses the *parse*, not the *storage*: the text stays in the map,
and 32 × 33 MiB ≈ 1.07 GiB. The design's own S6 ("Bounded allocation") does not cover it.

**Replacement.** Add `MAX_DOCUMENT_BYTES = MAX_SOURCE_BYTES` and: "a `didOpen`/`didChange` whose
`text` is longer than `MAX_SOURCE_BYTES` is not stored; the document is removed from the map and the
`TooLarge` diagnostic is published at `(0,0)`." Table row for `MAX_DOCUMENTS` becomes "bounds the
count; `MAX_DOCUMENT_BYTES` bounds each entry".

**Assertion.** `oversized_document_is_not_stored`: open a 17 MiB document, assert the `(0,0)`
`TooLarge` diagnostic, then assert a `hover` for that uri answers `null` and `VmHWM` grew < 40 MiB.

### M6 — §4.2 / §4.3: `point()` rescans the document once per diagnostic

> §4.2: "`let line = match text.split('\n').nth(line_index as usize)`"

**Why it is live.** `point()` is called at least once per diagnostic (§4.3 start, and again for
warnings via `warning_position`). With `MAX_DIAGNOSTICS = 512` on a legal 16 MiB `Blastfile` that is
~512 full-or-partial scans of up to 16 MiB — several GiB of byte scanning on **every `didChange`**,
i.e. a multi-second stall per keystroke on a generated file. The design calls the list "bounded
before it is serialized" but never bounds the conversion cost.

**Replacement.** Build `line_starts: Vec<u32>` (byte offset of each `'\n'` + 1) once per publish in
one O(n) pass, and resolve `point()` by binary search; state the bound as "one pass over the document
per publish, independent of the diagnostic count".

**Assertion.** `publish_is_linear_in_document_size`: a 16 MiB fixture with 512 errors publishes in
< 250 ms (`std::time::Instant`), and a second fixture with 1 error publishes in < 250 ms (proving the
cost does not scale with the count).

### M7 — §9.4 / §10.4: the grammar gate runs an unpinned package from the network

> §9.4: "`package.json` pins `tree-sitter-cli` to an exact `0.25.x` version"
>
> §9.4: "`$ npx --yes tree-sitter-cli@0.25 generate`"

**Why it is live.** `@0.25` is a **range**, not a pin; `--yes` runs whatever the registry serves
without a prompt, and the `git diff --exit-code` gate then blesses that version's generated
`parser.c`. The same command runs in the §10.4 query checks and in `make check`, so CI needs the
network and executes a third-party package with the runner's credentials — in a repository whose
Rust side is `CARGO_NET_OFFLINE`.

**Replacement.** Pin the exact version in `package.json` *and* in every command
(`tree-sitter-cli@0.25.3`), commit `package-lock.json`, and run
`npm ci --ignore-scripts && npx --no-install tree-sitter generate`.

**Assertion.** `check_zed_extension.py` asserts the version string in `package.json` equals the one
in the CI command and that no command contains `--yes`.

### M8 — §2.2 / §2.4: a `didChange` carrying a `range` is dropped silently, leaving stale diagnostics

> §2.4: "`didChange` with zero or more than one change, or a change carrying `range`/`rangeLength` |
> echoed (requests only; a malformed **notification** is dropped)"

**Why it is live.** `didChange` is a notification, so a client that ignores `change: 1` — Helix and
Neovim, which §2.3 explicitly invites — gets no error and keeps the diagnostics of the **old** text.
The editor then shows a policy file as clean after `unrestricted` was added: the LSP lies about the
one thing it exists to report.

**Replacement.** On a `didChange` whose change carries `range`/`rangeLength`, drop the document from
the map and publish the §2.5-style `(0,0)` severity-2 notice
`"incremental sync is not supported; reopen the file"`. Add the row to §2.2 and the failure table
§11.2.

**Assertion.** `incremental_change_is_refused_visibly`: a `didChange` with a `range` publishes one
`(0,0)` severity-2 diagnostic and a following `hover` answers `null` (the document is gone, not stale).

---

## MINOR

### m1 — §5.7 vs §11.2: the truncation count contradicts itself

§5.7: "`<n> = total - 511`" (600 refusals → 89). §11.2: "511 squiggles + `600 more problems; run
blastit check`". Change §11.2 to `89 more problems`.

### m2 — §2.5: the 33rd-document notice has no `version`

§5.1 says `version` is "**absent** from the `didClose` publish" only; §2.5's notice omits it. Add
`"version":0` or list the exception in §5.1.

### m3 — §11 S10: exit `2` contradicts the accepted page

`docs/reference/cli.md:78-80`: "It exits `0` after the client's `shutdown`, and `1` only if the
client disappears without one". `lsp --bogus` → `2` breaks that sentence. Either add the `2` row to
the page or make `parse_lsp` errors exit `1`.

### m4 — §3.3 vs `docs/reference/cli.md:74`: "Logs go to stderr"

The page promises logs on stderr; §3.3 promises **zero** stderr bytes for a valid session. Restate
the page as "diagnostics go to stderr only when the stream is unusable", so a later implementer
cannot reintroduce a log line that carries document text.

### m5 — §3.1: the header-cap boundary is ambiguous

"appending at most `MAX_HEADER_BYTES` bytes in total" vs "a 8 193-byte header block is refused" —
say whether the `\r\n\r\n` terminator counts, and assert both sides: 8 192 accepted, 8 193 refused.

### m6 — §8.3: `shell_env()` verbatim hands the LSP child every exported secret

`blastit lsp` reads no environment (§1.2) yet inherits `DATABASE_URL`, `AWS_*`, `GITHUB_TOKEN` in
`environ` for the whole session, including for untrusted worktrees. Filter to a documented allowlist
(`PATH`, `HOME`, `XDG_*`, `UV_*`, `VIRTUAL_ENV`, `PYTHONPATH`, `LANG`) and assert the filtered set in
`check_zed_extension.py`.

### m7 — §9.1: no assertion that no hand-written scanner ships in the grammar wasm

Zed compiles `scanner.c` "**only** if it exists" (§9.1). Add to `check_zed_extension.py`:
`src/scanner.c` and `src/scanner.cc` absent, and `grammar.js` has no `externals` — a future
hand-written scanner would be C running in the editor's grammar wasm (`--max-memory=4294967296`), a
new memory-unsafe surface the design does not audit.

### m8 — §5.2 / §7: no assertion that a hover never echoes an environment value

`$IDENT` hover interpolates a document-supplied **name**. Add to `critical_lsp.rs`: run with
`DATABASE_URL=postgres://secret@db/app` in the environment, hover `$DATABASE_URL`, assert the reply
contains the name and not `secret`.

### m9 — §2.2: `id: null` treated as absent can hang a client

A client that sends `{"id":null,"method":"initialize"}` and waits for a reply gets none, because the
design treats a `null` id as a notification. JSON-RPC 2.0 discourages `null` ids but permits them;
state the choice in §2.2 or answer `-32600` with `id: null` instead of dropping.

---

## Counts

| Severity | Count |
|---|---|
| BLOCKER | 3 |
| MAJOR | 8 |
| MINOR | 9 |

VERDICT: FAIL

---

## Round 2

Lens: **security**, fresh context, against the revised `docs/design/blastfile-lsp-zed.md`
(1947 lines) and `docs/decisions/ADR-0013-blastfile-lsp-and-zed.md`.

Spot-checks run in the worktree: `crates/blastit-sys/src/caps.rs:623` (`hold_none`, and
`clear`/`verify` behind it), `crates/blastit/src/cli.rs:636-640` (the `--role=dbproxy`
precedent), `crates/blastit/src/host.rs:178` (`BLASTIT_EXEC`), `crates/blastit-py/src/lib.rs:61-67`
(the dbproxy `is_file()` stat before dispatch), `crates/blastit-manifest/src/error.rs:1403-1470`
(`position()` and its single `_ => Position::NONE` at 1468), `.../blastfile/parser.rs:41`
(first-error return), `.../blastfile/mod.rs:202` (`MAX_SOURCE_BYTES = 16_777_216`,
`pub(crate)`), `crates/blastit/src/cli.rs:554` (`report_manifest_errors`),
`crates/blastit-sqlpolicy/src/literals.rs:60` (`has_quoted_literal`),
`crates/blastit-manifest/src/validate.rs:271-289` (`check_queries`/`LiteralQuery`),
`Cargo.toml:20-21` (no `panic = "abort"`), `crates/blastit/src/check.rs:136` (warnings dropped),
`crates/blastit/tests/` (no `critical_lsp.rs` yet), `docs/reference/cli.md:69-82`,
`docs/local.md:136-154`.

### Round-1 findings, one by one

| # | Round-1 grade | Status |
|---|---|---|
| B1 | BLOCKER | **RESOLVED** |
| B2 | BLOCKER | **RESOLVED** |
| B3 | BLOCKER | **RESOLVED** |
| M1 | MAJOR | **RESOLVED** (one summary-table residue, N3) |
| M2 | MAJOR | **RESOLVED** |
| M3 | MAJOR | **RESOLVED** |
| M4 | MAJOR | **RESOLVED** |
| M5 | MAJOR | **RESOLVED** (bound note, N10) |
| M6 | MAJOR | **RESOLVED** |
| M7 | MAJOR | **RESOLVED** (assertion gap, N8) |
| M8 | MAJOR | **RESOLVED** |

**B1 — RESOLVED.** §1 row 3 now reads "**one capability line**: the `Lsp` arm calls
`blastit_sys::caps::hold_none()` before `lsp::run()` (§11 S3)", §1.1 repeats it ("the `Lsp`
arm drops every file capability **before** it reads a byte (`blastit_sys::caps::hold_none()`,
the call `--role=dbproxy` already makes at `cli.rs:638`)"), and §11 S3 states the property:
"The `Lsp` arm calls `caps::hold_none()` before it reads a byte, and `lsp.rs` reads no
environment, no path, no socket". The call is real and fail-closed: `caps.rs:623`
`pub fn hold_none()` returns `Ok` only when `Capabilities::read()?.holds_none()`, otherwise
`clear()` → `verify(&[Ambient, Inheritable, Permitted, Effective])`, documented as "a process
that cannot prove it holds nothing must not serve". The `cli.rs:638` precedent exists at the
cited line. The round-1 runtime oracle (`capsh --addcap=cap_dac_override` then `CapEff == 0`)
did not survive into §12.1 — see N4/N12.

**B2 — RESOLVED.** §8.3's table row is now "`worktree.which("uv")` second, argv `run --frozen
--no-sync --quiet blastit lsp`" with the reason spelled out ("a bare `uv run` syncs the project
first, which runs the worktree's declared build backend with `shell_env()` in `environ`"), the
`src/lib.rs` listing carries the same six literals, and ADR §6 is reworded to "The extension
resolves the server without executing repository code" plus "`--no-sync` is not a detail". The
guard claim is concrete ("the guard asserts the **six** argv literals in order, that `--no-sync`
is present, and that the error string contains `uv sync`"). The drive-by build-backend execution
is closed.

**B3 — RESOLVED.** §2.5 now says "**`didOpen` inserts; `didChange` replaces.**" and closes the
hole in one sentence: "The map therefore has exactly as many entries as `didOpen`s that were
accepted, and no notification other than `didOpen` can add one." The unknown-`uri` case is
visible, not silent ("`blastit lsp has no open document for this file; reopen it`"), and
`did_change_for_unknown_uri_does_not_grow_the_map` (40 fresh-uri `didChange`s, then 32
`didOpen`s, then the 33rd notice) is the right oracle. A second `didOpen` for an already-open
`uri` is still unspecified — N11.

**M1 — RESOLVED, with a residue.** The false sentence is gone from §11 S3: the property is now
"`lsp.rs` reads no environment, no path, no socket", and the oracle is behavioural ("a session
spawned with `.env_clear()` completes the handshake and one diagnostic (so no environment is
needed, `BLASTIT_EXEC` included)"). That is assertable and true. The residue is §0's Deploy row
and the resolution table's overstatement — N3.

**M2 — RESOLVED.** §11 S2's oracle is now "`/proc/<server_pid>/task/<tid>/children` is empty
before and after — `/proc/self/task` is the **test** process's thread table and proves
nothing", and §1.2's list is the extended one
(`"std::fs", "std::path", "std::process", "std::os::unix::net", "libc", "rustix", …`).

**M3 — RESOLVED.** §11 S4: "a document whose `uri` names a **FIFO with no writer** (`mkfifo`) —
`open(2)` for reading blocks, so a reply within 2 s proves the path was never opened", plus the
`std::fs`/`std::path`/`File::open` grep. The property now has an oracle that fails when the open
happens.

**M4 — RESOLVED.** §3.3 installs `std::panic::set_hook(Box::new(|_| eprintln!("blastit lsp:
internal error")))` at the top of `run()`, wraps "every call into `blastit-manifest` for one
document" in `catch_unwind`, and publishes a fixed severity-1 diagnostic instead of dying. The
mechanism is live: `Cargo.toml` sets only `[profile.release] lto = "thin"`, so the default
`panic = "unwind"` holds and `catch_unwind` actually catches.

**M5 — RESOLVED.** §2.5: "`MAX_DOCUMENT_BYTES = MAX_SOURCE_BYTES` bounds **each entry**: a
`didOpen` or `didChange` whose `text` is longer than the parser's own cap is not stored at all",
with the `TooLarge` diagnostic still published from the length check
(`parser.rs:41` refuses before lexing) and the `uri` removed. 32 × 33 MiB is gone. The remaining
worst case is 32 × 16 MiB = 512 MiB (N10).

**M6 — RESOLVED.** §4.2: "the lines are found once per publish", with `Lines::new` building
`starts` in one pass and `point()` binary-searching; the bound is stated as
"`Lines::point()` is `O(log lines + col)` — independent of `MAX_DIAGNOSTICS`" and timed by P6.
`at.line() - 1` cannot underflow because `Position::is_none()` is `line == 0` (`error.rs:86-88`)
and is checked first.

**M7 — RESOLVED.** §9.4: "`npm ci --ignore-scripts` installs it from the committed lockfile (a
build-host step; CI caches `node_modules`), and `npx --no-install` can only use what is there",
`--yes` is gone from every command, and the lockfile pins the integrity hash, so the
supply-chain exposure is closed even if `package.json` kept a range. The *pin assertion* is
weaker than round 1 asked — N8.

**M8 — RESOLVED.** §2.2's row now says "An unknown `uri` or a change carrying
`range`/`rangeLength` is refused visibly (§2.5)", and §2.5 adds "**and** removes the `uri` from
the map (its old text's diagnostics must not survive the refusal)". The stale-clean-`unrestricted`
failure mode is closed, with `incremental_change_is_refused_visibly` as the oracle.

### New findings

#### N1 — MAJOR — S2's fixture produces no `LiteralQuery`, so the only deterministic "never executed" oracle cannot pass

> §11 S2: "a document with `query 'COPY x FROM PROGRAM "sh"'` publishes **exactly** the
> `LiteralQuery` diagnostic (severity 1, `source: "blastit"`), stderr is empty, and
> `/proc/<server_pid>/task/<tid>/children` is empty before and after"

`LiteralQuery` fires only when `has_quoted_literal(&query)` is true (`validate.rs:284`), and
`has_quoted_literal` counts **literals**, not quoted identifiers: its own doc test asserts
`assert!(!has_quoted_literal(r#"SELECT "x""#))` (`crates/blastit-sqlpolicy/src/literals.rs:39`),
and the module doc says "Quoted identifiers are not literals". `"sh"` in
`COPY x FROM PROGRAM "sh"` is a quoted identifier, so the statement has no literal, `validate`
returns `Ok`, and the document publishes **zero** diagnostics. The repository's own passing
fixture uses the other spelling — `query "SELECT \"notes\".\"name\" … = 'secret'"`
(`crates/blastit-manifest/tests/critical_manifest_011.rs:684`): a double-quoted Blastfile string
holding a single-quoted SQL literal.

The test as written is red for the wrong reason, and the likely "fix" is to weaken the assertion
to "no diagnostics", losing the check that the dangerous statement was *read* rather than run.
**Fix:** make the fixture `query "COPY x FROM PROGRAM 'sh'"` (Blastfile `"…"`, SQL `'…'`), and
keep "exactly one `LiteralQuery`, severity 1, `source: "blastit"`" as the assertion. The same
fixture then also fixes N7.

#### N2 — MAJOR — §12.5's table is empty and §12.6–§12.8 do not exist, though the resolution table and R53–R59 point at them

The document ends §12.4 and then jumps straight from a table header to the requirement index:

```
### 12.5 The live proof: both distribution paths

| Path | How it is driven |
## Requirement index
```

There is no §12.6, §12.7 or §12.8 anywhere in the 1947 lines, yet they are cited seven times:
"§12.6 records as the manual pass" (line 1449), "§12.8 records that hand-off" (1634), "§12.8
lists them" (1710), "**hand-off** → §12.8" (1873, 1906), "§12.8" (1916, 1917), and R53–R59 are
"the commands of §12.1–§12.5; §12.6–§12.7 are human-only" (1845). Consequences a security
reader cares about: the m3 hand-off (the `2` exit row in `docs/reference/cli.md`, which still
reads "and `1` only if the client disappears without one" with no `2` row, `cli.md:80-82`) is
unrecorded; the "Nothing is deferred silently" claim in the Review Resolution is false; and
R53–R59 have no command a reviewer can run. **Fix:** write §12.5's two rows (binary and wheel),
§12.6/§12.7 (the manual Zed passes), and §12.8 (the hand-off list), or delete the citations and
re-grade the hand-offs.

#### N3 — MINOR — §0 still says `blastit lsp` "touches no `BLASTIT_*` variable", which is false at the process boundary

> §0 Deploy: "`blastit lsp` touches no privilege, no `BLASTIT_*` variable, no `/run`, no
> listener"

`Host::binary()` reads `BLASTIT_EXEC` (`crates/blastit/src/host.rs:56,178`) and `main.rs:14-19`
constructs `Host` before `cli::main`, and `_blastit.main` stats the dbproxy path and raises
before dispatch (`crates/blastit-py/src/lib.rs:61-67`). §11 S3 is now correctly scoped
("`lsp.rs` reads no environment"), and the resolution table's claim that the property "names the
two reads that happen before dispatch" is not in the text. **Fix:** §0 → "needs no `BLASTIT_*`
variable", and either name the two pre-dispatch reads in §11 S3 or drop that sentence from the
resolution table.

#### N4 — MINOR — the new fail-closed `hold_none` refusal is outside every exit/stderr table

The `Lsp` arm's `Err` path prints `blastit lsp: {error}` and returns `FAILURE`. That is a new
code path the revision introduced, and it is in none of: §2.4's exit table (which lists only
`0`/`1`/`2` for session and CLI cases), §11.3's "four stderr sentences" (it interpolates an
`io::Error` `Display`, the only non-`&'static str` line in the arm), or S10's six sessions. S8's
"every stderr line is one of the four literals" would fail if that path were ever exercised in
the same assertion. **Fix:** add the row (`hold_none` failure → exit `1`, one fixed sentence such
as `blastit lsp: cannot drop capabilities`), or reuse an existing literal.

#### N5 — MINOR — "the seven position-less variants" is wrong; the wildcard covers ~31

> §1: "`MB.22.3` replaces that one arm with the explicit list of the seven position-less variants
> (`TooLarge`, `TooManyTokens`, `TooManyTrivia`, `TooManyRoutes`, `UidBaseTooHigh`,
> `NotRegularFile`, `Unreadable`)"

`error.rs:1404-1469` has exactly one `_ => Position::NONE` and no intermediate arms, so every
variant without `at` falls through — the seven whole-file ones **plus** the `Location` family
§4.2 itself lists (`UnknownGroup`, `GrantOnUnknownProxy`, `UnrestrictedGroup`,
`UnrestrictedWithQueries`, `UnrestrictedWithContent`, `UnusableTable`, `RedactedQueryPlaceholder`,
`LiteralQuery`, the `Static*` family, `Php*`, `InvalidApp`, `DuplicateRouteName`, …). Replacing
the wildcard with seven arms does not compile; the cheapest way out is to put the wildcard back,
which kills R3a. The `no_position_of_a_validation_error_is_invented` grep catches that, so it
fails loudly — but the leaf task as written is not buildable. **Fix:** "the explicit list of
every position-less variant: the seven whole-file ones plus the `Location`-carrying family of
§4.2".

#### N6 — MINOR — S5's fuzz corpus contains a case the exit table says ends the session

> §11 S5: "2 000 framed bodies from a fixed xorshift generator (`serde_json` shapes, truncated
> UTF-8, `Content-Length` over the cap, every `id` type) leave the process alive"

§3.4 row 2 and S10 both say `Content-Length` over the cap exits `1`. The corpus cannot both
include it and assert liveness. **Fix:** drop that case from the liveness corpus (it has its own
row, `oversized_body_is_refused_before_allocation`) or split the assertion into "the first 1 999
leave it alive; the over-cap frame exits `1`".

#### N7 — MINOR — S8's fixture yields no diagnostic, so the assertion is vacuous for the realistic leak path

> §11 S8: "a fixture whose SQL is `SELECT secret_token FROM api_keys`: stderr contains none of
> `secret_token`, `SELECT`, `api_keys`, the uri"

`query 'SELECT secret_token FROM api_keys'` has no quoted literal, no `…` and no table grant, so
`validate` returns `Ok` and the publish is `[]`. The test then proves only that the server does
not log unconditionally; it does not exercise the paths that could leak text (an error message,
a panic payload, a `Debug` of `ManifestError`). **Fix:** use the N1 `LiteralQuery` fixture, whose
message must not print the statement (`critical_manifest_011.rs:700-706` already asserts that),
and keep the `SELECT`/`secret_token`/uri `contains` checks.

#### N8 — MINOR — the `$TS` line is dead and nothing asserts the pin is exact

> §9.4: "`$ TS=$(node -p "require('./package.json').devDependencies['tree-sitter-cli']")`"

`TS` is assigned and never used by the four commands that follow, so it asserts nothing; §9.1
still writes "tree-sitter-cli 0.25.x (pinned)", the range notation round 1 objected to; and
§12.4's enumerated guard list contains no `package.json`/`--yes` property at all, so the
"exact version" claim has no oracle. The supply-chain risk is closed by `npm ci` + the lockfile,
so this is polish: **fix** by deleting `$TS`, writing the resolved version literally in §9.1, and
adding "`package.json`'s `tree-sitter-cli` value has no `^`/`~`/`x`, and no command contains
`--yes`" to §12.4.

#### N9 — MINOR — two `include_str!` paths cannot compile

`no_position_of_a_validation_error_is_invented` is specified as "`include_str!("../src/error.rs")`
does not contain `_ => Position::NONE`" (§12.1), but the test lives in
`crates/blastit/tests/critical_lsp.rs`, so `../src/error.rs` resolves to
`crates/blastit/src/error.rs`, which does not exist (`ls crates/blastit/src/` has no `error.rs`);
the manifest's file is `../../blastit-manifest/src/error.rs`. Separately, §7.2's
`include_str!("../../../../docs/reference/blastfile.md")` is one `..` too many: from
`crates/blastit/src/` three `..` reach the repository root. Both fail at compile time, so they
are loud — but they make two of the design's named assertions unbuildable as written.

#### N10 — MINOR — the stated worst case is still 512 MiB

§3.1: "bounds each entry's **text**, so 32 buffers cannot reach `32 × MAX_FRAME_BODY_BYTES`".
True, and the worst case is now 32 × 16 MiB = 512 MiB, plus a 33.6 MiB frame and its
`serde_json::Value`. That is bounded but not small for an editor helper. Consider a total-byte
budget (e.g. 64 MiB across the map, evicting the oldest) or `MAX_DOCUMENTS = 8`, and state the
number the design accepts.

#### N11 — MINOR — a second `didOpen` for an already-open `uri` is unspecified

§2.5 says "`didOpen` inserts; `didChange` replaces" but not what a repeated `didOpen` does.
`BTreeMap::insert` replaces (correct); `entry().or_insert_with(...)` keeps the old text and
republishes stale diagnostics — the exact M8 failure class, reachable by a client that re-opens a
file without closing it. **Fix:** "a second `didOpen` for an open `uri` replaces the text and the
version and republishes", plus one critical row.

#### N12 — MINOR — `hold_none` clears capabilities, not euid

`hold_none` proves the four capability sets are empty; it does not change the uid. A
`blastit lsp` started from a root shell therefore still parses editor bytes as uid 0 (owner-level
file access, and any root-owned path the process could reach), which is weaker than B1's
"parsed by a process holding nothing". The deploy case (a `setcap`'d binary run by a developer)
is fine. **Fix:** state that the uid is the caller's, or refuse `geteuid() == 0` in the `Lsp` arm
with one of N4's fixed sentences.

#### N13 — MINOR — S6's allocation sentence understates the `serde_json::Value`

§3.2: "Every other buffer is `READ_CHUNK` or grows with the document the client already sent."
A 33 MiB body of many small keys (`{"a":1,"a":1,…}`) becomes a `serde_json::Value` of millions of
`String`/`Number` allocations, several hundred MiB — bounded by `MAX_FRAME_BODY_BYTES` times a
constant, not by the document. **Fix:** say "O(`MAX_FRAME_BODY_BYTES`) with `serde_json`'s
per-node constant", or cap the frame body for frames that are not `didOpen`/`didChange`.

### Counts

| Round 2 | Count |
|---|---|
| BLOCKER open | 0 |
| MAJOR open | 2 (N1, N2) |
| MINOR open | 11 (N3–N13) |
| Round-1 BLOCKER/MAJOR resolved | 11 / 11 |

The three BLOCKERs are genuinely fixed in the text and in the code they call. The revision's own
law — "a claim a test can make is written as an assertion, in the words the test asserts" — is
what breaks the verdict: S2's fixture cannot produce the diagnostic it names, and the section
that records every hand-off (§12.8) does not exist.

VERDICT: FAIL

## Round 3

Round 3 read the current 2036-line revision against the same code sites, in this order:
the four items round 2 left open (N1–N5), then one scan for a security claim whose
oracle cannot fail, and for the four surfaces this revision introduced (the
`_values`-optional `directive`, `grant`'s `unrestricted` alias, `MAX_DOCUMENT_BYTES`,
`catch_unwind`).

### The round-2 items

| # | Round-2 grade | Status | Evidence in this revision |
|---|---|---|---|
| N1 | MAJOR | **RESOLVED**, one residue (N14) | §11 S2 (l.1675) now reads `query "COPY x FROM PROGRAM 'sh'"`; the Blastfile `"…"` holds an SQL `'…'`, so `has_quoted_literal` is true (`crates/blastit-sqlpolicy/src/literals.rs:40-52`) and `check_queries` pushes exactly one `LiteralQuery` (`validate.rs:284-291`). No other validator inspects the statement text, so "exactly the `LiteralQuery` diagnostic" is achievable. S2 remains the only deterministic non-execution oracle in §11. |
| N2 | MAJOR | **RESOLVED** | §12.5 has both distribution rows and a numbered session (l.1846–1877); §12.6 (l.1879), §12.7 (l.1888) and §12.8 (l.1902) exist, and every citation from §9.4, §10.4, §11 S10, §12.3 and the resolution tables now lands on text. §12.5's session is coherent as a protocol trace: step 2's `line 1`/`character 5` is P1's `(1,5)`, step 4's `end (5,0)` is the fixture's five `'\n'`, and step 5's `0`/`1` split is S10's. Its *coverage* sentence is wrong — N15. |
| N3 | MINOR | **RESOLVED** | §0 (l.48): "reads no `BLASTIT_*` variable **of its own**". The two pre-dispatch reads (`Host::binary()`'s `BLASTIT_EXEC`, `_blastit.main`'s dbproxy stat, `crates/blastit-py/src/lib.rs:61-67`) are no longer denied by the sentence, and §11 S3 keeps the precise scope. |
| N4 | MINOR | **OPEN (half)** | §2.4 (l.263–269) now has the `0`/`1`/`2` rows, and the `hold_none` failure is the `1` row's third clause. The *sentence* half is not fixed: the row still promises `blastit lsp: <error>`, an interpolated `io::Error`/`caps::Error` `Display` (`blastit lsp: CapabilitiesRemaining { … }`), while §11.3 (l.1717) says "there is no fifth" stderr line and S8 (l.1681) asserts "every stderr line is one of the four literals". The two cannot both be true. |
| N5 | MINOR | **RESOLVED** | §1 (l.112–121) no longer counts: "an explicit arm for **every** variant that carries neither `at` nor `Location` — today the whole-file seven …, `InvalidApp`, and the older route/mount refusals …, which §4.2 lists as the `Position::NONE` family". That is the set the wildcard actually covers, so the arm is buildable. |

The three findings that mattered in round 2 are fixed in substance, not in phrasing:
the S2 fixture now produces the diagnostic it asserts, §12.5–§12.8 exist and are cited
correctly, and the deploy row no longer denies a read it cannot control.

### Carried over from round 2, still open

Every one is MINOR and none blocks the verdict; they are listed so the record is not
selective.

| # | Still true because |
|---|---|
| N6 | §11 S5 (l.1678) still seeds the liveness corpus with "`Content-Length` over the cap", which §3.4 row 2 and S10 say exits `1`. |
| N7 | §11 S8 (l.1681) still uses `query 'SELECT secret_token FROM api_keys'`, which yields no diagnostic, so the leak assertion is vacuous for the paths that could print text. N1's fixture was meant to fix this too; S8 was not updated. |
| N8 | §9.4 (l.1452) still assigns `TS` and never uses it; §9.1 (l.1283) still says "tree-sitter-cli 0.25.x (pinned)"; §12.4's enumerated guard list still has no `package.json`/`--yes` property. |
| N9 | Half fixed: `include_str!("../../blastit-manifest/src/error.rs")` (l.1767) is now correct, but §7.2 (l.993) still writes `include_str!("../../../../docs/reference/blastfile.md")`. From `crates/blastit/src/`, three `..` reach the repository root, so the fourth fails to compile. |
| N10 | §2.5 / §3.1 still accept 32 × 16 MiB = 512 MiB as the map's worst case, plus the frame; no total-byte budget and no stated accepted number. |
| N11 | §2.5 (l.287–294) still defines only "`didOpen` inserts; `didChange` replaces" and never says what a second `didOpen` for an open `uri` does. `BTreeMap::insert` replaces; `entry().or_insert_with` keeps stale text — the M8 failure class. |
| N12 | §11 S3 still says the session is parsed "by a process holding nothing" without saying the *uid* is the caller's; `hold_none` clears capability sets, not `geteuid()`. |
| N13 | §3.2 (l.348–350) still says "Every other buffer is `READ_CHUNK` or grows with the document"; a 33 MiB body of many small keys becomes a `serde_json::Value` of millions of allocations. |

### Scan of the four surfaces this revision introduced

| Surface | Security verdict |
|---|---|
| `directive` with `optional($._values)` (l.1358) | **No exposure.** It is required: without it a value-less `transactions`/`prefork` inside a block is an `ERROR` on a file the parser accepts, i.e. a false red. Accepting *zero* values the parser refuses is coloured-no-red, and the LSP squiggle still fires. One documentation gap — N18. |
| `grant`'s `alias('unrestricted', $.keyword)` (l.1365) | **No exposure, one dead oracle.** The alias cannot paint a dangerous grant as safe (the red comes from `validate`'s `UnrestrictedRoute`, sited on that token), but it leaves `unrestricted` with no capture and `mode`, the only `@constant` source, unreachable — N16. |
| `MAX_DOCUMENT_BYTES` (l.299–305, 326) | **Correct and fail-closed.** Text over 16 MiB is not stored, `TooLarge` is still published from the length check before lexing (`parser.rs:41-47`), and the `uri` is dropped so no stale answer survives. The bound is per entry, so the 512 MiB map (N10) and the frame `Value` (N13) remain, both MINOR as graded. |
| `catch_unwind` + `set_hook` (l.377–394) | **Sound as written.** `Cargo.toml` sets only `[profile.release] lto = "thin"`, so `panic = "unwind"` holds and the hook replaces the payload-printing default before any document is parsed; `print.rs` has no `unwrap`/`expect`/`panic!` outside `#[cfg(test)]` and `parser.rs` has `MAX_BLOCK_DEPTH`, so the wrapped calls are not expected to panic. The gap is that nothing tests the assumption — N19. |

### New findings

#### N14 — MINOR — S2's fixture is still a fragment, and its procfs precondition is unstated

> §11 S2: "a document with `query "COPY x FROM PROGRAM 'sh'"` publishes **exactly** the
> `LiteralQuery` diagnostic"

The statement is now right; the *document* is still not written out. `query` is not a
section: at top level `parser::parse` refuses it with a syntax error before `validate`
runs, so a test that framed the fragment literally would publish one diagnostic — the
wrong one — and the assertion would fail for a third reason after N1's two. The fixture
must be spelled out, as the reference page does (`docs/reference/blastfile.md:245`):

```text
view v "/x" {
    postgres {
        query "COPY x FROM PROGRAM 'sh'"
    }
}
```

Second, the round's strongest oracle has an unstated precondition:
`/proc/<pid>/task/<tid>/children` exists only with `CONFIG_PROC_CHILDREN`. The design
does not say what the test does when it is absent, and the obvious implementation
(`if path.exists() { assert empty }`) makes the half vacuous on exactly the kernels
where it matters. **Fix:** write the document out, and state "a missing `children` file
is a test failure, not a skip".

#### N15 — MINOR — §12.5 promises three §6.1 outcomes and drives two

> §12.5: "sends **one session with three documents**, so all three §6.1 outcomes are
> exercised"

The session opens one `uri` and sends three *texts* (one `didOpen`, two `didChange`),
and it requests formatting only on the canonical text (`[]`) and the two-space text
(one `TextEdit`). The `null` outcome — formatting a buffer `fmt` refuses — is never
requested, although step 2 already has the broken buffer in hand. `critical_lsp.rs`'s
`formatting_broken_is_null` covers the behaviour, so this is a live-proof gap, not a
coverage hole. **Fix:** "one document, three texts", and add a `formatting` request on
the step-2 text with `result == null` before the first `didChange`.

#### N16 — MINOR — `mode` is unreachable, so `unrestricted` gets no capture and §10.4(c) cannot pass

> §9.2: "`mode: $ => alias(choice('transactions', 'functions', 'unrestricted'), $.keyword)`"
> §10.4: "`unrestricted` appears as `constant` while `select` appears as `function`"
> `… | grep -q 'constant.*unrestricted'`

`_statement` (l.1353–1355) is `choice(directive, endpoint, grant, use_directive,
table_grant)` — `mode` is in no rule, so `(mode (keyword)) @constant` (l.1553) can never
match. A grant's `unrestricted` is `alias('unrestricted', $.keyword)` under `grant`, and
the only capture for `grant` is `(grant "postgres" @keyword)` on the literal. There is no
`@constant` on the word that grants every statement, so the §10.4(c) grep fails, and
`tree-sitter generate` will also warn about the unused rule. **Security effect: none** —
the signal is `UnrestrictedRoute`, published at that token (§5.4 l.685) — but a named
assertion with no passing state is the class this round exists to remove. **Fix:** either
`_statement` gains `$.mode`, or `grant` drops the alias and `highlights.scm` captures
`(grant (keyword) @constant)`, with §10.4(c) updated to the rule that exists.

#### N17 — MINOR — `_value` has no `raw_string`, so `query '…'` is a false red on the reference's own example

> §9.2: "`_value: $ => choice($.codec, $.env_var, $.string, $.name)`"
> §9.3: "a `query` that takes a `"…"` string as well as `'…'` | nothing — both are legal"

`raw_string` is reachable only through `regex_raw_string`, so a single-quoted query —
`query 'SELECT count(*) FROM products WHERE price > $?'`,
`docs/reference/blastfile.md:245`, the page §8.4/§9.5 call the complete example — has no
production: the directive ends at `query`, and `'SELECT …'` is not a `_statement`. That
is an `ERROR` node on a file `blastit` accepts, exactly what §9.2's first law ("No false
red") forbids, and it makes the §9.5 corpus rows `multi-line-raw-string.txt` and
`env-and-query.txt` unprovable. It is loud — R47's fixture loop runs over
`test/fixtures/complete.Blastfile` and would fail — so it cannot ship, but the grammar as
written cannot pass its own gate. **Fix:** `_value: choice($.codec, $.env_var, $.string,
$.raw_string, $.name)`, which leaves `regex_raw_string`'s alias intact.

#### N18 — MINOR — the newly optional `_values` makes the looseness table's first row false

> §9.3: "any directive keyword followed by **one or more** values of any `_value` shape
> (`body body body`) | `NotADirectiveHere`, `BodyNeedsCodec`, …"

With `optional($._values)`, `body`, `query` and `host` with **zero** values are now
grammar-legal and parser-refused, and that case is in no row. §9.2's second law is
"Deliberate looseness is listed", so the table must say it: "a directive keyword with no
values (`body`, `query`) — coloured, no red". No security consequence (the parser's
sentence is the red); it is the same claim-versus-text drift N5 was.

#### N19 — MINOR — `catch_unwind`'s two assumptions have no oracle

> §11 S5: "a `panic::set_hook` that writes one fixed line, and `catch_unwind` around
> every `blastit-manifest` call per document"

S5's assertion is `include_str!("../src/lsp.rs")` contains `set_hook` and `catch_unwind`.
That grep passes with one `catch_unwind` around `check` and none around `fmt`, and it
passes in a build where `panic = "abort"` makes `catch_unwind` a no-op (the hook still
prints, then the process aborts — an exit code outside §2.4's `0`/`1`/`2`). Neither
assumption is checked. **Fix:** two cheap assertions in `lsp.rs`'s `mod tests` —
`assert!(std::panic::catch_unwind(|| panic!("x")).is_err())` (proves the profile unwinds)
and a count: `include_str!` contains at least as many `catch_unwind` occurrences as the
manifest calls in `run` (`check` and `fmt`). The `.env_clear()` and `print.rs` evidence
above is why this stays MINOR: today the call sites are panic-free.

### Counts

| Round 3 | Count |
|---|---|
| BLOCKER open | 0 |
| MAJOR open | 0 |
| MINOR open | 15 (N4 half, N6–N13, N14–N19) |
| Round-2 MAJOR resolved | 2 / 2 |
| Round-2 MINOR resolved | 2 / 5 (N3, N5) |

No open finding leaks a secret, weakens a sandbox, or lets an input reach a runtime
parser: the frame reader is bounded before allocation, `hold_none` fails the session
closed, the panic hook cannot print payloads, and the LSP still opens nothing but stdin.
The revision's grammar and fixture defects (N14–N18) are honest test failures, not
bypasses, and each has a one-line fix. What round 2's FAIL turned on — an oracle that
could not fire and a section that did not exist — is fixed.

VERDICT: PASS
