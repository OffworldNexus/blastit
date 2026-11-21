# `dx` review — Blastfile docs (acceptance round)

**Lens:** human developer experience — steps on (a) local laptop, (b) CI/CD, (c) deployment; zero
`sudo` for the Python/static path; leanness; one canonical way per task; a documented next step for
every failure a normal reader hits; a migration table a v1 user can follow without guessing.
**Documents:** `docs/blastfile.md` (**100 lines**), `docs/reference/blastfile.md` (**574 lines**).
The guide is under the 150-line cap; Local/CI/Deploy are 25/25/15 lines, each with one console fence.
**Language is settled** (`plan/nodes/MB/MB.21/NODE.md:33-103`); nothing below asks for new syntax.
**Citation:** `guide:` is `docs/blastfile.md`; `reference:` is `docs/reference/blastfile.md`; a bare
`:NNN` continues the file last named.

## This revision

Read both pages in full (`guide:1-100`, `reference:1-574`) and re-checked every claim against the
settled spec, the protected contract and the code it names:

* `plan/nodes/MB/MB.21/NODE.md:33-103`, `MB.21.1/NODE.md:23-71`;
* `python/tests/critical/test_check_contract.py`, `crates/blastit/src/audit_v1.rs`,
  `crates/blastit-ingress/src/lib.rs`, `crates/blastit/tests/critical_check_engine.rs:822-827`;
* `docs/ci.md:20-136`, `docs/reference/cli.md:14-88`, `docs/local.md:27-63`,
  `docs/deploy/index.md:1-91`, `docs/reference/manifest.md:96-247`, `docs/VISION.md:15-26`;
* site wiring `zensical.toml:20-46`.

The last round's BLOCKER and MAJORs are mostly fixed: the nine finding kinds are the real ones
(`reference:384-394`), the PHP commands exist (`guide:73-76`), both opt-outs are boot warnings only
(`reference:214-217`, `:274-276`), seccomp is in the fail-closed sentence (`guide:82-84`), the
migration gained the `app` row, route-level `groups`, multi-path and named-grant rows
(`reference:543-558`, `:563-567`). Steps are now three on a laptop, two in CI, one command plus a
recipe at deploy; the Python/static path needs no `sudo`.

**Headline.** The paragraph that defines the `--json` `fix` object is still false about the 0.2.0
document (`reference:419-425`), and two normal reader paths still have no working next step: the PHP
CI gate and connection-setup grants.

## Findings

### BLOCKER — the `fix` paragraph misdescribes the 0.2.0 `--json` document in three ways

`reference:419-425`:

> `fix.to` is present only on the actions that edit a grant, and still names the v1 node
> (`[route.db].queries`) until the cutover re-spells it. A body fix carries `fix.codecs`, and
> `remove_query` and `remove_route` carry `"unsafe": true`. A body fix proposes `body raw`; the author
> adds `unvalidated` deliberately, because the acknowledgement is never granted by a tool.

Each clause is contradicted by the code this page is supposed to document:

1. **A body fix does not propose `raw`.** `suggest_fix` proposes `json` for a valid JSON body, `form`
   for a valid form body, and `raw` only for anything else
   (`crates/blastit-ingress/src/lib.rs:429-441`). The common POST endpoint is proposed as
   `"codecs": ["json"]`.
2. **The acknowledgement is carried by the tool.** A body fix sets `"allow_raw": true` whenever the
   suggestion includes `raw` (`audit_v1.rs:305-330`, wired at `:1019-1030`), and the `raw_ingress` fix
   is *only* `"allow_raw": true` with `to: "[route.ingress]"` (`audit_v1.rs:323-330`, `:1058-1072`).
   `docs/ci.md:72` shows the live document doing exactly that.
3. **`fix.to` is not grant-only.** It also names `[route.ingress]` (above), and `add_group` names
   `[group.connection.<store>].queries` (`audit_v1.rs:686-693`), not `[route.db].queries`.

**Failure mode.** A Django/Flask reader adds a JSON POST view, `check` reports `missing_ingress`, and
the page says the fix proposes `body raw` and that no tool ever grants the acknowledgement. They
therefore hand-write `body raw unvalidated` — switching off JSON canonicalisation and deploying an
unvalidated route — when the tool actually proposed `body json`. A reader whose first denial is a
`SET` looks for `[group.connection.db].queries` in a language that has no `group`, and the page
assures them `to` is always `[route.db].queries`.

**Fix.** Replace the quoted sentences with the document the tool emits:

> `fix.action` is one of `add_query`, `add_group`, `remove_query`, `add_route`, `remove_route`,
> `add_test`, `declare_ingress`, `fix_ingress` and `observe`. `fix.to` names the table to edit —
> `[route.<store>].queries` for an `add_query`/`remove_query`, `[group.connection.<store>].queries`
> for `add_group`, and `[route.ingress]` for the `raw_ingress` fix — all v1-spelled until the cutover
> re-spells them. A body fix carries `fix.codecs` (`["json"]`, `["form"]`, or `["raw"]` with
> `"allow_raw": true`): a JSON body is proposed as `body json`, not `raw`. `remove_query` and
> `remove_route` carry `"unsafe": true`. `--learn` never writes a codec and never writes
> `unvalidated`; the author confirms the acknowledgement in the file.

If the owner intends a fix to stop carrying the acknowledgement, that is a change to `Fix` plus the
protected suites in `MB.21.4` — not a sentence on this page. Until then the page must describe the
document the binary prints.

### MAJOR — a PHP project still has no named green CI step, and the documented pytest outcome is wrong

`guide:56` still opens the CI section with "One unprivileged command proves nothing your suite
exercised was left out", `guide:59` shows `uv run blastit check`, and `guide:73-76` then says a PHP
app "exits `2` without pytest; with pytest there is no PHP request audit in 0.2.0, so it reports
`no_observations` and `unexercised_route`". `reference:430-436` repeats it.

Two consequences for the 0.2.0 headline backend:

* The only command a PHP project can make green is `uv run blastit fmt --check` (`guide:74-76`), but
  it is introduced as "a separate step", never as *the* gate. The section's promise (one command
  proves completeness) is false for PHP, and the reader is left to decide whether exit `2` is a
  failure (the legend at `guide:71` says "could not start").
* "With pytest … reports `no_observations` and `unexercised_route`" is only true when the pytest run
  passes. A PHP project that follows `guide:62-63` (`uv add --dev pytest` "if the project has none")
  has no Python tests, so `pytest -q` exits `5` and `check` exits **`3`** (`cli.md:60-61`), an
  outcome neither page names.

**Failure mode.** A Symfony reader copies the section, runs the two commands, sees `2` (or `3` after
adding pytest) on a working app, and has no way to tell a passing PHP gate from a broken one.

**Fix.** Add one sentence at the end of `guide:71-76` and mirror it at `reference:430-436`:
"For a PHP app the CI gate is `uv run blastit fmt --check`; `blastit check` validates the manifest,
static roots and `libphp` but has no request audit in 0.2.0, so it exits `2` (no pytest) or `3`
(pytest collects no tests) — add pytest only if you have Python tests, and do not expect a clean
`check`." Keep the `no_observations`/`unexercised_route` sentence for the case where a Python suite
does pass.

### MAJOR — connection setup (`SET`/`SHOW`/`set_config`) has no Blastfile next step anywhere

`audit_v1.rs:686-693` turns a connection-setup denial into `add_group` with
`to: "[group.connection.<store>].queries"`. The Blastfile has no `group`, and neither page tells the
reader what to do with that fix:

* the grants table (`reference:246-254`) has no connection-setup row;
* the trait example happens to be `query "SET TIME ZONE 'UTC'"` (`reference:300-306`), but nothing
  says a trait is *the* home for setup, or that it must be attached to **every** view that reaches
  the store;
* the migration table maps `[group.<name>]` → `trait` + `use` generically (`reference:547-548`) and
  never mentions the `connection` group that `docs/ci.md:84-92` documented for v1;
* the guide teaches only `BEGIN` → `transactions` (`guide:44-45`).

**Failure mode.** The first denial for most Django/SQLAlchemy/psycopg apps is a `SET` or `SHOW` on
connect. The reader gets a fix naming `[group.connection.db].queries`, pastes it (syntax error), or
adds the statement to one view and watches a different view deny it next run. `--learn` does not
close the gap either: it writes the statement as a `query` on the view that ran it and never writes
`use` (`reference:329-331`).

**Fix.** Add one paragraph under Grants (after `reference:254`), with the 5-line example: "Connection
setup (`SET`, `SHOW`, `set_config(…)`) can run on whichever view opens the connection first, so keep
it in one trait and attach it everywhere — `trait connection { postgres { query '…' } }` … `use
connection` in every view that reaches the store. `check` reports it as `add_group` with
`to: [group.connection.<store>].queries`, the v1 table this trait replaces." No new syntax.

## MINOR

1. `reference:491-494` — the Formatting table, which the guide points at (`guide:26`), still shows
   bare `blastit fmt`. Under the guide's own install (`uv add blastit`, `guide:34`) `blastit` is in
   `.venv/bin`; a CI step copied from the table fails with "command not found". Show
   `uv run blastit fmt …`, or add one line: "from a `uv add` install, prefix every local command with
   `uv run`."
2. `reference:546` — the `[proxy.<name>]` migration row offers `postgres $DATABASE_URL` or
   `postgres <name> { host $DB_HOST; … }` and omits the direct named-URL form the Stores section
   documents at `:116`. A v1 `input = { url = "ANALYTICS_URL" }` user must invent five variables.
   Fix: add "or `postgres <name> $ENV`".
3. `reference:171-172` — "a request whose path a view covers but whose method none declares gets a
   `405`, unless a static `fallback` answers it" overstates the fallback: `:373-374` limits it to
   `GET`/`HEAD`. Add "(on `GET` or `HEAD`)".
4. `guide:48` — "Stop it and read the diff" reads as if Blastit prints a diff; it rewrites the file
   on stop (`reference:323`). Fix: "stop it with Ctrl-C, then read `git diff Blastfile`."
5. `reference:338`, `:427-428` — `check --learn` is documented only as writing. The safe first step,
   `--learn --dry-run` → `"wrote": false` (`cli.md:50`), is missing. Add it to the proposal sentence.
6. `reference:297-313` — the reference never says what a trait block may hold. State it: "a trait
   holds store grants (`postgres [name] { … }`) only; `unrestricted` and `egress any` inside one are
   refused at boot."
7. `guide:6-21` — the lead example is not in the canonical layout the page tells readers to produce
   with `fmt` (`guide:26`); a copy-paste + `fmt` rewrites every inline block. Either show the
   canonical form or add "`fmt` expands the blocks".
8. `reference:137-139` — the `url` row hardcodes `port=5432` while the `port` row says "the local
   listener port"; a named store does not get 5432 (`docs/reference/manifest.md:234-239`). Say "the
   view's listener port (5432 for the first store)" or use `<port>` in the sample.
9. `guide:92-93` — "A body sent to a view with no `body` is a `413` there, not a warning" sits under
   Deployments, implying `dev` differs. It is a `413` in both (the codec is the codec); say "in
   `dev` and production".
10. `reference:518-528` — the error table has no row for a body sent to a `body none` view (a runtime
    `413`), which is the first thing a form-login reader hits. One row: "`413` | the view's `body`".

## Resolution check

Earlier `dx` round: 1 BLOCKER, 4 MAJOR, 15 MINOR. Verified against the current text, not the report.

| id | Status |
|---|---|
| B1 `--json` kinds | **resolved** -> `reference:384-394`; matches `audit_v1.rs` and `critical_check_engine.rs:822-827`. |
| M1 PHP gate | **still open** -> `guide:73-76`; commands exist but no green gate is named and the pytest-with-no-tests case (exit `3`) is missing (MAJOR above). |
| M2 `routes`/`proxies` shapes | **resolved** -> `reference:401-417` shows the objects; no prose calls them counts. |
| M3 v1 `app` row | **resolved** -> `reference:543-545`; `app` is optional (`:18`, `:51`), so the row is correct. |
| M4 opt-outs as `check` warnings | **resolved** -> `reference:214-217`, `:274-276`; `audit_v1.rs` has no such kind. |
| m1 ambiguous `sudo` | **resolved** -> `guide:30`. |
| m2 bare `blastit fmt` | **partial** -> `guide:26`, `guide:74` fixed; `reference:491-494` still bare (MINOR 1). |
| m3 "do not run it twice" | **resolved** -> `guide:63`. |
| m4 short failure list | **resolved** -> `guide:65-67` + link. |
| m5 "route, SQL and fix" | **resolved** -> `guide:68-69`. |
| m6 "key map is one table" | **resolved** -> `guide:96-97` ("one page"). |
| m7 seccomp | **resolved** -> `guide:82-84`. |
| m8 "declares one backend" | **resolved** -> `reference:9-12`. |
| m9 "`tables` grant" | **resolved** -> `reference:280`. |
| m10 complete file vs `fmt --check` | **resolved** -> `reference:447-449` (blank line between stores). |
| m11 over-broad error line | **resolved** -> `reference:514-518` scopes it to syntax/whole-file checks. |
| m12 migration gaps | **partial** -> `reference:549` (route `groups`), `:558` (named grant), `:563-567` (paths) added; the named-store URL form is missing at `:546` (MINOR 2). |
| m13 plan ids on public pages | **answered** -> `reference:573-574`; the brief mandates the id. Still unreadable to a site reader; owner's call. |
| m14 CI needs a database | **resolved** -> `guide:63-64`. |
| m15 denial loop needs a listener | **resolved** -> `guide:41-42`; verified `crates/blastit/src/stage0.rs:3034-3058` binds every declared store for every view under `--learn`. |
| Owner D3 (`fix` vocabulary, never rubber-stamp `raw`) | **still open** -> `reference:419-425`; the paragraph contradicts `audit_v1.rs:305-330` (BLOCKER above). |

## Questions

For the owner; the settled spec does not answer these, so they are not requirements.

1. **Static prefix stripping.** `reference:458-461` pairs `static "/assets" { root "public/assets" }`;
   is the prefix stripped before joining `root`? The page never says (the complete file would look for
   `public/assets/assets/…` if not).
2. **Trailing slash on a directory.** `reference:355-357`: does `index` answer `/about` or only
   `/about/`?
3. **Path decoding.** `reference:202-205` settles case and query stripping but not whether `%2F` and
   `%2e%2e` are matched decoded or raw.
4. **`.phtml` under a static root.** `reference:360-361` refuses `.php`/`.phar`; `manifest.md:155` and
   `frameworks/php.md` also refuse `.phtml`. Which is 0.2.0? A served `.phtml` leaks source.
5. **Empty `postgres { }`.** Is it valid, and does it create a listener? `guide:41-42` depends on
   `--learn` being the only way to give a new view a listener.
6. **`--learn`'s merged view.** `reference:327-328`: when two paths under one segment saw different
   methods, does it write the cross-product (granting a method never observed)? If so, the page
   should send the reader to the multi-endpoint form.
7. **Mixed pattern lists.** Is `view x GET "/a", regex '/b'` legal? `reference:167`, `:200` show the
   two forms separately.
8. **v1 env overrides.** `BLASTIT_APP`, `BLASTIT_PRELOAD`, `BLASTIT_SCRIPT_NAME` and
   `environ["blastit.route"]` (`manifest.md:20-33`, `:70`, `:357`) have no migration row or sentence.
   Do they survive, and does the app still see the view name?

VERDICT: FAIL
