# Blastfile docs — `ai-ergonomics` review (acceptance round)

## This revision

Fresh context; everything below is judged from what is on disk in this worktree.

- `docs/blastfile.md` — **100 lines** (limit 150). The 10-line file, Local, CI/CD,
  Deployments, the key-map pointer.
- `docs/reference/blastfile.md` — **574 lines**. Grammar/tokens, sections, stores,
  both view headers, patterns, every directive, grants, the safe-function table,
  traits, learn, static mounts/precedence, `check` (document, kinds, `fix`),
  formatting, errors, the complete file, migration.
- Checked the text against the settled spec (`plan/nodes/MB/MB.21/NODE.md`), the
  protected contract (`python/tests/critical/test_check_contract.py:814-1021`),
  the engine (`crates/blastit/src/audit_v1.rs`), the ingress suggestion
  (`crates/blastit-ingress/src/lib.rs:429-441`), the empty-suite pin
  (`crates/blastit/tests/critical_check_engine.rs:907-929`), the normaliser
  (`crates/blastit-sqlpolicy/src/normalize.rs:35-110`), the safe set
  (`crates/blastit-sqlpolicy/src/analysis.rs:140-225`), `docs/VISION.md`,
  `docs/reference/cli.md`, `docs/deploy/index.md`, `docs/deploy/anything-else.md`.

**Verified, not trusted:** the nine kind names now match `audit_v1.rs:334-433`
one-for-one (the earlier false-clean path — 0.1.1 names under `schema_version 1`
— is gone); `routes`/`proxies` are shown as objects; `method` is on
`unrouted_request`; the built-in function table (`reference:285-292`) matches
`analysis.rs:140-225` exactly, including `unaccent` being absent and
`websearch_to_tsquery` present; the normalisation prose (`:259-265`) matches
`normalize.rs` (`$1`→`$?`, `IN ($?, ...)`, identical `VALUES` rows, comments); the
`raw`/`raw unvalidated` split and its boot behaviour are stated as settled.

The **language layer is one-pass writable.** The failures are in the
**verification loop**: the `fix` object's fields and the last mile from a finding
to a Blastfile edit, one impossible sample document, and the deploy pointer the
guide still offers (plus the CLI page beside the reference in the nav), both
still 0.1.1.

## BLOCKER

### B1 — The PHP `check` outcome names a finding 0.2.0 cannot emit

**Where.**

- `docs/blastfile.md:73-75`:
  > with pytest there is no PHP request audit in 0.2.0, so it reports
  > `no_observations` and `unexercised_route`.
- `docs/reference/blastfile.md:432-434`: the same sentence plus `and exits 1`.

**Why it is false.** A PHP app has no WSGI gate, so its run writes no
`http_obs.jsonl`; `check.rs:260-267` treats that as zero requests.
`audit_v1::unexercised_routes` (`audit_v1.rs:944-951`) returns early when
`observations.http().is_empty()`, and the protected engine test pins exactly that
behaviour: `an_empty_suite_is_no_observations_and_suppresses_unexercised_routes`
(`crates/blastit/tests/critical_check_engine.rs:907-929`). The only findings are
`no_observations` (`audit_v1.rs:730-735`) plus `proxy_unused` for each declared
store (`:736-744`) — never `unexercised_route`.

**Failure modes.**

1. An agent writing the documented PHP gate filters the JSON for
   `unexercised_route` and waits for a finding that never arrives.
2. A PHP-only project that adds pytest to silence exit `2` gets pytest's `5`
   ("no tests collected") → `SUITE_FAILURE` → exit `3`, no document at all. The
   page's sentence reads like a document will be there.

**Exact fix.** Replace both sentences with:

> with pytest, the suite runs and the WSGI gate records no request (0.2.0 has no
> PHP request audit), so `check` reports `no_observations` — and `proxy_unused`
> for each declared store — and exits `1`. A suite that collects no test is exit
> `3`. Do not add pytest to silence the `2`.

### B2 — The `fix` object's documented shape is wrong, including the acknowledgement

**Where.** `docs/reference/blastfile.md:419-425`:

> `fix.to` is present only on the actions that edit a grant ... A body fix carries
> `fix.codecs` ... A body fix proposes `body raw`; the author adds `unvalidated`
> deliberately, because the acknowledgement is never granted by a tool.

**Why it is false, against the code the page tells an agent to filter.**

- `fix.to` **is** present on `raw_ingress`: `Fix::fix_raw_ingress` sets
  `{"action": "fix_ingress", "to": "[route.ingress]", "allow_raw": true}`
  (`audit_v1.rs:322-328`, serialised at `:248-256`). `[route.ingress]` is not a
  grant.
- `fix.allow_raw` **is** emitted and is not documented: `Fix::declare_ingress` /
  `fix_ingress` carry it whenever a raw codec is proposed (`audit_v1.rs:306-320`),
  and `suggest_fix` sets `allow_raw = codecs.contains(Raw)`
  (`crates/blastit-ingress/src/lib.rs:429-441`). That flag is the v1 spelling of
  the acknowledgement the migration table itself maps at `:551`.
- Therefore "the acknowledgement is never granted by a tool" is false: a body
  finding for bytes that are neither JSON nor form proposes it.

**Failure modes.** An agent that automates `fix` has no key for the one
security-relevant decision here. If it takes "proposes `body raw`" and misses the
following clause, it writes `body raw` (unacknowledged) and the **production
boot refuses the whole file** — the exact 0.2.0 refusal the page defines at
`:224-229`. If it takes "never granted by a tool" literally, it ignores the
`allow_raw: true` the document actually contains and cannot explain it.

**Exact fix.** Replace the paragraph with a key table and one sentence:

```text
| fix.to         | add_query, add_group, remove_query, and fix_ingress for a raw view |
|                | a v1 manifest path: [route.<store>].queries,                       |
|                | [group.<name>.<store>].queries, or [route.ingress]; never Blastfile syntax |
| fix.codecs     | declare_ingress, fix_ingress: ["json"], ["form"] or ["raw"]        |
| fix.allow_raw  | the same two, when a raw codec is proposed: true                   |
| fix.unsafe     | remove_query, remove_route: true                                   |
```

> A body fix proposes the codec the bytes need. When that is `raw`, the fix also
> sets `allow_raw: true`, the v1 spelling of `body raw unvalidated`. `--learn`
> never writes `raw` or `unvalidated`; take or reject the acknowledgement
> deliberately.

## MAJOR

### M1 — No action→Blastfile edit mapping: `add_group`, `observe`, and "paste `sql`" are the last mile

**Where.** `docs/reference/blastfile.md:384-399` (kinds), `:419-425` (actions),
`:246-254` (grants), `:259-265` (query), `:274-276` (`unrestricted`); the guide's
only mechanical loop is `docs/blastfile.md:42-45` ("paste it as `query '…'`").

**Failure modes.**

1. **`add_group` is common.** Any driver's `SET`/`SHOW`/`set_config` is mapped to
   `{"action": "add_group", "to": "[group.connection.<store>].queries"}`
   (`audit_v1.rs:689-693`). Blastfile has **no global attach**: a trait grants
   only through `use` per view (`reference:308-312`), and `--learn` never writes
   `use` (`:331`). The page never says the edit is a `trait connection { … }` plus
   `use connection` in **every** view, so an agent invents a trait name, attaches
   it to one view, or reaches for `postgres unrestricted` to get a green `check`.
2. **`observe` is not an edit.** It is the fix for every transaction-control
   denial (`sql` is the label `BEGIN`/`COMMIT`/…), every redacted literal
   (`…`), `no_observations`, and `proxy_unused` (`audit_v1.rs:677-697`,
   `:270-272`). The page lists it in the enum and never says what to do. The
   knowledge exists at `:248` (`transactions`) and `:264-265` (`$?`), but not at
   the finding.
3. **`sql` is not always pasteable.** The guide's "paste it as `query '…'`" is
   correct only for `add_query`. Pasting a redacted `sql` writes a grant with a
   literal `…`; pasting `BEGIN` grants a transaction label as a statement. Both
   then surface as `dead_grant`, i.e. whack-a-mole across a 50-route app.

**Exact fix.** Add an "Applying a finding" table immediately after the kind table:

```text
| add_query      | add query '<sql>' inside the view's postgres { } block; sql is already normalised ($? per value) |
| add_group      | the statement is connection setup and runs on whichever view opens the connection first: write it once in trait connection { postgres { query '<sql>' } } and use connection in every view (--learn never writes use) |
| remove_query   | delete that query line, or the trait entry when the finding carries group; unsafe is set |
| declare_ingress / fix_ingress | body json|form from fix.codecs; when fix.allow_raw is true the bytes are neither, so body raw unvalidated after reading why |
| add_route      | add view <name> <method> "<path>" { … } from the finding's method and path |
| add_test       | add a request to the suite, or delete the view; unexercised_route has no other opt-out |
| observe        | not an edit: a transaction label is transactions; a redacted sql is a value to bind, then --learn again; otherwise read the log line |
```

and one sentence: "`sql` is pasteable only when `fix.action` is `add_query`."

### M2 — The `--json` sample is not a document the engine can emit

**Where.** `docs/reference/blastfile.md:401-417`.

**Why.** It shows `"routes": {"total": 1, "exercised": 1}` beside findings for
**two** views (`catalog` and `checkout`), and
`"proxies": {"db": {"statements": 1, "denied": 0}}` beside a `missing_grant` on
that same proxy. `routes.total` is `manifest.route_count()` (`audit_v1.rs:754-757`);
`denied` is incremented for every denied statement on a declared store before the
finding is built (`:649-653`). Both values are impossible.

**Failure mode.** This is the page's one golden document, and the earlier round
asked for it precisely so an agent would build a verifier from it. An agent that
encodes the sample's invariants (`total` may be smaller than the number of route
names, `denied` may be `0` while `missing_grant` exists) writes a validator that
rejects real output — or accepts an already-broken report.

**Exact fix.** Make it a real document: `"routes": {"total": 2, "exercised": 1}`
and `"proxies": {"db": {"statements": 1, "denied": 1}}`. Keep the three findings;
with two routes and two requests (one routed, one not) every count is consistent.

### M3 — The guide's Deployments link is still 0.1.1 syntax, unlabelled

**Where.** `docs/blastfile.md:94`:

> The recipes are in [Deployments](deploy/index.md).

**What that page says.** `docs/deploy/index.md:55`: "a view dials no TCP socket
unless its route sets `egress = true`" — a `=` assignment and the v1 spelling
(`egress any`); `docs/deploy/anything-else.md:26` and `:138` repeat it;
`anything-else.md:216` says "`WorkingDirectory` is where Blastit finds
`./blastit.toml`". `docs/reference/cli.md:10` (in the same Reference nav, one row
above the Blastfile reference) says all three commands read `./blastit.toml`.

**Failure mode.** This is the earlier M4 one link over. The guide labels
`local.md` "(0.1.1 page, rewritten in this release)" (`:33-34`) but not the
Deployments page, so an agent told "deploy it" follows the guide's only deploy
pointer, copies `egress = true` into the `Blastfile` (`=` is a parse error) and
looks for `./blastit.toml` inside a container that has a `Blastfile`. Neither
`docs/deploy/**` nor `reference/cli.md` is in the RESULT's rewrite list.

**Exact fix.** Label the link exactly as `local.md` is labelled, or drop it until
MB.21.5.1 and state the three deploy facts inline (production fails closed;
`blastit --preset compose-traefik`; a body with no `body` is `413`). Add
`docs/deploy/**` and `docs/reference/cli.md` to MB.21.5.1's paths.

## MINOR

1. **`unrestricted`'s deterrent is factually backwards.**
   `reference:274-276`: "It replaces no missing grant and warns at boot." Adding
   `postgres unrestricted` makes every `missing_grant` disappear and `check`
   pass — that is exactly what an agent optimising the audit wants, and
   `VISION:26` names this failure. Rewrite: "It is not a fix for a missing
   grant: `check` passes with it because every statement is allowed, so the
   audit can no longer tell you what the view needed. It warns at boot." This is
   the highest-value MINOR here.
2. **`check --learn`'s scope is missing.** `reference:338-339` says it "prints a
   proposal instead of findings" but not that it proposes **new routes only**;
   a statement grant on an existing view is still the 0.2.x gap (`VISION:25`).
   An agent that runs `check --learn` expecting its `missing_grant` set to be
   added gets nothing. Add: "on an existing view, no statement grant is
   proposed."
3. **The `--learn` proposal's `wrote` is shown as always `true`.**
   `reference:427-428` prints `"wrote": true`. It is `true|false` (dry run, a
   refused statement) and a refusal exits `1` with the proposal
   (`test_check_contract.py:1050-1056`). Say both values, that the document has
   no `schema_version` and no `findings`, and that `false` means nothing was
   written.
4. **The unnamed default store has no name in the machine keys.**
   `reference:110-111` ("without a name a store is the default one") versus
   `:405-410` (`"proxy": "db"`) and `:421-422` (`[route.db].queries`). Say which
   name the unnamed `postgres` takes in `proxy`, `proxies` and `fix.to`, or stop
   printing `db` until MB.21.4 pins it. An agent cannot join a finding to its
   own store otherwise.
5. **A trait's `dead_grant` `to` is a different path.**
   `reference:421-422` shows only `[route.db].queries`; a trait's is
   `[group.<trait>.<store>].queries` (`audit_v1.rs:935`), and `remove_route` is
   listed in the enum (`:420`) though 0.2.0 constructs none. Print the group form
   and either emit `remove_route` or mark it reserved.
6. **The Stores output table's left column is the v1 slot, not the variable.**
   `reference:133-143` starts at `url`, `host`, `port`, while the DSL's variable
   is the `$IDENT` the author wrote (`$DATABASE_URL`, `$DB_HOST`, …). An agent
   can read `url` as a required variable name. Retitle the column "slot (the
   `$IDENT` you wrote)" and use the split-form variables in the rows.
7. **`--json`'s stdout discipline is unstated.** `reference:381-382` says stdout
   is empty on `2`/`3`, but not that on `0`/`1` stdout is the document and
   **nothing else** — the suite's output moves to stderr
   (`test_check_contract.py:832-838`). One sentence lets an agent call
   `json.loads(stdout)` without a heuristic.
8. **The guide's failure list is still not the nine kinds.** `guide:65-67` names
   five and omits `missing_ingress` (the finding `--learn` never prevents) and
   `raw_ingress`. It links the reference, but an agent reading only the guide
   will not know an undeclared body is a failure at all.
9. **Mixed pattern forms are unstated.** `reference:200-205` gives `regex` and a
   quoted pattern, and `:25` leaves `PATTERN` undefined; whether
   `GET "/a", regex '/b'` is legal is not said (the RESULT admits it is
   unanswered). Also name the error for a lookaround/backreference: syntax error
   at `line:col`, or a whole-file refusal? An agent porting a v1 path list has
   to guess both.
10. **The `workers` migration has no action.** `reference:555` and `:573-574`
    say "no spelling; MB.21.2 settles". A migrating file needs the 0.2.0 action:
    "omit it; the cap does not exist in 0.2.0." Without that the agent cannot
    tell drop from invent.
11. **`observed` is a MIME essence with no codec mapping.**
    `reference:389-390` carries `observed`, `:423` carries `fix.codecs`; the
    codec table (`:211-215`) does not say `application/json`→`json`,
    `x-www-form-urlencoded`→`form`, anything else→`raw`. It is the one field a
    `missing_ingress` fix turns on.

## Resolution check

| id | Earlier finding | Verdict |
|---|---|---|
| M1 | `--json` named but not specified; 0.1.1 filter returns a false clean | **resolved** -> `reference:384-399` (nine v1 kinds and what each carries), `:401-417` (sample — but impossible, see M2), `:386-390` (counts as objects), `:396-397` (`severity`), `:381-382` (empty stdout on `2`/`3`), `:392` (`method`), `:427-428` (proposal). The earlier rename table is unnecessary: the names are the ones the code emits. |
| M2 | `fix` vocabulary undefined | **partially resolved** -> `reference:419-422` (enum, `to` optional), `:423-425` (`codecs`, `unsafe`). Still open: `fix.allow_raw` absent and two false claims (**B2**); no action→edit mapping (**M1**). The RESULT's D3 row overstates this. |
| M3 | PHP gate cannot run; `check`'s PHP exit undefined | **gate resolved, outcome false** -> `guide:72-76`, `reference:430-436` give `fmt --check` = 0 and `check` = 2 after the preflight, and "plus the boot" is gone. **B1**: `unexercised_route` is never emitted, and a no-test suite is exit 3. |
| M4 | The guide's links teach 0.1.1 TOML | **partially resolved** -> `guide:33-34` labels `local.md`; the `ci.md`/`php.md` links are gone; `zensical.toml:44` says "The manifest (0.1.1, retired)". Still open -> `guide:94` → `deploy/index.md` (`egress = true`, `anything-else.md:216` `./blastit.toml`) and `reference/cli.md:10` (**M3**). |
| M5 | The only upload answer cannot deploy; the same-file promise | **resolved** -> `reference:214-215` (`raw` = unacknowledged), `:224-229` (`raw unvalidated` deploys with a boot warning; `--learn` writes neither), `:550-552` (migration rows), `guide:90-93`. The guide's same-file sentence is now true for the acknowledged form. |
| m1 | Exact-`query` matching hidden | **resolved** -> `reference:259-265` (comments/whitespace, `$1`≡`$?`, `IN` collapse, identical `VALUES`; literal caveat). |
| m2 | `app` required in the table, optional in the grammar | **resolved** -> `reference:17` (`app?`), `:51` ("one, optional"), `:59-61`. |
| m3 | "the boot line prints it" | **resolved** -> `reference:59-61` names the app and the callable, claims no banner output. |
| m4 | Guide's failure list short; no severity | **mostly resolved** -> `guide:65-68` + `reference:384-399`; the guide still omits `missing_ingress`/`raw_ingress` by name (MINOR 8). |
| m5 | `regex` dialect and list mixing | **partially resolved** -> dialect at `reference:202-203` (linear time, no lookaround/backreferences); a mixed quoted/`regex` list is still unstated (MINOR 9). |
| m6 | A header-less multi-endpoint view matches nothing | **resolved** -> `reference:187-188` ("declare at least one `METHOD PATTERN`"). |
| m7 | `check`'s stderr line is not a finding | **resolved** -> `reference:434-435` (advisory, changes no finding). |
| m8 | "Click through the app" is human-only | **resolved** -> `guide:47-51` ("replay your suite or a few `curl` calls against `127.0.0.1:8080`"). |
| m9 | "no `=`" can be misread as banning `=` in SQL | **resolved** -> `guide:23-25` ("`=` between a keyword and its value ... `=` inside a quoted SQL statement is data"). |
| m10 | `--learn` proposal document missing | **partially resolved** -> shape at `reference:338-339`, `:427-428`; `wrote` values and the `check --learn` scope are still missing (MINORs 2-3). |
| m11 | `app` in the boot event vs the app rule | **resolved** (merged into m3) -> `reference:59-61`; no event claim remains. |

RESULT's `D1-D8`: D1, D2, D4, D6, D7, D8 verified in the text. **D3 is
overstated** (no `allow_raw`; see B2) and **D5 is overstated** (the deploy and
CLI pages are still unlabelled; see M3).

## One-pass integration walk

A 50-route Python/Postgres JSON app:

1. Copy `guide:6-21`; write `app`, `entrypoint python`, `postgres $DATABASE_URL`,
   one `view` per route with explicit methods (`reference:159-188`).
2. `blastit dev --learn` with the suite or scripted `curl`; stop; `blastit fmt`.
3. Add `body json` per POST endpoint, `use`, `transactions` — the docs say
   `--learn` writes none of these (`guide:51`, `reference:330-331`).
4. `blastit check --json`; `routes`/`proxies` are objects, the kinds are the
   ones the engine emits, `unrouted_request` carries `method`.
5. Close findings: `add_query` → `query '<sql>'`; `declare_ingress` → `body`;
   `add_route` → a `view`; `remove_query` → delete. Here it breaks: `add_group`
   (the driver's `SET`/`SHOW`) has no documented Blastfile edit and is common
   (M1); `observe` is opaque (M1); the sample's counts cannot be used as a
   golden fixture (M2).
6. `blastit fmt --check` must be 0; `check` never rewrites (`guide:75-76`).
7. Deploy: `blastit --preset compose-traefik` fails closed correctly. If the
   agent follows `guide:94`, it is back in `blastit.toml` and `egress = true`
   (M3).

For a **PHP** app the walk cannot reach a green `check`: the docs promise
`no_observations` + `unexercised_route` (B1), and the only exit-0 step is
`fmt --check`.

The **language** is one-pass: no syntax has to be invented, the pattern/store/
static/error tables are sufficient, and the safe-function set is exact. The
**verification loop** is where a one-pass agent loses the pass.

## Questions

1. **The unnamed default store's JSON name.** The sample asserts `db`, and the
   migration maps `[group.<name>.<proxy>]`; nothing in the settled spec names an
   unnamed store. Which name will `proxy`, `proxies` and `fix.to` use? (Prose
   only; no new syntax.)
2. **Mixed pattern forms.** Is `GET "/a", regex '/b'` legal? If yes, say so; if
   not, say which error an agent will see, so a ported v1 path list has a
   documented move. (No new syntax.)
3. **`dev --learn` on an existing view.** `VISION:25` says `dev --learn` writes
   grants, while `check --learn` proposes new routes only. The reference's
   `--learn` table (`:326-332`) is written for `dev` and implies both. Confirm the
   existing-view grant is in 0.2.0's `dev --learn`, or say it is the 0.2.x gap.
4. **`egress any` for an app that calls an external API.** It is the only
   0.2.0 outbound path and opens every address and port. Is such a route meant to
   deploy in 0.2.0? If yes, say "this is the SSRF escape; prefer no deploy until
   the HTTP proxy lands"; if no, say "do not deploy this view".
5. **`[route.php] workers`.** Confirm the 0.2.0 migration action is "omit it"
   (rather than "wait for MB.21.2"), so the reference can state one instruction.

VERDICT: FAIL
