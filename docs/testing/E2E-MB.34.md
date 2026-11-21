# E2E — MB.34 (SQL template validation, Quick Fix, SQL highlighting)

Outside-in proof: hand-written fixtures, every tail a command this host ran. Branch
`release/0.2.0`, base `d5e43e9`, HEAD `1117a10` (the MB.32 gate commit on top), binary
`target/debug/blastit` from that tree, Docker 29.10.0. **VERDICT: PASS** — one item is
`NOT RUN` (§5, no Zed here), and one behaviour differs from this leaf's brief: `--json` has no
document for these four defects, by design (§2).

## 1. The fixture: four defects in one `Blastfile`

`/tmp/opencode/mb347/four/Blastfile` (numbers for reading, not in the file):

```text
 1  postgres $DATABASE_URL
 2
 3  view products GET "/products/" {
 4      postgres {
 5          query 'SELCT id FROM products WHERE price > $?'
 6          query 'SELECT products.id, orders.total
 7                 JOIN orders ON orders.product_id = products.id'
 8          query "SELECT id FROM products WHERE email = 'a@b.com'"
 9          query 'SELECT id FROM products WHERE price > …'
10      }
11  }
```

`SELCT` typo; a **multi-line** statement with no `FROM` (a `JOIN` cannot start a select list);
an inline literal in the real `"…"` spelling; a redacted `…`. `DATABASE_URL` was unset so the
validation refusals are what prints. `SELECT id\nWHERE price > $?` — the obvious "missing
`FROM`" — is valid PostgreSQL (`SELECT 1 WHERE true`) and proves nothing, so it is not used.

## 2. CLI, `--json` and the LSP publish, on the same bytes

```console
$ blastit check
blastit check: ./Blastfile: 5:16: the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`
6:16: the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`
8:54: route "products" (index 0): queries[2] inlines a quoted literal (grant on proxy "db"); bind the value with `$?` instead of writing it in the statement
9:54: route "products" (index 0): queries[3] holds a redacted `…` placeholder (grant on proxy "db"); grant the exact statement, with `$?` where the values bind
exit=2

$ blastit check --json > out.json 2> err.txt; echo "exit=$?"; wc -c < out.json
exit=2
0
```

A `query` that does not load is a configuration refusal (exit `2`), never a finding: `--json`
prints **nothing** on stdout and the same four lines on stderr — `docs/ci.md`'s new paragraph
("an empty stdout is a refusal, never a clean audit"; "the refusal adds no `kind` and no
`fix.action`"). The brief expected a JSON `line:col` per defect; adding one would have been a
new finding kind, so there is none.

`uv run --no-project /tmp/opencode/mb347/compact.py <binary> four/Blastfile` (one `initialize`,
`initialized`, `didOpen`, `shutdown`, `exit`): `publishDiagnostics.params` keys
`['diagnostics', 'uri', 'version']`, `uri: file:///work/Blastfile`, `version: 1`; the objects,
one per line (JSON verbatim, indentation folded):

```json
{"message": "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`", "range": {"end": {"character": 20, "line": 4}, "start": {"character": 15, "line": 4}}, "severity": 1, "source": "blastit"}
{"message": "the statement does not parse; check it for a typo, a missing `FROM` or a stray `)`", "range": {"end": {"character": 21, "line": 5}, "start": {"character": 15, "line": 5}}, "severity": 1, "source": "blastit"}
{"message": "route \"products\" (index 0): queries[2] inlines a quoted literal (grant on proxy \"db\"); bind the value with `$?` instead of writing it in the statement", "range": {"end": {"character": 54, "line": 7}, "start": {"character": 53, "line": 7}}, "severity": 1, "source": "blastit"}
{"message": "route \"products\" (index 0): queries[3] holds a redacted `…` placeholder (grant on proxy \"db\"); grant the exact statement, with `$?` where the values bind", "range": {"end": {"character": 54, "line": 8}, "start": {"character": 53, "line": 8}}, "severity": 1, "source": "blastit"}
```

| # | Defect | CLI | JSON | LSP start (+1) | LSP end | Message identical? |
|---|---|---|---|---|---|---|
| 1 | `SELCT` typo | `5:16` | — (no document) | `(4,15)` → `5:16` | `5:21` | yes, byte for byte |
| 2 | multi-line, missing `FROM` | `6:16` | — (no document) | `(5,15)` → `6:16` | `6:22` | yes |
| 3 | inline `'a@b.com'` | `8:54` | — (no document) | `(7,53)` → `8:54` | `8:55` | yes |
| 4 | redacted `…` | `9:54` | — (no document) | `(8,53)` → `9:54` | `9:55` | yes |

Both surfaces name the same character and sentence: `Syntax` sits on the statement's first token
(hence `:16` for the multi-line case, not the `JOIN` the parser stopped on — the parse message is
not printed because it can quote a value); the literal and the `…` sit on their own character
inside the string. CLI is 1-based, LSP `character` 0-based, same UTF-16 axis (ASCII here;
`critical_lsp.rs` C7 pins the non-BMP/tab cases where that axis is not the byte offset).

**The `--json` finding shape is unchanged.** `--json` findings are observation findings, so the
comparison uses an old-style-defect fixture (a `SELECT 2` the grant lacks, live `postgres:18`):

```json
"findings": [
  {"kind": "missing_grant", "severity": "error", "route": "home", "proxy": "db",
   "sql": "SELECT 2", "fix": {"action": "add_query", "to": "view home postgres"}},
  {"kind": "dead_grant", "severity": "error", "route": "home", "proxy": "db",
   "sql": "SELECT 1", "fix": {"action": "remove_query", "to": "view home postgres", "unsafe": true}}
]
```

Same `schema_version: 1`, same members, same closed `fix.action` set (`add_query`,
`remove_query`, `add_test`, `declare_ingress`, `add_route`, `observe`) — structural, not luck:
`git diff --stat 5f293985..HEAD -- crates/blastit/src/{audit_v1,check,cli}.rs` and the matching
`git log` are both **empty**, so the report emitter and the JSON document are byte-identical to
the epic's base and no new `kind` or `action` can exist.

## 3. Quick Fix round trip — `NOT RUN` in Zed, wire transcript instead

**`NOT RUN (no Zed on this host)`**: `which zed` is empty (no binary in `~/.local`, `/usr/bin`,
`/opt`, no flatpak), so there is no lightbulb click and no screenshot. The frames the extension
sends were recorded against `target/debug/blastit lsp` (`/tmp/opencode/mb347/quickfix.py`) on a
real file (`/tmp/opencode/mb347/fixproj`, `postgres:18` on `127.0.0.1:55432`,
`BLASTIT_PYTHON`/`PYTHONPATH` → the repo `.venv` and `python/`):

```console
$ blastit check                     # before
blastit check: ./Blastfile: 5:23: route "home" (index 0): queries[0] inlines a quoted literal (grant on proxy "db"); bind the value with `$?` instead of writing it in the statement
exit=2
<- codeActionProvider = true
<- publishDiagnostics (before) [1]: start 4:22 -> 5:23, end 5:24
<- codeAction.result[0]: {"title": "Parameterize SQL for Blastfile ($?)", "kind": "quickfix",
     "edit": {"newText": "SELECT $? AS email", "range": {"start": {"line": 4, "character": 15},
     "end": {"line": 4, "character": 40}}}}
<- publishDiagnostics (after the client's didChange) [0]: []
-> shutdown {"id": 3, "result": null} | exit: 0 | stderr: ""
$ blastit check                     # after, the edited buffer written back
1 passed in 0.12s
blastit check: 1 requests, 1 statements, 1/1 routes, clean
exit=0
```

The app binds the value on the wire (`execute("SELECT %s AS email", ("a@b.com",))`), so the
grant `SELECT $? AS email` matches the `$1` the proxy saw — the fix is half the job, as
`docs/local.md` says. Hover, same session (`$?` at `4:53`, `...` at `4:71`):

```text
$?: `$?` is one bind placeholder: the app sends one value for it, and the wire sees `$1, $2, …` in
    the order the placeholders appear. `IN ($?, ...)` binds any number of values with one grant —
    the list length is not part of the grant.
...: `...` is Blastit's variadic marker, not SQL: it is legal only as `IN ($?, ...)` (any number of
    values) or after a `VALUES` row, `($?, $?), ...` (any number of rows). `blastit check` refuses
    it anywhere else.
```

## 4. Highlighting: the tree, and the theme capture list

`tree-sitter parse` (CLI 0.25.10, the `package.json` pin) on
`query 'SELECT id FROM products WHERE price > $? AND id IN ($?, ...)'`:

```text
      (query_decl [4, 8] - [4, 76]
        (sql_string [4, 14] - [4, 76]
          (sql_keyword [4, 15] - [4, 21])   SELECT
          (sql_other [4, 21] - [4, 22])
          (sql_word [4, 22] - [4, 24])      id
          (sql_other [4, 24] - [4, 25])
          (sql_keyword [4, 25] - [4, 29])   FROM
          (sql_other [4, 29] - [4, 30])
          (sql_word [4, 30] - [4, 38])      products
          (sql_other [4, 38] - [4, 39])
          (sql_keyword [4, 39] - [4, 44])   WHERE
          (sql_other [4, 44] - [4, 45])
          (sql_word [4, 45] - [4, 50])      price
          (sql_other [4, 50] - [4, 51])
          (sql_operator [4, 51] - [4, 52])  >
          (sql_other [4, 52] - [4, 53])
          (sql_parameter [4, 53] - [4, 55]) $?
          (sql_other [4, 55] - [4, 56])
          (sql_keyword [4, 56] - [4, 59])   AND
          (sql_other [4, 59] - [4, 60])
          (sql_word [4, 60] - [4, 62])      id
          (sql_other [4, 62] - [4, 63])
          (sql_keyword [4, 63] - [4, 65])   IN
          (sql_other [4, 65] - [4, 66])
          (sql_other [4, 66] - [4, 67])     (
          (sql_parameter [4, 67] - [4, 69]) $?
          (sql_other [4, 69] - [4, 70])
          (sql_other [4, 70] - [4, 71])
          (sql_ellipsis [4, 71] - [4, 74])  ...
          (sql_other [4, 74] - [4, 75])))   )
```

A lowercase statement tokenizes identically — `tree-sitter query highlights.scm` prints `keyword`
for `select` `[5,15-21]`, `count` `[5,22-27]`, `from` `[5,31-35]`, `where` `[5,43-48]`, `and`
`[5,60-63]`, and `variable.special`/`variable` for `$1` `[5,57-59]` and `$2` `[5,71-73]`. The
five captures that paint it (`editors/zed/languages/blastfile/highlights.scm`, same copy in the
grammar package):

```scheme
(query_decl "query" @keyword)
(sql_string) @string.special
(sql_keyword) @keyword
(sql_parameter) @variable.special @variable
(sql_ellipsis) @variable.special @variable
```

All five are in the fifteen-name theme allowlist of `docs/design/blastfile-lsp-zed.md` §10.1
(`@keyword`, `@string.special`, `@variable`, and the `@variable.special` first half of Ayu's
right-to-left fallback pair); **no new capture name**. `sql_word`/`sql_number`/`sql_operator`/
`sql_other` are deliberately uncaptured and inherit `@string.special`. `make zed-check` holds
both copies and the node types to it (47 properties).

## 5. What was NOT run

| Item | Why |
|---|---|
| Quick Fix in Zed (lightbulb → click → clean file) | **`NOT RUN (no Zed on this host)`**; §3 is the same frames on the wire and its `blastit check` exit `0` is the real file |
| Hover rendered in Zed, highlighting screenshot | same; §3 has the hover payload, §4 the tree and captures |
| `make docs-serve-check`, `make php-sapi-e2e`, `make mutants-rust` | outside this node's acceptance |

## 6. Documented commands and the fast gates

```console
$ make doc-tone          # doc-tone: ok — 30 user-facing page(s), no banned phrase          exit=0
$ make docs-check        # zensical --strict: No issues found, 0.19s                       exit=0
$ uv cache clean blastit --force && make docs-test
                         48 fences: 30 executed, 18 accounted, 0 failed · 21.4s            exit=0
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
                         52 passed in 1.88s                                                exit=0
$ uv run --no-project tools/plan.py validate          plan OK                              exit=0
$ cargo test -p blastit-sqlpolicy -p blastit-manifest 44 suites, 920 passed, 0 failed       exit=0
$ cargo test -p blastit --test critical_lsp           52 passed, 0 failed                   exit=0
$ make zed-check         zed-check: OK (47 properties checked); 26 tests OK                 exit=0
```

`make docs-test` executed the console fences of `README.md` and every public page: 48 fences,
30 executed (all six of `docs/local.md`, all five of `docs/ci.md`), 18 statically accounted,
0 failed. The group was run twice — 05:09 on a tree with 2 changed files, and 05:18 on a tree
with 18, after a sibling node's in-flight page edits landed (§8.4) — green both times with the
same 48/30/18/0 counts.

## 7. `make check` and `make e2e`

```console
$ timeout 3000 make check                                                                   # wall 218 s
guard: OK (0 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 2 changed file(s)
  no change to a protected critical test
…
PASS  docs/ci.md  fence 3  json      0.8s  one JSON document with 6 findings
…
ok   highlights: sql captures
zed-check: OK (47 properties checked)
UV_OFFLINE=1 uv run --no-project scripts/test_check_zed_extension.py
Ran 26 tests in 5.000s / OK
uv run --no-project tools/plan.py validate
plan OK
check exit=0
```

184 `test result: ok` lines, zero failures; 48/48 fences; `critical-guard` reports 2 changed
files and `no change to a protected critical test`. No other cargo build was running against
this workspace (the only waits were uv's package-cache lock); see §8.4 for the doc files a
sibling node edited while this session ran.

```console
$ timeout 2400 make e2e                                                                     # wall 181 s
scripts/build-wheel.sh --manylinux 2_28
README.md: already absolute
📦 Built wheel for CPython 3.14 to /wheel-out/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
wheel-tag: blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl: Tag: cp314-cp314-manylinux_2_28_x86_64
…                                  (Docker + rootless scenarios: 8 groups, 411 `ok` checks)
PASS: 0 failure(s)
…
  ok   an entry rewritten in place is refused (500)
  ok   and the in-place decoy never reaches the client
  ok   a restart serves the page as it is now
  ok   the restarted boot serves it (200)
e2e exit=0
```

Both gates were bounded and neither hit its bound (218 s, 181 s). The Docker
(`E2E_DOCKERFILE=Dockerfile.prebuilt`) and rootless (`e2e/rootless/run.py --local`) halves each
ran their real multi-UID scenarios against the `manylinux_2_28` wheel; 8 groups report
`PASS: 0 failure(s)` and the only `warning:` in the log is `blastit-php-sapi`'s documented
no-toolchain degrade inside the wheel container (`make php-sapi-e2e` owns that shim).

## 8. Hazards for the next agent

1. **`README.md` is dirty on purpose — never `git add` it.** `scripts/readme-absolute-links.py`
   — the release-time transformation (`ref` `develop`) that `scripts/build-wheel.sh` invokes and
   then *restores from a copy on EXIT* (`build-wheel.sh:119-127`, `trap restore_readme EXIT`) —
   left this copy dirty from a direct or interrupted run: relative brand, `ARCHITECTURE.md` and
   `LICENSE` links became `raw.githubusercontent.com` / `github.com/…/blob/develop` URLs. Gates
   keep it as-is: a run prints "README.md: already absolute" and restores the bytes it found, so
   the file's sha1 is identical before and after `make check`/`make e2e`. No plan node owns the
   rewrite, so expect the diff to survive every gate — and never `git add` it.
2. **An exit-`0` `blastit check` needs `pytest` and a resolvable `DATABASE_URL`.** The standalone
   binary walks `PATH` for `python3` (3.10 here); export `BLASTIT_PYTHON` (and
   `PYTHONPATH=<repo>/python`) to the repo `.venv`, or `check` stops at "pytest is not
   installed". A grant is only proved exercised against a real `postgres:18`, and a `SELECT 'x'`
   sent as a literal does not match a `$?` grant — bind it in the code too.
3. **A parse failure's site is the statement's first token** (`:16` for both `SELCT` and the
   multi-line case) — the printed sentence's contract; do not move it onto the token the parser
   stopped on, which can be a value.
4. **This worktree is shared, and a sibling node was editing it during this session.** `git
   status` held 2 changed files when `make check` started (05:05) and 18 at 05:18: `docs/
   getting-started.md`, `docs/local.md`, `docs/reference/cli.md`, `docs/troubleshooting.md`,
   `editors/zed/README.md`, `python/tests/test_docs_assets.py` and `plan/nodes/MB/MB.35/*`
   appeared at 05:14–05:16 (a cross-platform-LSP node). No `crates/**` file and no grammar file
   changed, so the binary, the LSP and the tree-sitter evidence above still describe the
   committed sources; the file-lock waits in `make check` were uv's cache, not a cargo build.
   Re-read `git status` before running a gate, and do not treat a newer diff in those pages as
   this epic's.

## 9. The two facts this result must state

1. **No squiggle is at `0:0`.** All four are inside the string: `5:16`, `6:16`, `8:54`, `9:54`
   from `blastit check`; `(4,15)`, `(5,15)`, `(7,53)`, `(8,53)` from the LSP.
2. **No `query` sentence gained a `pg_query` internals lecture.** The epic's diff to the five
   user pages contains no `pg_query`, `libpg`, `lexer`, `CST`, `grammar` or `parser.c`: tables
   and bullets only, zero new fences.

VERDICT: PASS
