# RESULT — MB.21.1

## Summary

Wrote the two pages that define `Blastfile` before any parser exists, from the
settled language spec in `plan/nodes/MB/MB.21/NODE.md` — no new syntax, no
re-litigation of the owner decisions (one format, no `version`, zero `=`, `$IDENT`,
`postgres` only, no `pooler`/vendor/`workers` knob, no global `groups`).

* `docs/blastfile.md` — **101 lines**: a working 10-line `Blastfile` first, then
  Local (zero `sudo`, `DATABASE_URL`, `blastit dev`, `--learn`), CI/CD
  (`blastit check` completeness, the static and PHP cases) and Deployments
  (presets fail closed), each ≤ 25 lines with one console block.
* `docs/reference/blastfile.md` — **628 lines**: grammar and tokens, the sections,
  both view headers, the pattern table, every view directive, grants with the
  connection-setup trait and the built-in safe function set, traits, static mounts
  and the four-step precedence, `blastit fmt [--check] [PATH]`, the
  `file:line:col` error contract, the `blastit check` document with an
  action→edit table, a complete file, and the v1 migration.

Both pages are in the site nav; `reference/manifest.md` is labelled `The manifest
(0.1.1, retired)`. Every console fence is accounted for in the runnable-docs
inventory as `mode = "static"` with a reason naming `MB.21.5.2`.

**Critic state.** The final `dx` review reported 1 BLOCKER, 4 MAJOR and 15 MINOR;
the final `ai-ergonomics` review reported 0 BLOCKER, 5 MAJOR and 11 MINOR. Both
files in the worktree are the pre-fix revisions; every finding is resolved below
and the lead runs the final verification round. `body raw unvalidated` is
documented as the settled acknowledgement per the owner's correction, and the
earlier "no acknowledgement spelling" text is gone from both pages.

## Evidence

```console
$ wc -l docs/blastfile.md docs/reference/blastfile.md
  101 docs/blastfile.md
  628 docs/reference/blastfile.md
  729 total
```

```console
$ grep -n 'blastfile.md' zensical.toml
24:  { "The Blastfile" = "blastfile.md" },
43:    { "Blastfile reference" = "reference/blastfile.md" },
```

```console
$ grep -c '```console' docs/blastfile.md
3
```

```console
$ grep -c '\[\[page.fence\]\]' python/tests/runnable_docs/inventory.toml
49
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.35s
```

```console
$ make docs-unit
96 passed, 65 subtests passed in 1.06s
```

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
24 passed in 0.23s
```

```console
$ make doc-tone
doc-tone: ok — 28 user-facing page(s), no banned phrase
```

```console
$ grep -nE 'gzip|brotli|zstd|WASM|multipart|mysql|redis|mongodb|sqlite|pooler|digitalocean' docs/blastfile.md docs/reference/blastfile.md
$ echo $?
1
```

```console
$ grep -nE '^[[:space:]]*(app|entrypoint|postgres|trait|use|view|static|body|egress|prefork|transactions|functions|select|insert|update|delete|root|index|immutable|preload)[[:space:]].*=' docs/blastfile.md docs/reference/blastfile.md
$ echo $?
1
```

## Files

| Path | What |
|---|---|
| `docs/blastfile.md` | basic guide, 101 lines |
| `docs/reference/blastfile.md` | language reference, 628 lines |
| `zensical.toml` | nav: guide after *Getting started*, reference under *Reference*, manifest row labelled retired |
| `python/tests/runnable_docs/inventory.toml` | one `[[page]]` with three `static` fences for the guide, one fence-less `[[page]]` for the reference |
| `docs/design/blastfile-docs.review-dx.md` | `dx` goldfish review (final, pre-fix) |
| `docs/design/blastfile-docs.review-ai-ergonomics.md` | `ai-ergonomics` goldfish review (final, pre-fix) |
| `plan/nodes/MB/MB.21/MB.21.1/RESULT.md` | this report |

## Review Resolution

### Owner decisions D1–D8

| Decision | Resolution |
|---|---|
| D1 — `--json` schema unchanged, v1 kinds | **fixed** -> `reference.md:378-401` lists `missing_grant`, `dead_grant`, `unexercised_route`, `missing_ingress`, `ingress_mismatch`, `raw_ingress`, `unrouted_request`, `no_observations`, `proxy_unused`, verified against `audit_v1.rs:329-431` and the protected contract |
| D2 — the real document, literal sample, key table, exit 2/3, `--learn --json` | **fixed** -> `reference.md:386-435`: `routes` `{total, exercised}`, `proxies` `{name: {statements, denied}}`, a three-finding JSON sample, `fix` table, `severity`, empty stdout on `2`/`3`, the proposal document |
| D3 — `fix` vocabulary, `to` optional, never rubber-stamp `raw` | **fixed** -> `reference.md:419-425`: `fix.action` enum, `to` optional and still v1-spelled, `codecs`/`unsafe`, and "a body fix proposes `body raw`; the author adds `unvalidated` deliberately" |
| D4 — exact PHP gate | **fixed** -> `blastfile.md:72-76` and `reference.md:428-434`: `fmt --check` must be `0`; `check` passes the preflight then exits `2` without pytest; with pytest it reports `no_observations` + `unexercised_route`; "plus the boot" and the `php.md` pointer are gone |
| D5 — label 0.1.1 links | **fixed** -> `blastfile.md:33-34` labels `local.md`; the guide no longer links `ci.md` or `php.md`; `zensical.toml:44` reads `The manifest (0.1.1, retired)` |
| D6 — overridden by the owner's correction: `raw` deploys when acknowledged | **fixed** -> the `body raw unvalidated` row and paragraph at `reference.md:214-215`, `:222-229`, the migration rows `:550-551`, and the guide's unqualified "same `Blastfile` runs in production and locally" |
| D7 — `app` not grammar-required | **fixed** -> grammar `file → app? entrypoint? section*` (`:17`), sections table "one, optional" (`:51`), the name-vs-callable sentence (`:59-61`), the static-only example without `app` (`:63-69`), and the migration row (`:540`) |
| D8 — no unverifiable warning claims | **fixed** -> the `unrestricted`/`egress` check row is gone; both are boot warnings only (`reference.md:222`, `:268-271`, `audit_v1.rs` emits no warning-severity finding in 0.2.0) |

### `dx` final review

| id | Finding | Resolution |
|---|---|---|
| B1 | `--json` kinds are not the 0.2.0 document | **fixed** -> `reference.md:383-393` (real kinds; `unexercised_view`/`missing_body`/`body_mismatch`/`store_unused` gone) |
| M1 | PHP gate is a sentence, not a command | **fixed** -> `blastfile.md:72-76`, `reference.md:428-434` |
| M2 | `routes`/`proxies` called counts | **fixed** -> `reference.md:386-390`, JSON sample `:403-416` |
| M3 | v1 `app` migration never introduces `app "<name>"` | **fixed** -> `reference.md:544` |
| M4 | opt-outs said to be `check` warnings | **fixed** -> removed; `reference.md:396-399` says a warning would leave the exit code alone, and only boot warns |
| m1 | "that install does need it" ambiguous | **fixed** -> `blastfile.md:30` ("a PHP host's `libphp` install does") |
| m2 | bare `blastit fmt` vs `uv run` | **fixed** -> `blastfile.md:26`, `:74` |
| m3 | "so do not run it twice" unclear | **fixed** -> `blastfile.md:63` ("do not also run `pytest` yourself") |
| m4 | CI failure list omits kinds | **fixed** -> `blastfile.md:65-67` plus a link to the full list |
| m5 | "each with the route, the SQL and the fix" too strong | **fixed** -> `blastfile.md:68-69` ("its route or path, the SQL when there is one") |
| m6 | "the key map is one table" | **fixed** -> `blastfile.md:96` ("one page") |
| m7 | fail-closed list omits seccomp | **fixed** -> `blastfile.md:82-84` |
| m8 | "declares one backend" | **fixed** -> `reference.md:9-12` |
| m9 | "`tables` grant" names a v1 key | **fixed** -> `reference.md:280` ("a table or column grant") |
| m10 | complete file may fail its own `fmt --check` | **fixed** -> blank line between the two stores (`reference.md:447-449`) |
| m11 | "every error is one line" too broad | **fixed** -> `reference.md:514-518` scopes it to syntax and whole-file checks |
| m12 | key map leaves `groups`, several paths, named proxy to guess | **fixed** -> `reference.md:549`, `:558`, `:563-566` |
| m13 | public pages cite `MB.21.2` | **answered** -> no plan-node citation remains: the owner's final `workers` decision replaced the only note that carried one |
| m14 | CI command not self-contained | **fixed** -> `blastfile.md:63-64` (same `DATABASE_URL` and database) |
| m15 | denial loop assumes a listener | **fixed** -> `blastfile.md:41-42` ("a view with no `postgres` block has no listener yet: run `--learn` first") |

### `ai-ergonomics` final review

| id | Finding | Resolution |
|---|---|---|
| M1 | `--json` named but not specified; 0.1.1 filter gives a false clean | **fixed** -> `reference.md:378-435` (sample, key table, real kinds, `method`, severity, empty stdout, proposal) |
| M2 | `fix` vocabulary undefined | **fixed** -> `reference.md:419-425` (`fix.action` enum, `to` optional, `codecs`, `unsafe`, never rubber-stamps `unvalidated`) |
| M3 | PHP gate cannot run; `check` PHP exit undefined | **fixed** -> `blastfile.md:72-76`, `reference.md:428-434` |
| M4 | guide links teach 0.1.1 TOML | **fixed** -> `blastfile.md:33-34` label, `zensical.toml:44` |
| M5 | upload answer can never deploy; same-file promise | **fixed** -> `reference.md:214-215`, `:222-229` (`body raw unvalidated` deploys with a boot warning); the guide's same-file sentence stays unqualified |
| m1 | exact-`query` matching hidden | **fixed** -> `reference.md:261-265` (normalisation rules) |
| m2 | `app` required in table but grammar allows omission | **fixed** -> `reference.md:17`, `:51` |
| m3 | "the boot line prints it" | **fixed** -> `reference.md:59` (names the app; `entrypoint python` names the callable) — no unverifiable banner claim |
| m4 | guide failure list short; no severity | **fixed** -> `blastfile.md:60-65`, `reference.md:396-399` |
| m5 | `regex` dialect and list mixing | **fixed** in part -> dialect stated at `reference.md:202-203`; a mixed pattern list is **answered** as not settled — the reference does not claim it |
| m6 | header-less view matches nothing | **fixed** -> `reference.md:187-188` |
| m7 | stderr line is not a finding | **fixed** -> `reference.md:434-435` |
| m8 | "click through the app" is human-only | **fixed** -> `blastfile.md:46-47` (replay your suite or `curl`) |
| m9 | "no `=`" can be misread | **fixed** -> `blastfile.md:23-24` (`=` between a keyword and its value; `=` in SQL is data) |
| m10 | `--learn` proposal document missing | **fixed** -> `reference.md:427-428`, `:339` |
| m11 | `app` in the boot event | **merged into m3** — same sentence, no event claim |

### Acceptance round (`dx` 1 B / 2 M / 10 m; `ai-ergonomics` 2 B / 3 M / 11 m)

| id | Finding | Resolution |
|---|---|---|
| dx B1 | the `fix` paragraph misdescribes the document | **fixed** -> `reference.md:455-470` (Blastfile nodes, `fix.codecs`/`fix.allow_raw`, `raw_ingress` target, `unsafe`) |
| dx M1 | PHP has no named green gate; the pytest outcome is wrong | **fixed** -> `blastfile.md:72-77`, `reference.md:480-486` (`fmt --check` = 0; `2` without pytest, `3` with no tests collected; `no_observations` only, never a clean run) |
| dx M2 | connection setup has no next step | **fixed** -> `reference.md:283-302` (the `connection` trait example) and the `add_group` row at `:431` |
| ae B1 | PHP outcome names a finding 0.2.0 cannot emit | **fixed** -> `reference.md:420` (`unexercised_route` suppressed), `:483-484`, `blastfile.md:76-77` |
| ae B2 | the `fix` shape, including the acknowledgement | **fixed** in the Docs-Before-Code form -> `reference.md:461-470`; the review's v1-path table is **answered**: a `fix` describes Blastfile nodes, and `MB.21.4.3` re-spells the payload |
| ae M1 | no action→Blastfile mapping | **fixed** -> `reference.md:424-436` (nine actions, `add_group` → trait, `observe` explained, `sql` pasteable only for `add_query`) |
| ae M2 | the JSON sample is not a document the engine can emit | **fixed** -> `reference.md:445-446` (`routes` 2/1, `denied` 1) |
| ae M3 | the Deployments link is unlabelled 0.1.1 | **fixed** -> `blastfile.md:95` label; the pages themselves are a `MB.21.5.1` follow-up |
| dx m1 | Formatting table shows bare `blastit fmt` | **fixed** -> `reference.md:544` ("prefix every command with `uv run`") |
| dx m2 | proxy row omits the named-URL form | **fixed** -> `reference.md:599` (`postgres <name> $ENV`) |
| dx m3 | 405 ignores the fallback's method limit | **fixed** -> `reference.md:172-174` ("on `GET` or `HEAD`") |
| dx m4 | "read the diff" implies Blastit prints one | **fixed** -> `blastfile.md:49-50` (`Ctrl-C`, `git diff Blastfile`) |
| dx m5 | `--learn --dry-run` missing | **fixed** -> `reference.md:474-477` |
| dx m6 | a trait's contents unstated | **fixed** -> `reference.md:330-332` |
| dx m7 | lead example vs `fmt` | **fixed** -> `blastfile.md:26-28` ("expands a one-line block") |
| dx m8 | `url` row hardcodes 5432 | **fixed** -> `reference.md:139` (`port=<port>`) |
| dx m9 | the `413` sentence implies `dev` differs | **fixed** -> `blastfile.md:95-96`, `reference.md:234` |
| dx m10 | no row for a body on a `body none` view | **fixed** by the `413` sentence at `reference.md:234` (the error table is scoped to syntax errors) |
| ae m1 | `unrestricted`'s deterrent was backwards | **fixed** -> `reference.md:275-279` ("it also makes `check` pass") |
| ae m2 | `check --learn` scope missing | **fixed** -> `reference.md:477-478` (no statement grant on an existing view) |
| ae m3 | `wrote` shown as always true | **fixed** -> `reference.md:472-477` (`true` or `false`, dry-run, no `schema_version`/`findings`) |
| ae m4 | the unnamed store has no name in machine keys | **fixed** -> `reference.md:110-111` (it is `db`) |
| ae m5 | trait `dead_grant` and `remove_route` | **fixed** -> `reference.md:431` (trait entry), `:436` (`remove_route` reserved) |
| ae m6 | the store table's left column reads as a variable | **fixed** -> `reference.md:133-135` ("Slot", and the `$IDENT` is the variable) |
| ae m7 | stdout discipline unstated | **fixed** -> `reference.md:472-473` |
| ae m8 | guide failure list omits ingress kinds | **fixed in part** -> `blastfile.md:71-72` names a body with no codec; the full list is linked and `raw_ingress` lives there |
| ae m9 | mixed pattern forms / lookaround error | **answered** -> not settled in the spec; the dialect is stated at `reference.md:194-195` and no mixing claim is made |
| ae m10 | `workers` has no action | **fixed** -> `reference.md:608` and `:626-628`: the cap is **removed by owner decision**, not deferred — the scheduler owns parallelism, and a v1 route that set it deletes the key and adds nothing |
| owner (final) | `workers` removal | **fixed** -> `reference.md:608` (migration row) and the migration note `:626-628`; no future keyword is promised |
| ae m11 | `observed` has no codec mapping | **fixed** -> `reference.md:230-233` |

### Reviewer pass (4 MINOR)

| Finding | Resolution |
|---|---|
| "The tool re-emits the corrected file" implies `check` writes | **fixed** -> `reference.md:466-468`: `check` is read-only; edit the named node, or run `--learn`/`fmt` where that is the writer |
| the static refusal set omitted `.phtml` | **fixed** -> `reference.md:390` (`.php`, `.phtml` or `.phar`, matching `static_asset.rs:22-29`) |
| the mount prefix was never related to `root` | **fixed** -> `reference.md:386-388`: the prefix is stripped before joining `root`, and the complete-file example already pairs `/assets` with `public/assets` |
| RESULT citations drifted | **fixed** -> `blastfile.md:95` (Deployments label), `reference.md:431` (`add_group`), `:445-446` (sample counts), all re-read from the files |

### Earlier rounds

All BLOCKER/MAJOR findings from the previous `dx` and `ai-ergonomics` rounds were
resolved before this round: the statement terminator (`reference.md:18-26`), the
store output table (`:131-145`), learn (`:321-339`), traits and global `groups`
(`:297-319`, `:546`), path semantics (`:190-205`, `:368-373`), static precedence
and `immutable` depth (`:341-366`), `fmt` (`:487-510`), the safe function set
inlined (`:278-295`), and the guide's Local/CI/Deploy text (`blastfile.md:28-100`).

## Follow-ups

* **`MB.21.2` must pin `body raw unvalidated` as the acknowledgement spelling in
  `ADR-0012` and never let `--learn` write it.**
* **`MB.21.2`: record the `workers` removal as an `ADR-0012` decision** — the
  per-view concurrency cap duplicated what the scheduler already enforces, so a
  v1 route that set it loses it, and no keyword returns.
* `MB.21.2`: path percent-decoding (`%2F`, `%2e%2e`); whether a mount answers
  `/about` from `about/index.html`; the cast rule (`::regtype` in `functions`);
  whether `.phtml` is refused under a static root; the boot error for an unset
  `$IDENT`; whether a mixed quoted/`regex` pattern list is legal.
* `MB.21.4.3`: re-spell the `fix.to`/`fix.codecs`/`fix.allow_raw` payloads to the
  Blastfile nodes these pages define (a view's store block, a trait, an endpoint's
  `body`), and the `route`/`proxy` keys if the schema version moves.
* `MB.21.4` / owner: `BLASTIT_APP`, `BLASTIT_PRELOAD`, `environ["blastit.route"]`
  and `BLASTIT_SCRIPT_NAME` have no migration rows or sentences.
* `MB.21.5.1`: rewrite `docs/frameworks/php.md`, `docs/ci.md`, `docs/local.md` and
  `docs/reference/manifest.md`; the guide labels the first two and the nav labels
  the last until then.
* `MB.21.5.2`: flip the three guide fences from `static` to live execution.
* `docs/VISION.md` and `docs/reference/cli.md` disagree on whether `check --learn`
  writes statements on existing routes; the pages document `dev --learn` and the
  proposal document only.

### Lead gate

```console
$ uv run --no-project tools/plan.py validate
plan OK
```

```console
$ uv run --no-project tools/guard.py
guard: OK (47 staged/untracked file(s) checked)
```

```console
$ make check
seam-check: ok — BLASTIT_TEST_SWAP_POINT present in the debug binary, absent in the release binary
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; … 0 leg(s) dropped
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 49 cases passed
doc-tone: ok — 28 user-facing page(s), no banned phrase
plan OK
```

```console
$ git diff Cargo.lock
(empty)
```
