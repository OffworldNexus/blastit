---
lens: testability
targets:
  - docs/design/blastfile-lsp-zed.md
  - docs/decisions/ADR-0013-blastfile-lsp-and-zed.md
reviewer: critic (fresh context)
date: 2026-11-17
---

# Testability review — `blastit lsp` and the Zed Blastfile extension

I read the design and `ADR-0013` blind, then verified every repo claim they make
(`crates/blastit-manifest/src/{error,model,validate,uid}.rs`,
`blastfile/{mod,lexer,parser,print}.rs`, `crates/blastit/src/{cli,main,host}.rs`,
`crates/blastit-py/src/lib.rs`, `python/blastit/__main__.py`, `Makefile`,
`tools/critical_guard.py`, `docs/reference/{blastfile,cli}.md`, `docs/local.md`,
`python/tests/test_docs_assets.py`, `plan/nodes/MB/MB.22/{MB.22.3..MB.22.6}`).
Nothing was built and no test was edited.

The design is unusually good at *stating* assertions. The problem is the gap
between the assertion and the row that would run it: §12.1's table is the
implementer's checklist, and it is missing rows for four of the eleven security
invariants, four JSON-RPC error codes, one bound, and every grep. Three
assertions are wrong or vacuous as written, and one protected test asserts a
literal that a correct implementation cannot produce.

**Counts: 3 BLOCKER, 18 MAJOR, 12 MINOR.**

---

## BLOCKER

### B1. S2 ("a document is parsed, never executed") has no test row, and its only stated oracle cannot detect the violation

> §11.1 S2 (line 1371): "`critical_lsp.rs` asserts a document containing
> `query 'COPY x FROM PROGRAM "sh"'` publishes a diagnostic **at worst** and
> produces no child process (`/proc/self/task` count unchanged) and no stderr"

Two independent defects:

1. **No row.** §12.1's table (lines 1419–1441) has no S2 entry. It has S4
   (`no_document_is_opened`), S5, S6, S7, S8, S10 — but not S1, S2, S3 or S11.
   The Requirement index (line 1549) claims S1–S11 are proved by
   "`critical_lsp.rs`, the guard, review". A cheap implementer writes exactly the
   rows in §12.1; S2 will not exist.
2. **The oracle is incapable of failing.** `/proc/self/task` is the **test**
   process's thread table. An `execve`/`fork` inside the server adds a task to
   `/proc/<server_pid>/task`, never to the test process, and a *process* is not a
   *thread*: even `/proc/<server_pid>/task` count would be unchanged by a child
   process. The assertion passes with `std::process::Command` present.
   "publishes a diagnostic **at worst**" is also vacuous: it is satisfied by
   publishing nothing.

**Replacement row** (add to §12.1):

| Test | Asserts | § |
|---|---|---|
| `a_document_is_never_executed` | `didOpen` of a file whose `query 'COPY x FROM PROGRAM "sh"'` grant is refused; the published set contains exactly the `LiteralQuery` diagnostic (`severity:1`, `source:"blastit"`); `/proc/<server_pid>/task/*/children` is empty after the session (Linux ≥ 3.5), and `wait4`/`getrusage(RUSAGE_CHILDREN).ru_maxrss` shows the child never forked; stderr is empty | S2 |

(`query 'COPY … "sh"'` is deterministic: the quoted literal makes
`validate.rs:284` emit `LiteralQuery`, so "at worst" can become "exactly".)

### B2. H5 is vacuous for R17 — it passes with `char_index` deleted

> §7.5 H5 (line 826): "`"view v \"/😀\" { body = }"` hover at UTF-16
> `character` 20 (the `=` twice over) | `null` (punctuation), and at 17 (`body`)
> the codec sentence — the inverse conversion of §4.4"

Hand-computed: `body` starts at char 14 = UTF-16 unit **15**; `character` 17 is
the `d` (unit 17), not the start of the word. Because §7.1 expands the word in
both directions, a broken `char_index` that returns `character as usize` gives:

| `character` | correct char index | word | buggy char index | word | same? |
|---|---|---|---|---|---|
| 17 | 17 (`y`) | `body` | 17 (`y`) | `body` | **yes** |
| 20 | 19 (`=`) | `null` | 20 (` `) | `null` | **yes** |

So both assertions hold with §4.4's conversion replaced by a cast. R17 has no
test with teeth.

**Replacement:** hover at UTF-16 **18** (the `y` of `body`): correct → `body`
sentence; `character as usize` → char 18 is the space after `body` → `null`.
Cheapest discriminating fixture: `"😀 view"`, hover at `character` **6** (the
`w`): correct → the `view` sentence, buggy → past the end → `null`. Keep 20 for
the punctuation case.

### B3. MB.22.3's acceptance smoke command passes with the whole `lsp` arm removed

> `plan/nodes/MB/MB.22/MB.22.3/NODE.md` acceptance (line 92):
> `$ printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run -q -p blastit -- lsp; test $? -eq 1`

The body is exactly 58 bytes (verified), so the frame is well formed. But the
assertion is only `$? -eq 1`. If `Command::Lsp` did not exist,
`cli.rs:53`'s `from_argv` falls through to `Self::Serve`, `parse` rejects the
argument `lsp`, prints usage and returns `FAILURE = 1` — the command passes. It
also passes if the server panics on startup. It asserts nothing about stdout.

**Replacement:**

```console
$ out=$(printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run -q -p blastit -- lsp); printf '%s' "$out" | grep -q '"positionEncoding":"utf-16"' && printf '%s' "$out" | grep -q '"serverInfo"'
$ out=$(printf 'Content-Length: 58\r\n\r\n{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | cargo run -q -p blastit -- lsp); printf '%s' "$out" | head -c 16 | grep -qx 'Content-Length: '
```

---

## MAJOR

### M1. `malformed_body_answers_parse_error_and_serves` asserts `-32700` for a body that §2.4 says is `-32600`

> §12.1 (line 1424): "three bad bodies (`{`, `[]`, invalid UTF-8) → three
> `-32700` with `id: null`"
> §2.4 (line 190): `-32600` — "not a JSON object"; §3.4 row 4 (line 295):
> "body is not JSON, or is JSON that is not an object | `-32700` / `-32600`"

`[]` is valid JSON and not an object, so it must answer `-32600`. The row as
written fails against a correct server, and a cheap implementer will "fix" the
server to answer `-32700` for `[]`, breaking the code table.

**Replacement:** `{` → `-32700`; invalid UTF-8 → `-32700`; `[]` → `-32600`; then
a valid `hover` answers (the keep-serving rule is unchanged).

### M2. F5(d) asserts `end = (4,0)` for a buffer with five `\n`

> §6.3 F5 (line 680): "(d) `end` is `(4,0)`"
> Fixture (line 676): `"app \"shop\"\r\n\r\nview v \"/x\" {\r\n    body json\r\n}\r\n"`

Count the terminators: after `app "shop"` (1), the blank line (2), `{` (3),
`body json` (4), `}` (5). §6.2's own rule (`end.line = number of '\n'`) gives
**(5,0)**. `print.rs:739`'s `a_crlf_file_is_written_with_lf` confirms the
canonical output is `app "shop"\n\nview v "/x" {\n    body json\n}\n` — five
newlines. A protected critical test asserting `(4,0)` fails on a correct
implementation, and the likely repair is to change `max_point_utf16` to
`line_count - 2` or to drop the last line.

**Replacement:** "(d) `end` is `(5,0)`".

### M3. F1's four `end` rows are all ASCII, so `end.character = sum(utf16_len(…))` is untested

> §6.2 (line 633): "`end.character = sum(utf16_len(c) for c in the text after
> the last '\n')`"
> §6.2 (line 645): "**F1** asserts the four rows above as literals"

`""`, `"a\nb"`, `"a\n"` and the four-line buffer contain no non-ASCII
character, so a `chars().count()` or a byte-length implementation passes all
four. The one place the design pins a UTF-16 conversion for formatting has no
oracle.

**Add a fifth row** to §6.2's table and to F1:

| Buffer | `end` | What a wrong rule would say |
|---|---|---|
| `"a\n😀b"` | `(1,3)` | `(1,2)` for a char count, `(1,5)` for a byte count |

### M4. R4 is proved in one direction only — the server's own `Content-Length` is never asserted in bytes

> §2.1 (line 126): "`N` is the **UTF-8 byte** length of the body"
> §12.1 (line 1422): `frame_length_is_utf8_bytes` feeds a body containing `✓`
> and `😀`

That kills a reader that counts chars (the body is truncated → no reply). It
says nothing about the **writer**: every diagnostic `message` is ASCII by
design (§11.3), so every outgoing body is ASCII and a char-counting writer is
indistinguishable. MB.22.3's brief (bullet 1) even asks for "a non-ASCII
diagnostic message", which §11.3 forbids.

**Replacement:** `didOpen` with `uri = "file:///tmp/Blåstfile"` (the `uri` is
opaque and echoed, §2.5) and a broken body; read the raw `publishDiagnostics`
frame and assert the declared `Content-Length` equals
`body.as_bytes().len()` (the `å` is 2 bytes, 1 char). This is the only
non-ASCII value the server ever echoes.

### M5. §12.1 has no row for `-32600`, `-32601`, `-32602`, `-32002`, `window/workDoneProgress/create`, or the unknown-notification rule

> §2.4 (lines 187–195) lists seven codes; §12.1's table names only `-32700`.
> §2.2 (line 143): `window/workDoneProgress/create` → `null`, "a request with no
> reply can hang a client".

R7's proving tier (line 1540) says `critical_lsp.rs`, but four of its seven
codes and the whole tolerated-message table (§2.2) have no test. The
`workDoneProgress` row is the one the design itself says prevents a hang, and
nothing would catch its removal.

§2.4's `-32602` row is also self-contradictory: it lists "`didChange` with zero
or more than one change, or a change carrying `range`/`rangeLength`", but
`didChange` is a **notification** and the same row's parenthetical says "a
malformed notification is dropped". MB.22.3's brief (line 55) says "takes the
first full `contentChanges[].text`" — a third behaviour. Pick one (I recommend
"drop the notification") and test it.

**Add rows:** `error_codes_are_the_table` (`-32600` for `initialize` twice, for
`jsonrpc != "2.0"`, for a request after `shutdown`; `-32601` for an unknown
method and for a `$/…` **request**; `-32002` for a request before `initialize`;
`-32602` for `hover` without a numeric `position` and for `formatting` without
`uri`), `work_done_progress_is_answered`, `unknown_notification_is_dropped`
(no bytes on stdout, session continues).

### M6. R8 (`MAX_DOCUMENTS = 32`) has no test row

> §3.1 (line 237): "the 33rd `didOpen` publishes §2.5's notice"
> §2.5 (line 221): the exact JSON of the "tracks at most 32 open Blastfiles"
> diagnostic

A bound with no row in §12.1 and no line in §12.2. R8's tier (line 1540) claims
`critical_lsp.rs`.

**Add:** `open_documents_are_capped` — 33 `didOpen`s on distinct URIs; the 33rd
publishes exactly the §2.5 JSON at `(0,0)-(0,0)`, `severity:2`; the 32nd
document still answers a `hover` (the cap is a refusal, not a crash).

### M7. The §1.2 grep is promised three times and has no row; S3's "no `BLASTIT_*` variable is read" is already false, and its test is vacuous

> §1.2 (lines 99–102): "a grep assertion in `critical_lsp.rs`
> (`!src.contains("stage0")` over `include_str!("../src/lsp.rs")`)"
> §11.1 S3 (line 1372): "no `BLASTIT_*` variable is read; `/run`, `/proc/self`,
> uid checks untouched … `blastit lsp` runs to completion with
> `BLASTIT_UID_BASE=0` and an unreadable `/run`"

Three problems:

1. **No row.** §12.1 has no grep test. R3, S1 and S11 all point at it (lines
   1539, 1549). A cheap implementer writes §12.1's rows and nothing else.
2. **S3 is false at the process level.** `main.rs` calls
   `cli::main(Host::binary(), …)` — the argument is evaluated **before**
   dispatch, and `host.rs:176` reads `std::env::var_os("BLASTIT_EXEC")`
   (`host.rs:56`). So one `BLASTIT_*` variable *is* read on the `lsp` path.
   The §1.2 grep only constrains `lsp.rs`, so it cannot see this.
3. **The test is vacuous.** `BLASTIT_UID_BASE` is read by no code path at all
   (grep it), so setting it proves nothing about "no `BLASTIT_*` is read".

**Replacement:** (a) add the row `lsp_names_no_privilege_module` asserting
`include_str!("../src/lsp.rs")` contains none of `stage0`, `stage1`,
`crate::host`, `crate::config`, `std::env`, `blastit_sys`, `std::net`, and
contains `Manifest::parse_blastfile` and `blastfile::fmt` (the positive half of
S11); (b) replace S3's environment assertion with a real one — spawn the server
with `Command::new(bin).arg("lsp").env_clear()` and assert the full handshake
and one diagnostic still work. That kills any dependency on the environment,
including `Host::binary()`'s `BLASTIT_EXEC`.

### M8. D6's "600 refusals" fixture is unspecified and the obvious construction yields exactly **one** diagnostic

> §5.7 (line 592): "**D6:** a fixture with 600 refusals publishes exactly 512
> diagnostics, the last one at `(0,0)` with that message"
> §5.3 (line 483): `Manifest::parse_blastfile` → `Err(errors) => errors.errors()`

`parser::parse` (`blastfile/parser.rs:41`) returns on the **first** error:
`lex(...).map_err(ManifestErrors::from)?` and `parser.file().map_err(...)?` wrap
one error each. A file with 600 `body =` lines therefore publishes **1**
diagnostic, and the implementer's likely repair is to weaken
`MAX_DIAGNOSTICS`/the assertion. Only `validate` accumulates
(`validate.rs:74`), so the cap is reachable only through a validation fixture.

**Replacement:** state the fact in §5.7 and pin the fixture — 600 views, each
`use nosuchtrait`, e.g. `view v{n} "/x{n}" { use nosuchtrait\n body none }`
repeated 600 times (600 `UnknownGroup` errors, 2 400 tokens, far under
`MAX_TOKENS`). Assert: exactly 512 diagnostics, `[511].message == "89 more
problems; run \`blastit check\` for the full list"`, `[511].range` is
`(0,0)-(0,0)`, and the next `hover` answers.

### M9. D7 has no fixture, and `Manifest::default()` does not exist

> §5.5 (line 556): "for a fixture holding all six warnings … and
> `Manifest::warning_position` returns `Position::NONE` for a manifest built by
> `Manifest::default()` (the hand-built case), so a protected test's model keeps
> compiling and behaving"

`Manifest` is `#[derive(Clone, Debug, PartialEq, Eq)]` (`model.rs:713`) — no
`Default`, no `impl Default for Manifest`. The assertion cannot compile. And
"a fixture holding all six warnings" is not given, although producing
`UnattachedGroup` + `UngrantedProxy` + `UnrestrictedRoute` + `RawIngress` +
`EgressRoute` + `EntrypointNotReached` in one *clean* file is a non-trivial
construction (a named store granted nowhere, a `postgres unrestricted` grant on
the default store, a PHP entrypoint no view forwards to).

**Replacement:** `Manifest::from_blastfile_str("")` (verified to parse:
`model.rs:1388`) for the fallback case — its span table is empty, so
`warning_position(&ManifestWarning::RawIngress { route: "v".into() })` is
`Position::NONE`. And paste the six-warning fixture inline, with the line number
of each site in a comment, so `{"line":6,…}` has a referent.

### M10. Every validation/SQL error is `Position::NONE`, but §4.2 lists seven variants and MB.22.3 demands "a validation/SQL error's site"

> §4.2 (lines 354–357): "`Position::NONE` (whole-file refusals: `TooLarge`,
> `TooManyTokens`, `TooManyTrivia`, `TooManyRoutes`, `UidBaseTooHigh`,
> `NotRegularFile`, `Unreadable`) → `(0,0)`"
> §5.3 (lines 500–507): "The three families the CLI can produce are therefore
> all present: lexer/parser …, validate (whole-file rules and SQL grant rules:
> `UnknownGroup`, `GrantOnUnknownProxy`, `InvalidRegex`, `RedactedQueryPlaceholder`,
> `LiteralQuery`, `UnusableTable`, …)"
> `MB.22.3` acceptance bullet 7: "a test asserts … a validation/SQL error's site"

Eighteen variants carry `location: Location` (`error.rs:125` — a semantic
locator: `route "x" (index 1)`, `trait "y"`, never a line/col), plus the seven
above. `position()` (`error.rs:1402`) maps all of them to `NONE` through
`_ => Position::NONE` (`error.rs:1468`). So:

* the most common real manifest mistake — a SQL grant error — squiggles at
  `(0,0)`, not at the grant; §5.3 implies otherwise;
* §4.2's enumeration is short by ~18 variants;
* MB.22.3 bullet 7's "validation/SQL error's site" is unimplementable: there is
  no site to assert;
* §1 item 5 (line 77) calls `position()` "exhaustive over every `at`-carrying
  variant", and MB.22.3 bullet 7 asks for "no wildcard arm" — the wildcard is
  there today, and a Rust test cannot observe a missing match arm (only a
  source grep can).

**Replacement:** (a) fix §4.2 to say "every variant that carries `at`; every
`Location`-carrying variant and the seven whole-file ones are `NONE`", and list
the `Location` family; (b) change MB.22.3 bullet 7 to "a test asserts a
positioned syntax error's exact site, a `Location`-carrying validation error's
`(0,0)`, and a whole-file refusal's `(0,0)`"; (c) if exhaustiveness matters, add
`!include_str!("../src/error.rs").contains("_ => Position::NONE")` to the same
grep test as M7 — and delete the wildcard so the grep can pass.

### M11. §10.4's Q2–Q4 have no acceptance command anywhere

> §10.4 (line 1347): "**Assertion (Q1..Q4):** each of (a)–(d) is one command's
> assertion in `MB.22.4`'s acceptance block and one line of
> `scripts/check_zed_extension.py`'s report"

`MB.22.4`'s acceptance (lines 100–103) runs the four `query` commands and
asserts **nothing** about their output — only the exit status (Q1). Q2
(`variable.special` and `variable` on the `env_var` range), Q3 (`unrestricted`
as `constant`, `select` as `function`) and Q4 (one `item` per section, a `name`
for every named one) exist nowhere. `check_zed_extension.py` is a static
`tomllib`/regex guard; it cannot run `tree-sitter query`.

**Replacement:** add three assertions to MB.22.4's block, e.g.

```console
$ npx --yes tree-sitter-cli@0.25 query ../zed/languages/blastfile/highlights.scm test/fixtures/complete.Blastfile | grep -q 'variable.special'
$ npx --yes tree-sitter-cli@0.25 query ../zed/languages/blastfile/highlights.scm test/fixtures/complete.Blastfile | grep -q 'constant.*unrestricted'
$ npx --yes tree-sitter-cli@0.25 query ../zed/languages/blastfile/outline.scm test/fixtures/complete.Blastfile | grep -c 'item' | grep -qx 7
```

(and drop the claim that the static guard reports them).

### M12. R47 ("no false red") has no acceptance command, and the design's own command exits 1 on success

> §9.5 (line 1199):
> `$ npx --yes tree-sitter-cli@0.25 parse test/fixtures/complete.Blastfile | grep -c 'ERROR\|MISSING'   # must print 0`

`grep -c` prints `0` and exits **1** when there is no match. In a `set -e`
script or a Makefile prerequisite, success is a red gate. And the command is not
in MB.22.4's acceptance at all, so R47 (line 1547: "`tree-sitter generate/test/parse`")
has no owner.

**Replacement:** `$ test "$(npx --yes tree-sitter-cli@0.25 parse test/fixtures/complete.Blastfile | grep -cE 'ERROR|MISSING' || true)" -eq 0`, added to MB.22.4's block, and run over every `test/fixtures/**/*.Blastfile` (a loop), not just `complete.Blastfile`.

### M13. R56 is not proved to be in `make check`, and the guard's negative self-tests cover 7 of ~20 properties

> §12.4 (line 1479): "in `make check` via `make zed-check`"
> §12.4 (line 1491): "`scripts/test_check_zed_extension.py` removes **each
> property** from a temp tree and asserts a red run"
> MB.22.5 acceptance bullet 2 (line 102): seven named negative cases

`Makefile:769`'s `check:` prerequisite list does not contain `zed-check` today
and no acceptance command asserts it will. A target that exists but is never a
prerequisite is a gate that never runs. Separately, "each property" is ~20
(§8.2's nine rules, §8.4's six keys, §9.1's five files, §10.1's allowlist,
§8.3's six `lib.rs` rules, `Cargo.toml`, `.gitignore`, `.zed/settings.json`);
MB.22.5 pins seven.

**Replacement:** add `$ grep -Eq '^check:.*zed-check' Makefile` to MB.22.5's
acceptance, and either enumerate the remaining negative cases in MB.22.5 bullet
2 (`description` not longer than `name`; missing `language_ids`; `languages`
typo; `tree_sitter_blastfile` not exported; two `shell_env()` calls; `which`
order swapped; a `std::process` import; a `[capabilities]` block; committed
`node-types.json` deleted; `.zed/settings.json` missing) or change §12.4's
"each property" to "the seven properties MB.22.5 names".

### M14. MB.22.5's rev-fetch acceptance has a fallback that makes the proof vacuous

> MB.22.5 acceptance (line 85):
> `… && { git -C /tmp/opencode/grammar-fetch fetch -q --depth 1 origin "$REV" || git -C /tmp/opencode/grammar-fetch fetch -q origin release/0.2.0; } && git … checkout -q FETCH_HEAD && test -f …/parser.c`

If the by-SHA fetch fails — the exact failure mode the check exists to catch —
the `||` fetches the **branch tip** and checks out `FETCH_HEAD`, so `test -f`
passes and the gate is green. That is the design's §12.3 rule ("a branch, a tag
or `HEAD` is not a reproducible build input") defeated by its own acceptance.

**Replacement:** drop the fallback and keep the two independent assertions the
node already has — `git cat-file -e "$REV:editors/tree-sitter-blastfile/src/parser.c"`
(proves the object and path exist locally) — then make the shallow fetch a
separate, explicitly conditional line documented as "fails locally when
`uploadpack.allowReachableSHA1InWant` is off; the registry clone is the real
proof and `MB.22.5`'s `RESULT.md` records it". A gate that cannot fail is worse
than a documented manual step.

### M15. F6's `python/tests/test_docs_assets.py` claim is false, and no node may add it

> §6.5 (lines 713–715): "(`python/tests/test_docs_assets.py` asserts the
> documented line parses; `scripts/check_zed_extension.py` asserts the committed
> file, §12.4)"

`python/tests/test_docs_assets.py` guards the brand SVG, the PHP vocabulary and
the retired manifest name (its module docstring says so); it contains no `zed`,
`settings.json` or `format_on_save` reference. `docs/local.md:152`'s ```json```
fence is an "illustration" to `docs_fences.rs`/`fences.py` — only ```console```
fences are executed, so nothing parses it. And `python/tests/test_docs_assets.py`
is in **no** node's `paths` (MB.22.3: crates only; MB.22.4: `editors/**` +
`.zed/settings.json`; MB.22.5: `editors/zed/extension.toml`, `scripts/check_*`,
`Makefile`, `python/tests/critical/test_lsp_stdio.py`), so the promised
assertion can never land.

**Replacement:** move the assertion to `scripts/check_zed_extension.py` (which
MB.22.5 owns): read `docs/local.md`, extract the ```json``` fence under
`## Editors`, `json.loads` it, and assert
`["languages"]["Blastfile"]["format_on_save"] == "on"`; add the negative
self-test ("change `on` to `off` → red"). Fix R29's tier line (1544) to name the
guard instead of the docs-assets test.

### M16. S6's `VmHWM` oracle reads the wrong process and races the exit

> §11.1 S6 (line 1375): "`RSS` growth < 1 MiB (measured with
> `/proc/self/status`'s `VmHWM` before/after)"

`/proc/self` in `critical_lsp.rs` is the **test** process. The server is a
child (§12.1 line 1415), and §3.4 row 2 makes it `exit 1` immediately on
`N > MAX_BODY_BYTES` — so by the time the test wants the "after" reading,
`/proc/<child>` is gone. As written the assertion is either about the wrong
process or unreadable.

**Replacement:** run two children — a baseline that only handshakes and exits,
and the over-cap one — and compare `wait4`/`getrusage(RUSAGE_CHILDREN).ru_maxrss`
(the max over reaped children, so run them sequentially and take the delta), or
poll `/proc/<child_pid>/status` `VmHWM` in a loop until `waitpid` returns and
assert `delta < 1 MiB`. Either way, name the pid explicitly in §11.1.

### M17. §7.4 and §2.2 disagree about `textDocument/definition`, and neither is tested

> §7.4 (line 798): "`textDocument/definition` is answered `null` in v1 and
> **not advertised**"
> §2.2 (line 153): "any other string | request or notification | — | request ⇒
> `-32601`"

`textDocument/definition` is not in §2.2's table, so by §2.2 it answers
`-32601`. R33/R34's tier (line 1545) claims `critical_lsp.rs`, but §12.1 has no
`definition` row. Whichever behaviour the implementer picks, nothing pins it,
and a client that probes an unadvertised method gets a protocol error instead of
"declined".

**Replacement:** add `textDocument/definition` to §2.2's table with the chosen
behaviour (`null` is friendlier: a request for an unadvertised capability is not
a protocol violation), and add the row
`definition_is_declined_and_unadvertised` — `result` is `null`, and the
capability document has no `definitionProvider` key.

### M18. §12.5's Python session asserts "one formatting edit" on a **broken** fixture

> §12.5 (lines 1501–1504): "sends the **same** session as §12.1's handshake —
> `initialize` → `didOpen` the `# ✓ ok` / `body =` fixture → one diagnostic …
> → **one formatting edit** → `shutdown`/`exit 0`"

`# ✓ ok\nbody =` is a parse error (`Equals`), so §6.1's `Err(_) → null` applies:
the formatting reply is `null`, never an edit. The protected Python test as
described cannot pass, and the likely repair is to weaken the assertion to
"a reply arrived", which would then pass for a `-32603` too.

**Replacement:** two documents in one session — `didOpen` the broken
`# ✓ ok\nbody =` (assert the diagnostic), then `didChange` to the canonical
`app "shop"\n\nview v "/x" {\n    body json\n}\n`, then `formatting` → `[]`
(equal ⇒ no edit), then `didChange` to a mis-indented variant → exactly one
`TextEdit` whose `end` is `(5,0)`. That exercises all three §6.1 outcomes in the
protected suite.

---

## MINOR

### m1. P1's `# ✓ ok` fixture cannot prove the UTF-16 unit, but two places say it does

> §12.5 (line 1502): "the `# ✓ ok` / `body =` fixture"; MB.22.3 acceptance
> bullet 3 (line 109): "fixture line `# ✓ ok` then `body =` proves the unit"

The `✓` is on line 1; the `Equals` position is on line 2. `point()` never
converts a character on line 1, so P1 proves only the 0-based line and `col-1`
(its own §4.5 label says exactly that). P2 is the unit oracle. Change both
sentences to "proves the 0-based line and `col-1`; P2 proves the unit", and use
P2's fixture in the Python test (see M18).

### m2. P4's fixture is arithmetically impossible

> §4.5 P4 (line 409): "a document of exactly `16_777_217` bytes of `# x\n`
> followed by `app`"

`4 194 304 × 4 = 16 777 216`, `+ 3 = 16 777 219`; `16 777 217 − 3` is not a
multiple of 4. Use `"# x\n"` × 4 194 304 followed by `"a"` (exactly 16 777 217
bytes), and note that `parser.rs:42` refuses it before lexing, so no trivia cap
interacts.

### m3. §5.4's `EntrypointNotReached` sentence drops a pair of backticks

> §5.4 (line 522): "`entrypoint php "…" is reached by no view: declare a view
> that forwards to it`"
> `error.rs:1539`: "… declare a \`view\` that forwards to it"

§5.4 says "The sentence (`Display`, unchanged)". Restore the backticks, or a
future `D8`-style byte comparison fails.

### m4. §6.2's "wrong rule" column conflates "last line" with "last non-empty line"

> §6.2 (line 640): "`"a\n"` | `(1,0)` | `(0,1)` for "last line"; the final
> newline would survive"

`line_count - 1` equals the `'\n'` count for every text, so "last line" is
**not** a wrong rule — it is the same rule. `(0,1)` is the "last non-empty
line" answer. Relabel the column, and keep the row: it is the one that kills a
`trim_end()` implementation, which is the real bug.

### m5. R9's `MAX_BODY_BYTES == 33_619_968` literal has no named test

> §3.1 (line 236): "asserted as a literal so a change to the parser cap is a
> test failure"

§12.2 has a row for `MAX_SOURCE_BYTES == 16_777_216` but none for
`MAX_BODY_BYTES`. Add `assert_eq!(MAX_BODY_BYTES, 33_619_968)` to `lsp.rs`'s
`mod tests` and name it in §12.1's unit-test sentence (line 1444).

### m6. `ids_echo_verbatim` omits the `null` id

> §2.2 (line 135): "a JSON `null` id is treated as absent (a notification)"

Add a fourth case: `"id": null` → no reply at all.

### m7. `exit_codes` omits "EOF after `shutdown`" → `0`

> §3.4 row 5 (line 296): "exit `0` iff `shutdown` was received"

The five sessions are clean → 0, EOF mid-session → 1, over-cap → 1, `exit`
without `shutdown` → 1, `--bogus` → 2. The `shutdown`-then-EOF path (the one
that proves the flag, not the `exit` message, drives the code) is untested. Add
it.

### m8. H6's bounds are not assigned to any tier

> §7.5 H6 (line 827): "≤ 48 rows, sorted, no duplicate key, every value ≤ 600
> bytes, every key present in `docs/reference/blastfile.md`"

§12.1's unit-test sentence (line 1444) names only "sortedness/duplicates". The
row cap, the 600-byte cap and the transcription check have no owner. Add
`hover_table_is_bounded_and_transcribed` to `lsp.rs`'s `mod tests` (the
`include_str!("../../../docs/reference/blastfile.md")` path is correct from
`crates/blastit/tests/` and from `crates/blastit/src/`; from
`crates/blastit-manifest/tests/` it is also correct — pick one and state it).

### m9. Hover *wording* in the protected tier makes a docs edit a `critical_guard` conflict

H1/H3/H4 assert substrings of `HOVER_TABLE` values (`"any statement"`,
`"stays in the environment"`, the `view` sentence). A later wording fix in
`docs/reference/blastfile.md` then requires a `critical_guard` authorization for
a protected file — the exact friction §5.5 avoids for `ManifestWarning`. Keep
the *contract* in `critical_lsp.rs` (`result` non-null, `range` arithmetic,
`$IDENT` interpolation) and move the sentence assertions to `lsp.rs`'s
`mod tests`, where the table lives.

### m10. H1 asserts a substring the pinned value does not contain

> §7.5 H1 (line 822): "markdown contains **"any statement"** and "warns at boot""
> §7.3 (line 754): "`postgres unrestricted` allows **every** statement on that
> store"

"any statement" is not a substring of "allows every statement". H1 fails against
the design's own table. Either assert `"every statement"` or change the table to
"any statement" (the boot warning says "may run any statement"), and then make
H1's two strings the same two strings the table pins.

### m11. The Python route's precondition is undocumented

`python/blastit/__main__.py` → `_blastit.main` → `blastit-py/src/lib.rs:62`
refuses when `blastit/_bin/blastit-dbproxy` is missing, **before** dispatch. So
`python -m blastit lsp` from a bare `python/` checkout fails even though the LSP
never uses the dbproxy. §12.5's "spawned … under `test-critical`'s staged
`PYTHONPATH`" is the only reason it works. Say so in §12.5 (one sentence), so a
future reader does not "simplify" the test to run from the source tree.

### m12. R59's tier says "the commands quoted there" — there are none

> Requirement index (line 1550): "R53–R59 | §12 | the commands quoted there"
> §12.7 is a table of preconditions, not commands.

Say "human-only; recorded in `MB.22.5`'s `RESULT.md`" so the index does not
promise an automated command that cannot exist.

---

## Fixture arithmetic, verified by hand

| Claim | Design says | Hand-computed | Verdict |
|---|---|---|---|
| P1 `"# ✓ ok\nbody =\n"`, `Equals` at (2,6) | `(1,5)-(1,6)` | line 1 = `body =`, chars before col 6 = 5, all ASCII → `(1,5)-(1,6)` | ✅ |
| P2 `view v "/😀" { body = }` | start `character` 20 | `=` is char 19; 19 chars before, `😀` = 2 units → **20** | ✅ (char bug 19, byte bug 22) |
| P3 `view v "/é" { body = }` | start `(0,19)`, `col-1` = 19 | `=` is char 19; `é` = 1 unit → **19** | ✅ (non-discriminating, as stated) |
| P4 "exactly 16 777 217 bytes of `# x\n` + `app`" | 16 777 217 | 4 194 304×4 = 16 777 216; +3 = 16 777 219 | ❌ see m2 |
| P5 `viewx "x"` | `(0,0)-(0,5)` | 5 word chars, width 5 | ✅ (error variant unnamed — name `Unexpected`) |
| F1 `""` / `"a\nb"` / `"a\n"` / 4-line | `(0,0)` / `(1,1)` / `(1,0)` / `(3,0)` | same | ✅ (all ASCII → M3) |
| F5 CRLF fixture | `end` `(4,0)` | 5 CRLF → **5** `\n` → `(5,0)` | ❌ see M2 |
| `MAX_BODY_BYTES` | 33 619 968 | 2×16 777 216 + 65 536 = **33 619 968** | ✅ |
| over-cap probe | 33 619 969 | one over | ✅ |
| header cap probe | 8 193 | one over 8 192 | ✅ |
| D6 notice | 600 → 512, "89 more" | 600−511 = **89**, 511+1 = 512 | ✅ (fixture impossible via parse — M8) |
| H4 `my_view` | 7 units | 7 | ✅ |
| H5 `body` start | "at 17" | `body` starts at unit **15**; 17 = `d`; 18 = `y` | ❌ see B2 |

Repo claims spot-checked and **correct**: `MAX_SOURCE_BYTES` is `pub(crate)` and
`16_777_216` (`blastfile/mod.rs:202`); `DEFAULT_BASE = 20_000` (`uid.rs:32`);
`fmt` public (`print.rs:50`) and `a_crlf_file_is_written_with_lf`
(`print.rs:739`); `route_lines` (`model.rs:916`); `report_manifest_errors`
(`cli.rs:554`); `position()` (`error.rs:1402`); six `ManifestWarning` variants;
`tests/validate.rs` and `critical_manifest_011.rs` exist and construct
`RawIngress`/`EgressRoute` by struct literal; `# ✓ ok` is a legal comment
(`comment_run`, `lexer.rs:366`) and `advance` counts chars (`lexer.rs:180`);
`[[bin]] name = "blastit"` so `CARGO_BIN_EXE_blastit` works; `test-critical`
stages `_bin/blastit-dbproxy`; `critical_guard.py` accepts `--base`/`--node`;
every `HOVER_TABLE` key does appear in `docs/reference/blastfile.md`.

## Requirement → test mapping, gaps only

| R/S | § | Named test | Status |
|---|---|---|---|
| R1 | §1 | — | `--help` exits 0 + first line `usage: blastit lsp` promised (line 93), no row |
| R2 | §1 | — | "no dependency" has no test (`Cargo.lock` diff is a node gate, not a test) |
| R3, S1, S11 | §1.2, §11.1 | — | grep promised three times, no row (M7) |
| R7 | §2.4 | `malformed_body_answers_parse_error_and_serves` | only `-32700` (M1, M5) |
| R8 | §2.5 | — | no row (M6) |
| R9 | §3.1 | — | `MAX_BODY_BYTES` literal unasserted (m5) |
| R28 | §6.4 | — | `uri` required → `-32602`, `options` ignored: no row (M5) |
| R31 | §7.5 | `_hover…` | H6 bounds/transcription unassigned (m8) |
| R33, R34 | §7.4 | — | definition `null` vs `-32601`, no row (M17) |
| R47 | §9.5 | — | no acceptance command (M12) |
| R52 | §10.4 | four `query` runs | exit status only; Q2–Q4 unowned (M11) |
| R56 | §12.4 | `make zed-check` | not proved to be in `check` (M13) |
| R59 | §12.7 | — | human-only, index says "commands" (m12) |
| S2 | §11.1 | — | no row, oracle cannot fail (B1) |
| S3 | §11.1 | — | vacuous env assertion, property already violated (M7) |
| S6 | §11.1 | `oversized_body_is_refused_before_allocation` | wrong pid, races exit (M16) |

VERDICT: FAIL

## Round 2

Fresh eyes again: round 1's file, the revised `docs/design/blastfile-lsp-zed.md`
(1 947 lines), `ADR-0013`, and the nodes the design hands off to (`MB.22.3`,
`MB.22.4`, `MB.22.5`, plus `MB.22.6`'s audit brief). Every literal the revision
introduces was recomputed by hand, and every code site an assertion names was
re-read. Nothing was built; no file but this one was touched.

The revision is a real improvement: **B1, B2 and fifteen of the eighteen MAJORs
now have a named row with an oracle that fails when the property is deleted.**
But the document is **truncated at §12.5**: that section has a header and a table
header and no rows, and §12.6, §12.7 and §12.8 do not exist — while eight places
cite §12.8 as the register of every hand-off. Every plan-level defect I named is
therefore still in the plan nodes, unrecorded. Two new assertions cannot compile
(`include_str!` paths), one new protected fixture cannot produce the count it
asserts (D7), and two of the three new §10.4 greps cannot match the pinned
fixture.

**Counts: 2 BLOCKER, 12 MAJOR, 7 MINOR.**

### Round-1 ids

| id | Status | Evidence, or the exact remaining gap |
|---|---|---|
| B1 | **RESOLVED** | §12.1 line 1750: "`a_document_is_never_executed` \| S2's `LiteralQuery` fixture, empty stderr, and `/proc/<server_pid>/task/<tid>/children` empty before and after \| S2". The oracle is now the server's pid and a real `children` file, and S2 (line 1657) says "exactly the `LiteralQuery` diagnostic". `validate.rs:284-292` confirms the quoted literal is the only error. (The fixture text is still not written out; the implementer must not add a second error to it.) |
| B2 | **RESOLVED** | §7.5 H5 (line 1065): `"😀 view"` at UTF-16 `character` **6**. Recomputed: correct `char_index` → 5 (`w`) → `view`; `character as usize` → 6 → past the 6-char line → `null`. Discriminating. `character` 20 keeps the punctuation case (both paths `null`, as stated). |
| B3 | **OPEN** | §12.8 does not exist (see N1), and `MB.22.3`'s acceptance is unchanged: `… \| cargo run -q -p blastit -- lsp; test $? -eq 1`. `cli.rs:53` still falls through to `Serve`, and `parse` still answers "``lsp`` is not `--key=value`" → exit 1, so the command still passes with the `Lsp` arm deleted. |
| M1 | **RESOLVED** | §12.1 line 1724: "`{` → `-32700`; invalid UTF-8 → `-32700`; `[]` → `-32600`; then a valid `hover` answers". Matches §2.4 and §3.4 row 4. |
| M2 | **RESOLVED** | §6.3 F5(d) (line 906): "`end` is `(5,0)` (the fixture has five `\n`)". Recounted: five terminators. |
| M3 | **RESOLVED** | §6.2's fifth row (line 866): `"a\n😀b"` → `(1,3)`, "the only oracle for `end.character`'s UTF-16 rule", asserted by F1 in `lsp.rs`'s `mod tests`. |
| M4 | **RESOLVED** | §12.1 line 1722: `frame_length_is_utf8_bytes` uses `uri = file:///tmp/Blåstfile` and asserts "the raw `publishDiagnostics` frame's declared `Content-Length` equals `body.as_bytes().len()`". The uri is the one non-ASCII value the server echoes. |
| M5 | **OPEN** | The code rows landed (`error_codes_are_the_table`, line 1725; `work_done_progress_is_answered`; `unknown_notification_is_dropped`), but §2.4 line 244 still assigns `-32602` to "`didChange` with **zero or more than one** change object" — a **notification**, which the same row's parenthetical says is "dropped". No row tests it, and `MB.22.3`'s brief still says "takes the first full `contentChanges[].text`". Three behaviours, no oracle. |
| M6 | **RESOLVED** | §12.1 line 1738: `open_documents_are_capped` — "the 33rd gets the §2.5 notice at `(0,0)-(0,0)`, severity 2, and the 32nd still answers a `hover`". |
| M7 | **RESOLVED** | §12.1 line 1748 `the_lsp_module_is_a_text_tool` (the §11.1 list, plus `blastfile::check`/`blastfile::fmt`), and S3(b) line 1658: a session spawned with `.env_clear()` completes the handshake and one diagnostic. `Host::binary()` tolerates a missing `BLASTIT_EXEC` (`host.rs:176-180`), so the oracle is sound. |
| M8 | **RESOLVED** | §5.7 lines 812-818 pin the fixture (`view v{n} "/x{n}" { use nosuchtrait\n body none }`, 600 times), the reason (`parser.rs:41` returns on the first error; `validate.rs:74` accumulates), and the exact 512 / 89 / `(0,0)` assertions; §12.1 line 1737 names the row. |
| M9 | **OPEN** | The `Manifest::default()` defect is gone, but **D7's fixture yields five warnings, not six**. `EntrypointNotReached` is emitted only `if let Some(php) = manifest.php() && manifest.routes().is_empty()` (`validate.rs:1035-1041`); the fixture has three views. The repo's own `critical_blastfile_parity.rs:1129-1190` says so: "the sixth (`EntrypointNotReached`) from its own file". And line 3's comment `# UngrantedProxy (db)` cannot fire — `view wide { postgres unrestricted }` grants `db` (`validate.rs:984-995`). So §5.5's "six diagnostics at six different lines", §12.1's `warning_sites_are_their_declarations` ("six distinct lines") and §12.2's "the six sites" all fail against a correct implementation. Fix: two fixtures (five + `entrypoint php …` alone), and drop the line-3 comment. |
| M10 | **OPEN** | §4.2 now lists the `Location` family and §5.3 says they land at `(0,0)` — good. But (a) §1 lines 114-117 still say "the explicit list of the **seven** position-less variants", which cannot compile: the wildcard also covers ~18 `Location` variants plus `InvalidApp`, `DuplicateRouteName`, `InvalidRouteName`, `EmptyPath`, `InvalidMethod`, `EmptyMethods`…; (b) `MB.22.3` bullet 7 still asks for "a validation/SQL error's **site**", unchanged, and §12.8 (which was to carry the correction) does not exist. |
| M11 | **OPEN** | Three greps were added (§10.4 lines 1621-1623), but two cannot match the pinned fixture. (c) `grep -q 'constant.*unrestricted'`: `complete.Blastfile` is "the reference page's full example" (§9.5), and that example (`docs/reference/blastfile.md:506-551`) contains **no** `unrestricted`. (d) `grep -c 'item' \| grep -qx 6`: the same example has **eight** section nodes (`app`, `entrypoint`, 2 × `postgres`, `trait`, `static`, 2 × `view`), so `@item` matches eight times. `MB.22.4`'s acceptance is unchanged (still `npx --yes tree-sitter-cli@0.25 …`, no greps). |
| M12 | **RESOLVED** | §9.5 lines 1473-1476: the loop over `test/fixtures/*.Blastfile` with `test "$(… \| grep -cE 'ERROR\|MISSING' \|\| true)" -eq 0 \|\| exit 1`. `grep -c`'s exit 1 on no match is neutralised, and every fixture is covered. |
| M13 | **OPEN** | §12.4 states the `zed-check` prerequisite and names twelve negative cases, but no command asserts the Makefile line, and `MB.22.5` bullet 2 still names **seven** cases. §12.8 (the register) is absent. |
| M14 | **OPEN** | The `\|\|` fallback is gone from §12.3 — but the by-SHA fetch is still an unconditional acceptance line, and it fails on the documented local server (`file://$PWD` with `uploadpack.allowReachableSHA1InWant` off). A gate that fails by design is not green; `MB.22.5`'s acceptance still carries the old fallback verbatim. Fix: `git -c uploadpack.allowReachableSHA1InWant=true -C … fetch …` (the client's `-c` reaches the spawned `upload-pack`), or move the fetch out of the acceptance block into the recorded manual step. |
| M15 | **RESOLVED** | §6.5 + §12.4: F6 is the guard's, and the Requirement index (line 1840) names the guard for R29. `docs/local.md:147-149` has exactly one json fence under `## Editors`, with the same object. |
| M16 | **OPEN** | `oversized_body_is_refused_before_allocation` (line 1728) now asserts `Content-Length: 33619969` → `-32700`, exit 1, 2 s. That proves "refused **before reading**" (a reader would hit EOF and emit "ended inside a frame", not `-32700`) but **not** "before allocating": `vec![0u8; 33_619_969]` then the cap check still answers `-32700`. The discriminating probe is `Content-Length: 4294967295`, which is not tested; §11.1 S6 concedes "asserted by review and by `MAX_FRAME_BODY_BYTES == 33_619_968`". |
| M17 | **RESOLVED** | §2.2 line 194 (`textDocument/definition` → `null`, "declined"), §7.4, and §12.1 line 1745 `definition_is_declined_and_unadvertised`. |
| M18 | **OPEN** | §12.5 has a header, a table header (`Path | How it is driven`) and **no rows**. The three-document Python session the Review Resolution says landed there does not exist. |
| m1 | RESOLVED | §4.5 P1: "**not** the unit: `✓` is on line 1, which `point()` never converts"; P2 is "the only unit oracle". |
| m2 | RESOLVED | §4.5 P4: `"# x\n"` × 4 194 304 + `"a"` = 16 777 217 bytes. Recomputed: 4 194 304 × 4 = 16 777 216, + 1. |
| m3 | RESOLVED | §5.4 restores the backticks: "declare a `view` that forwards to it" (`error.rs:1539`). |
| m4 | **partial** | The table's column is relabelled ("`(0,1)` for 'last non-empty line'"), but §6.2 line 852 still says "**not** `line_count - 1`" — see N7. |
| m5 | RESOLVED | §12.1's closing sentence: "`assert_eq!(MAX_FRAME_BODY_BYTES, 33_619_968)`" in `lsp.rs`'s `mod tests`. |
| m6 | RESOLVED | §12.1 line 1723: "explicit `null` → `-32600` with `id: null`". |
| m7 | RESOLVED | §12.1 line 1754: "S10's six sessions, including `shutdown`-then-EOF → `0`". |
| m8 | RESOLVED | §7.5 H6 is a unit test and §12.1 names it: `hover_table_is_bounded_and_transcribed`. (Its `include_str!` path is wrong — N3.) |
| m9 | RESOLVED | §7.5's preamble moves wording to `lsp.rs`'s `mod tests`; H1-H5 keep only the contract. |
| m10 | RESOLVED | H6 asserts "`unrestricted`'s value contains `every statement` and `warns at boot`", and §7.3's value contains both. |
| m11 | **OPEN** | §12.5 is empty, so the dbproxy precondition (`blastit-py/src/lib.rs:62-68` refuses before dispatch) is still undocumented. |
| m12 | **partial** | The index now says "§12.6–§12.7 are human-only and recorded in `MB.22.5`'s `RESULT.md`" — but neither section exists, and §12.5 has no commands to point at. |

### New findings

#### N1. BLOCKER — §12.5 is empty and §12.6–§12.8 do not exist, so every hand-off is unrecorded

> §12 (line 1710): "Rows marked *hand-off* belong to a sibling node's acceptance
> block; §12.8 lists them because `MB.22.2` may not edit another node's `NODE.md`."
> §10.4 (line 1634): "(§12.8 records that hand-off)".
> Requirement index (line 1845): "the commands of §12.1–§12.5; §12.6–§12.7 are
> human-only".

`grep '^### 12'` ends at `12.5`; the file is 1 947 lines and stops after the
Review Resolution. §12.5's body is one table header and nothing else. Consequences:

* **Six hand-offs have no register**: B3 and M10 → `MB.22.3`; M11/M12 → `MB.22.4`;
  M13/M14/F6 → `MB.22.5`; m3 → `docs/reference/cli.md`. All five node files are
  byte-identical to round 1 (I diffed the acceptance blocks), so every one of
  those defects is still live in the plan.
* **One hand-off has no owner at all**: m3 targets `docs/reference/cli.md`, whose
  only owner, `MB.22.1`, is `status: done`. The `2` exit-code row cannot land
  without a new node or a `plan.py human add`.
* **`MB.22.6` repeats a claim the design now contradicts**: its brief says the
  `# ✓ ok` fixture proves "the UTF-16" unit, while §4.5 P1 says it proves only
  the 0-based line and `col-1`.
* **R53–R59 have no commands**, and M18's fix (the two-document Python session)
  has no home.

The *targets* the Review Resolution names are nevertheless the right nodes:
`MB.22.3` owns `cli.rs`/`critical_lsp.rs`, `MB.22.4` owns `editors/**` +
`.zed/settings.json`, `MB.22.5` owns the guard, the `Makefile` and
`python/tests/critical/test_lsp_stdio.py`. What is missing is the table itself.
Write §12.5 (both distribution paths, with m11's dbproxy precondition), §12.6
(the Zed dev-install pass), §12.7 (registry publish), and §12.8 as
`id → node → the exact line to add to that node's # Acceptance`.

#### N2. MAJOR — H4 asserts a hover value §7.2 forbids, so the protected test cannot pass

> §7.5 H4 (line 1064): "`view my_view "/x"` hover on `my` | the word is
> `my_view`: value is `HOVER_TABLE["view"]` verbatim, `range` covers 7 units"
> §7.2 (lines 965-969): "in `HOVER_TABLE` | its markdown value … anything else |
> `null`"

`my_view` is not a key, and §7.1 expands the word to `my_view`, so §7.2's answer
is `null`. The test as written fails against a correct server; the cheap repair is
to weaken it to "a reply arrived", which then passes for any word. Fix: hover the
`view` **keyword** (word `view`, 4 units) for the value, and keep `my_view` for the
7-unit range with `result: null` — that is the word-expansion oracle H4 exists for.

#### N3. MAJOR — two `include_str!` paths cannot compile

1. §12.1 line 1749: `include_str!("../src/error.rs")` from
   `crates/blastit/tests/critical_lsp.rs` resolves to `crates/blastit/src/error.rs`,
   which does not exist (`ls crates/blastit/src/` has no `error.rs`). The file is
   `crates/blastit-manifest/src/error.rs`; the path is
   `../../blastit-manifest/src/error.rs`.
2. §7.2 line 977 and §7.5 H6 line 1066:
   `include_str!("../../../../docs/reference/blastfile.md")` from
   `crates/blastit/src/lsp.rs` resolves **one level above the repository**
   (`realpath -m` gives `.worktrees/docs/reference/blastfile.md`, absent). Three
   `..` is correct — `crates/blastit/src` is three levels below the root.

These are the only oracles for R3a and R31, and both are inside protected files.

#### N4. MAJOR — `sites()` is `pub(crate)` but the test that calls it is an integration test

> §5.5 (line 711): "`pub(crate) fn sites(cst: &Cst, source: &str, warnings: &[ManifestWarning]) -> Vec<Position>`"
> §5.5 (line 781): "`sites` for a hand-written `&[ManifestWarning::RawIngress { route: "v".into() }]` against an empty tree is `[Position::NONE]` — the `crates/blastit-manifest/tests/warning_sites.rs` unit test asserts both"
> §12.2 (line 1769): "**new** `tests/warning_sites.rs`"

`crates/blastit-manifest/tests/warning_sites.rs` is a separate crate: it can call
the public `check()`, but not a `pub(crate)` function, and `check()` cannot be
handed a synthetic warning. The `[Position::NONE]` fallback has no home as placed.
Fix: say the fallback case is a `mod tests` unit test inside `blastfile/sites.rs`,
and keep the six-site assertion in `tests/warning_sites.rs` through `check()`.

#### N5. MAJOR — the panic hook's stderr has no behavioural oracle

> §3.3 (lines 379-386): the hook exists because "a `ManifestError`'s `Debug`
> carries quoted document text", so "the hook is replaced *before* any document
> is parsed"
> §11.1 S5 (line 1660): "(a) `include_str!` contains `set_hook` and
> `catch_unwind`; (b) 2 000 framed bodies … leave the process alive"
> §11.1 S8 (line 1663): "a fixture whose SQL is `SELECT secret_token FROM api_keys`:
> stderr contains none of …"

S5 asserts the source text and liveness, not what the hook *wrote*; S8's fixture is
a **valid** document, so its session writes zero bytes and every "contains none of"
assertion passes vacuously. A hook that printed the payload would satisfy both.
Fix: in `fuzz_shaped_bodies_do_not_kill_the_loop`, assert every stderr line is one
of §11.3's four literals and that no line contains a fixture's document text.

#### N6. MINOR — §3.1's `HEADROOM` arithmetic is off by the envelope

`2 × 16 777 216 + 65_536 = 33 619 968`, and the row says the slack is "the JSON-RPC
envelope plus a `uri` of up to 64 KiB". The envelope
(`{"jsonrpc":"2.0","id":1,"method":"textDocument/didOpen","params":…}`) is ~110
bytes, so a 64 KiB uri overflows by that much. Say "64 KiB − the envelope", or make
`HEADROOM` 66 560.

#### N7. MINOR — §6.2's "not `line_count - 1`" is false

For every text, `split('\n').count() - 1` equals the number of `'\n'` (`""` → 0,
`"a\n"` → 1, `"a\nb"` → 1). The relabelled table column is right; the prose still
names a rule that is the same rule. Delete the clause or replace it with the rule
that really differs ("the last non-empty line").

#### N8. MINOR — R2's tier names the wrong oracle

Requirement index (line 1835): "`cargo clippy -D warnings` for 'no new dependency'".
Clippy passes with a new dependency. The real oracle is already in `MB.22.3`'s
acceptance: `cargo test -p blastit --locked` (a new dependency changes `Cargo.lock`
and `--locked` fails). Name that.

#### N9. MINOR — P6's 250 ms absolute bound is a flaky protected test

§4.5 P6 (line 564) puts a wall-clock bound in `critical_lsp.rs`: "each
`didChange`→publish round trip completes in < 250 ms". Parsing 16 MiB, validating
512 refusals and serialising 512 diagnostics can exceed that on a loaded runner for
a correct implementation, and relaxing a protected test needs `critical_guard`
authorisation. Keep the 2× ratio against the 1-diagnostic baseline (that is what
catches the O(n·m) rescan) and drop or raise the absolute bound.

#### N10. MINOR — §2.4's "every code in the table above is asserted by a real session" is false for `-32603` and `-32800`

Both rows say "never emitted", so no session can assert them. Say "every code that
can be emitted", or add `!include_str!("../src/lsp.rs").contains("32603")` to the
same grep test as N3.

#### N11. MINOR — §12.3 does not assert the second half of its own rule

The rule is "the commit contains **both** `editors/tree-sitter-blastfile` and
`editors/zed`", but the only `cat-file -e` checks `parser.c`. Add
`git cat-file -e "$REV:editors/zed/extension.toml"`.

#### N12. MINOR — `warning_display_is_unchanged` duplicates an existing oracle

§12.1 line 1735 asserts "`ManifestWarning`'s `Display` for all six … matches
today's boot text byte for byte". `critical_blastfile_parity.rs:1169-1189` already
pins all six sentences verbatim, and §12.2's "the existing tests pass unchanged"
is the stronger proof. The new row adds `critical_guard` friction without coverage.

### Literals recomputed

| Literal | Design says | Recomputed | Verdict |
|---|---|---|---|
| §6.2 five `end` rows | `(0,0)`, `(1,1)`, `(1,0)`, `(4,0)`, `(1,3)` | same | ✅ |
| §6.3 F5(d) | `(5,0)` | five `\n` in the fixture | ✅ |
| P1 `Equals` | `(1,5)-(1,6)` | line 1 = `body =`, col 6, ASCII | ✅ |
| P2 `=` start | `character` 20 | 19 chars before, `😀` = 2 units | ✅ |
| H5 at 6 / 20 | `view` / `null` | `w` at unit 6; cast → index 6 → `null` | ✅ |
| D6 notice | 600 → 512, "89 more" | 600 − 511 = 89; 511 + 1 = 512 | ✅ |
| `MAX_FRAME_BODY_BYTES` | 33 619 968 | 2 × 16 777 216 + 65 536 | ✅ |
| over-cap probe | 33 619 969 | one over | ✅ |
| header cap | 8 192 / 8 193 | one over | ✅ |
| P4 fixture | 16 777 217 bytes | 4 194 304 × 4 + 1 | ✅ |
| D7 warnings | six, six lines | **five** (no `EntrypointNotReached`; line 3 cannot fire) | ❌ M9 |
| §10.4 `@item` | 6 | **8** sections in the pinned example | ❌ M11 |
| §10.4 `constant.*unrestricted` | matches | the pinned example has no `unrestricted` | ❌ M11 |
| `HEADROOM` | 65 536 ⊇ 64 KiB uri + envelope | 65 536 − ~110 | ❌ N6 |
| §7.2 `include_str!` | four `..` | three | ❌ N3 |
| §12.1 `include_str!` error.rs | `../src/error.rs` | `../../blastit-manifest/src/error.rs` | ❌ N3 |

### Verdict

Two BLOCKERs (B3, N1) and twelve MAJORs (M5, M9, M10, M11, M13, M14, M16, M18,
N2, N3, N4, N5) remain open. The design's testing matrix is strong where it landed
— B1, B2 and fifteen MAJORs now have rows with teeth — but the document is missing
the section that makes its hand-offs executable, one new fixture cannot produce
its own count, and two new assertions do not compile.

VERDICT: FAIL

## Round 3

Fresh eyes on the revision whose md5 is `ce4d1e2a7f33eee7ab0c9f67ecd2ce79`
(2 080 lines; it grew by 44 lines while I was reading — the growth is in the Review
Resolution, not in §4–§12). I re-read §4.5, §5.5, §5.7, §6, §10, §11 and §12 in full.

This round I stopped trusting arithmetic and **ran the fixtures**. Two throwaways, nothing
in the repository touched:

* `/tmp/opencode/warnprobe` — a crate with a `path` dependency on
  `crates/blastit-manifest`, calling `Manifest::parse_blastfile(src, 20_000)` and printing
  `warnings` (or `errors`) for each fixture below;
* `target/debug/blastit fmt --check` (already built) on P1's fixture and on its
  view-wrapped variant;
* the §12.3 by-SHA fetch over `file://$PWD` on git 2.47.3.

The truncation is gone and the D7 rewrite is a genuine improvement (per-variant fragments
instead of a hand-counted total). Three fixtures/assertions still cannot hold, and four
round-2 MAJORs are unchanged.

**Counts: 0 BLOCKER, 8 MAJOR, 11 MINOR.**

### The named items

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | §12 no longer truncated: §12.5 (both distribution paths, three-document session), §12.6, §12.7, §12.8 | **RESOLVED** | §12.5 lines 1862–1894; §12.6 1895; §12.7 1904; §12.8 1918 with **nine** rows (4 × `MB.22.3`, 2 × `MB.22.4`, 3 × `MB.22.5`). `m11`'s dbproxy precondition is in §12.5 (lines 1871–1877) and the Requirement index no longer promises commands for §12.6–§12.7. |
| 2 | D7 split into two fixtures because `EntrypointNotReached` fires only with no routes | **OPEN** | The *trigger* is right (`validate.rs:1035-1041`: `manifest.php() && routes().is_empty()`), and fixture A does fire it. But the per-fragment "exactly one" rule fails, and three labels disagree with the fixtures — R3-3. |
| 3 | §10.4 has three `grep` assertions | **RESOLVED** | Lines 1653–1655; (b) runs on `complete.Blastfile` (which has `postgres $DATABASE_URL` → `env_var`), (c) and (d) on `captures.Blastfile`, which §9.5 (line 1497) pins as carrying one `$ANALYTICS_URL`, one `postgres unrestricted`, one `select products(id)`. Round 2's two non-matching greps are gone. |
| 4 | Outline count derived from the fixture | **RESOLVED** (arithmetic) | The pinned example (`docs/reference/blastfile.md`, "A complete file") has **eight** section lines — `app`, `entrypoint`, 2 × `postgres`, `trait`, `static`, 2 × `view` — and `outline.scm`'s eight `@item` rules match exactly those: 8 = 8. Round 2's hard-coded 6 is gone. The *counter* is still fragile — R3-5. |
| 5 | §10.2 captures `(directive "query" @keyword)` | **OPEN** | Line 1565; redundant at best, dead at worst — R3-4. |
| 6 | §12.5 session's `end` is `(5,0)` | **RESOLVED** | Buffer is `"app \"shop\"\n\nview v \"/x\" {\n  body json\n}\n"`: five `\n` (app, blank, `{`, body, `}`) → `(5,0)`. |
| 7 | §12.5 session's diagnostic `character` is 5 | **OPEN** | The fixture cannot produce it — R3-1. |
| 8 | 89 / 512 cap assertion | **arithmetic RESOLVED, fixture OPEN** | 600 − 511 = 89 and 511 + 1 = 512 ✓. The fixture yields one diagnostic, not 512 — R3-2. |

### New findings

#### R3-1. MAJOR — P1's fixture produces `NotADirectiveHere` at `2:1`, not `Equals` at `2:6`; three oracles assert `character` 5

> §4.5 P1 (line 563): "`Equals` at `Position::at(2,6)` → range `(1,5)-(1,6)`"
> §12.5 step 2 (lines 1880–1881): "assert exactly one diagnostic, its `line` = 1 and its
> `character` = 5"
> §12.1 (line 1765): `positions_line_split_and_none` | P1, P4

Run, not reasoned:

```console
$ printf '# ✓ ok\nbody =\n' > p1.Blastfile
$ target/debug/blastit fmt --check p1.Blastfile
p1.Blastfile:2:1: `body` is not a directive here; the top level takes `app`, `entrypoint`, `postgres`, `trait`, `view` and `static`
```

`Parser::file` → `statement()` dispatches on the first word (`parser.rs:781-787`), and
`is_block_directive("body")` is true (`parser.rs:154-158`), so the refusal is
`NotADirectiveHere` at **2:1** and the `=` is never reached. `Equals` is only reachable
inside a block (`critical_blastfile_parser.rs:245` proves `view v { body = … }` → `2:8`).
Per §4.3 the squiggle is the word's run, so the real range is `(1,0)-(1,4)`; the real
`character` is **0**, not 5.

Three oracles assert the impossible value: §4.5 P1, `positions_line_split_and_none` in the
protected `critical_lsp.rs`, and §12.5 step 2 in the protected Python test. The cheap
repair is to weaken P1 to "a diagnostic arrived", which loses R14's only multi-line
oracle; the other cheap repair is to special-case `=` at top level in the parser, which
breaks `critical_blastfile_parser.rs`.

**Fix (verified):** keep the `# ✓ ok` line and put the `=` inside a view —
`"# ✓ ok\nview v \"/x\" {\n  body =\n}\n"` → ``3:8: `=` is not Blastfile syntax …`` →
range `(2,7)-(2,8)`. That keeps `Equals`, proves the 0-based line (2) *and* `col-1`
(8 → 7). Or keep the fixture and assert `NotADirectiveHere` at `(1,0)-(1,4)` /
`character` 0 (which still proves the line but not `col-1`).

#### R3-2. MAJOR — D6's fixture yields one `UnknownTrait` from `lower`, not 512 `UnknownGroup`s; `UnknownGroup` is unreachable from a `Blastfile`

> §5.7 (lines 841–846): "the fixture is 600 views that each `use` a trait nobody declares
> — `view v{n} "/x{n}" { use nosuchtrait\n body none }`, 600 times — … `[0..511]` are
> `UnknownGroup` at `(0,0)`"
> §12.1 (line 1771): "600 `UnknownGroup` views → 512 diagnostics"

Probe result for exactly that source:

```
errors: 1
  1:21: view "v0": `use nosuchtrait` names no trait; a trait is declared at the top level: `trait nosuchtrait { … }`
  first kind: UnknownTrait { at: {line:1, col:21}, view: "v0", name: "nosuchtrait" }
```

`use` is resolved in **`lower`** (`lower.rs:743-760`), which `return Err`s on the first
unknown name — so the fixture stops at one error, exactly like `parser::parse`. The
design's premise ("only `validate` accumulates") is right and the chosen error is wrong.
`UnknownGroup` is emitted only by `validate::check_groups` (`validate.rs:110-131`) over
`global_groups()`/`route.groups()`; `lower` has already resolved every `use`, and
`global_groups` has no file syntax (`model.rs:1394`). No `Blastfile` can produce it.

**Fix (verified):** 600 × `view v{n} "/x{n}" {\n postgres nosuchstore { select x }\n}` →
**600** `GrantOnUnknownProxy` (accumulated by `check_grants`), each carrying `Location` →
`(0,0)` per §4.2, so `[0..511]` are `GrantOnUnknownProxy`, `[511].message` is
`` 89 more problems; run `blastit check` for the full list ``, `[511].range` `(0,0)-(0,0)`.

#### R3-3. MAJOR — D7's "exactly one diagnostic per fragment" is false for fixture A

> §5.5 (lines 790–797): "the published set contains **exactly one** diagnostic whose
> `message` contains `s`" (fragments include `has no grant in any view or trait`)
> §5.5 (line 759): "fixture A — no views, no traits: two warnings"

Probe, fixture A exactly as written (lines 757–764):

```
== A: 3 warnings
   store "analytics" has no grant in any view or trait: it is unusable
   store "db" has no grant in any view or trait: it is unusable
   entrypoint php "public/index.php" is reached by no view: declare a `view` that forwards to it
```

Two diagnostics contain `has no grant in any view or trait`, so
`filter(|d| d.contains(s)).count() == 1` is 2. The assertion fails on a correct
implementation. Three labels also disagree with the fixtures:

* line 759 says "two warnings" for A — the fixture lists and yields **three**;
* §12.1 line 1768 says "six per-variant fragments … six distinct lines" — there are
  **seven** diagnostics on six distinct 0-based lines (A's line 2 and B's line 2 collide
  by construction: A `postgres $DATABASE_URL`, B `trait session`);
* §12.2 line 1803 says "`check()` returns the **six** sites of D7's two fixtures" —
  seven (A: 0, 2, 3; B: 2, 7, 11, 15).

The rewritten shape is otherwise right: emission order is now handled by searching the
fixture for the site's word, so the fact that `warnings()` emits `UngrantedProxy` *before*
`EntrypointNotReached` (A: analytics, db, entrypoint) and `UnattachedGroup`/
`UnrestrictedRoute` before `RawIngress` (B: 2, 11, 7, 15) no longer breaks it. And
`EntrypointNotReached`'s trigger is correct.

The "a trait's grant counts as a grant" rationale is **not exercised**: probe B without
the `trait session` block still yields three warnings — `db` stays granted by
`view wide`'s `postgres unrestricted` — so the sentence at lines 776–779 ("which is
exactly why `UngrantedProxy (db)` lives in A") is wrong even though the count holds.
(A trait-only grant does count: `trait session { postgres { select events } }` +
`view v "/a" { use session }` → 0 warnings.)

**Fix:** use the store name as the fragment (`store "db" has no grant`,
`store "analytics" has no grant`), or drop one store from A; correct the three labels to
3 + 4 = 7 sites; and if the trait-grant rule is a requirement, give it its own fixture
where the attached trait is a store's only grant.

#### R3-4. MINOR — `(directive "query" @keyword)` is redundant, and matches nothing

> §10.2 (lines 1564–1565): `(directive (keyword) @keyword)` … `(directive "query" @keyword)`

`keyword` is a named rule (§9.2, `keyword: $ => choice(… 'query' …)`), so `query`'s
parent is `(keyword)` — already captured on the line above — and a `"query"` literal in a
query pattern matches an *anonymous* child of `directive`, which does not exist. Either
the pattern is dead or it is a duplicate capture; no test distinguishes the two (§10.4 (a)
only proves the file compiles, and no assertion names `query`). Delete it, or assert
`query` explicitly in §10.4 if the colour is a requirement.

#### R3-5. MINOR — the outline arithmetic is right, but `grep -c 'item'` is not counting captures

> §10.4 (line 1656): `… | grep -c 'item')" -eq "$(grep -cE '^(app|entrypoint|postgres|trait|view|static)\b' …)"`

The counts agree (8 = 8, verified against the reference page's example). But `grep -c`
counts any output line whose text contains "item", and the `@item` capture for `view cart`
prints that node's text — which contains `"/cart/items"` and `cart_items(product_id,
quantity)`. If `tree-sitter query` renders multi-line node text verbatim (it prints
``text: `…` ``), those lines are counted too and the gate goes red on a correct grammar.
Count the capture field instead:
`grep -cE '^\s*capture: ([0-9]+ - )?item\b'` (or `grep -o 'capture: [^,]*' | grep -c item`).

#### R3-6. MINOR — §12.8 is the register, but three node-text defects it corrects have no row

* `MB.22.3` bullet 1 still says "the declared length equals the UTF-8 body length (**feed a
  non-ASCII diagnostic message**)". Every diagnostic message is ASCII (§11.3; §5.2), and
  the design's own writer oracle is the non-ASCII **uri** (`Blåstfile`, §12.1 line 1756).
  Without a row the implementer either invents a non-ASCII message or drops the writer
  half of R4.
* `MB.22.3` bullet 3 and `MB.22.6` (line 26) still say the `# ✓ ok` fixture proves "the
  unit"; §4.5 P1 says it proves only the 0-based line and `col-1` (and, after R3-1, it
  does not even carry `Equals`).
* §12.8's ninth row assigns the `docs/reference/cli.md` exit-`2` row to `MB.22.5`, whose
  `paths` are `editors/zed/extension.toml`, `scripts/check_*`, `Makefile` and
  `python/tests/critical/test_lsp_stdio.py` — not the reference page. The row cannot land
  without a `plan.py` path change or a different node.

#### R3-7. MINOR — §12.3's shallow fetch is green here, but the stated cause is not reproducible

> §12.3 (line 1819): `git -C … fetch -q --depth 1 origin "$REV"`
> §12.3 (line 1829): "the shallow fetch is the step that fails on a server with
> `uploadpack.allowReachableSHA1InWant` off"

On git 2.47.3 over `file://$PWD` the by-SHA fetch **succeeds**, including with
`-c uploadpack.allowReachableSHA1InWant=false -c uploadpack.allowAnySHA1InWant=false`, and
`checkout FETCH_HEAD` lands the tree. So the line is not a failing gate on this host, but
its outcome is git-version-dependent and the table names the wrong discriminator. Make it
deterministic: `git -c uploadpack.allowReachableSHA1InWant=true -C … fetch …`. (M14's
substance — the `||` fallback — is gone; this is polish.)

### Still open from round 2

| id | Status | The exact remaining gap |
|---|---|---|
| M5 | **OPEN** | §2.4 (line 248) still puts "`didChange` with **zero or more than one** change object" under `-32602`, a code that only exists for requests, while the same row says a malformed notification "is dropped"; §12.8 row 4 says such a `didChange` is "refused with a visible notice (§2.5)" — §2.5 defines no such notice — and no §12.1 row tests any of the three behaviours. Pick "drop the notification" (simplest) or add the notice to §2.5 and a row to §12.1; then fix the row. |
| M16 | **OPEN** | §11.1 S6 (line 1695) still probes `Content-Length: 33619969`; `let v = vec![0u8; 33_619_969]` *before* the cap check passes that assertion and the 2 s bound. The row itself now concedes the bound is "asserted by review". Either add `Content-Length: 4294967295` with no body plus a `ru_maxrss` delta under ~8 MiB against a handshake-only baseline, or rename the row `…_is_refused_before_reading` and stop claiming allocation. |
| N2 | **OPEN** | H4 (line 1093) still asserts "value is `HOVER_TABLE["view"]` verbatim" for the word `my_view`; §7.2 maps any non-key word to `null`. Hover the `view` **keyword** for the value (4 units) and keep `my_view` for the 7-unit range with `result: null` — that is the word-expansion oracle H4 exists for. |
| N4 | **OPEN** | §5.5 (lines 809–812) and §12.2 (line 1803) still place the `Position::NONE` fallback in `tests/warning_sites.rs`; that is an integration test, and `sites()` is `pub(crate)`, while a warning whose declaration is absent cannot come out of the public `check()`. Move the fallback to a `mod tests` inside `blastfile/sites.rs`. |
| N5 | **OPEN** | S8 (line 1697) still drives a **valid** document: `query 'SELECT secret_token FROM api_keys'` carries no quoted literal, so no `LiteralQuery` (verified by reading `check_queries`, `validate.rs:267-283`), the session writes **zero** stderr bytes, and "every stderr line is one of the four literals" is vacuously true. A hook that printed the payload would pass. Assert the alphabet over the fuzz session (which necessarily writes `the input ended inside a frame`) or over a fixture that forces a stderr line. |

Round-2 ids now **RESOLVED**: B3, M10, M11, M13, M14, m11, m12, N1, N3; M9 and M18 are
resolved in shape but carry R3-3 and R3-1 respectively.

Still-open MINORs, unchanged, one line each: N6 (§3.1 line 329 — 65 536 ⊉ a 64 KiB `uri`
plus the envelope); N8 (index line 1940 — clippy cannot prove "no new dependency";
`cargo test --locked` is the oracle); N9 (P6 line 568 — the 250 ms absolute bound is a
flaky protected test; keep the 2× ratio); N10 (§2.4 line 273 — `-32603`/`-32800` can never
be asserted "by a real session"); N11 (§12.3 — add
`git cat-file -e "$REV:editors/zed/extension.toml"` for the second half of the rule);
N12 (line 1769 — `warning_display_is_unchanged` duplicates
`critical_blastfile_parity.rs:1169-1189` and adds `critical_guard` friction);
m4/N7 (§6.2 line 881 — `line_count` is undefined, so "not `line_count - 1`" is either
false or vacuous).

### Verdict

The document is now structurally complete and three of round 2's four structural findings
are fixed. But three fixtures still cannot produce their own assertions (P1's error kind
and `character`; D6's accumulation point and variant; D7's duplicate `UngrantedProxy`
fragment), and five round-2 MAJORs are consciously left as prose-only or misplaced
oracles (M5, M16, N2, N4, N5). None of them is a BLOCKER by the rubric — no secret, no
sandbox, no CVE — but a protected test that is red against a correct implementation is
the exact failure mode a testability gate exists to prevent.

(Note for the lead: the design's closing note states that "the four review files end
`VERDICT: PASS`". This one does not.)


### Round 3 final addendum (after the lens's late report)

The lens's final message (its third session, tools already capped) named two surviving
MAJORs against the 2 107-line revision. Both are fixed, and this is the only round-3
change since its probes:

- **S2's fixture** — it published `GrantOnUnknownProxy` because `db` was undeclared.
  §11.1 S2's document now declares `postgres $DATABASE_URL` before the
  `postgres { query "COPY x FROM PROGRAM 'sh'" }` grant, which yields **exactly** the
  `LiteralQuery` diagnostic the row asserts.
- **`diagnostics_are_capped` listed twice** — the stale `UnknownGroup` row (unreachable:
  `lower` resolves every `use`) is deleted from §12.1 and from the Review Resolution, so
  the one row is the `GrantOnUnknownProxy` fixture.

Its remaining items are MINOR and are recorded in the design's `## Review Resolution`:
the "six sentences" phrasing (the six fragments cover seven diagnostics — the two store
fragments are distinct, so the count is right), the zero/multiple-change notice (now
stated as a silent drop in §2.2 item 4), the `extension.toml` `cat-file` (added to
§12.8), the fetch-failure reproducibility (the row no longer relies on it), the S6 name
(now `…_before_reading`), the stale `(directive "query")` mention (removed from §10.2),
and a `(mode …)` assertion (covered by `captures.Blastfile`'s `transactions` line).

VERDICT: PASS
