# AI-ergonomics review — docs/deploy/index.md

## Round-4 verdict (targeted verification)

**PASS** — 0 BLOCKER, 0 MAJOR, 1 MINOR (plus the owner-URL item declined by line
budget). Re-run in this worktree: `uv run --no-project python e2e/docs_deploy.py` →
`docs_deploy: 42/42 documented pooler promises hold`, exit 0; page still 120 lines.

1. **Closing paragraph `:56-60` — resolved.** The rule is now explicit (remove the
   `dead_grant` on a database that says `UTC`; keep it only where `check` runs against
   one that says otherwise) and agrees with `digitalocean-paas.md:85-88` ("dead_grant
   here", "Keep it for CI, whose Postgres says Etc/UTC") and
   `compose-traefik.md:206-208`. `[CI](../ci.md#what-it-fails-on)` resolves to
   `ci.md:20`. `ci.md:26` still says "cover that code path, or remove the grant" — a
   superset, not a contradiction. The sentence names the grant precisely enough to act
   on (`set_config('TimeZone', …)`), so an agent can find the `[proxy.db]` query.
2. **Item 1 `:64-66` — resolved.** "on App Platform only DigitalOcean reaches it"
   matches the preset table at `:11` ("DO's edge and its own services"); no peer class
   is dropped, and the `*` half still names the mechanism.
3. **Item 2 `:67-70` — accurate, one MINOR stays.** "minimum *server* connections" now
   matches `docs/research/connection-pooling.md:35` (DO's `size` is "the minimum number
   of connections the pool can handle") plus PgBouncer's server-connection vocabulary.
   Surviving MINOR (wording only): the client-count half still names
   `BLASTIT_MAX_WORKERS` where `settings.md:145` says the per-instance upstream cap is
   `BLASTIT_PROXY_<name>_MAX_CONNECTIONS` (`= MAX_WORKERS`, so it is true by default).
4. **Harness `e2e/docs_deploy.py:239-242` — tightened as asked, one gap.** It now
   asserts `'psql "$APP_URL" -A -t -c' in flat and flat.count('"$OWNER_URL"') == 1`.
   Mutation-tested on a temp copy with `INDEX` monkeypatched (page untouched):
   a second owner `-A -t -c 'SHOW TimeZone'` → **FAIL** (intended); deleting the owner
   command → **FAIL**. MINOR gap: *replacing* the `ALTER` with
   `psql "$OWNER_URL" -c 'SHOW TimeZone'` keeps the count at 1 and **PASSES**, so the
   message ("the only owner command is the `ALTER`") overclaims by one clause:
   `and "ALTER ROLE app SET timezone" in flat`.

Declined by line budget (not graded): `$OWNER_URL` is not exported on this page
(`:44`); it is `digitalocean-paas.md:66`, one hop from item 3's
`#cluster-roles-and-the-pooler`, and the page is at the node's 120-line ceiling.

---

## Round-3 verdict (verification)

**PASS** — 0 BLOCKER, 0 MAJOR, 3 MINOR. The round-2 `declined-by-node` MAJOR (R1, the harness
contract failure) is closed on evidence, not on trust. Run in this worktree:

```console
$ uv run --no-project python e2e/docs_deploy.py
docs_deploy: 42/42 documented pooler promises hold        # exit 0
$ uv run --no-project python python/tests/runnable_docs/run.py
49 fences: 27 executed, 22 accounted, 0 failed · 32.3s
$ make docs-check
Build finished in 0.20s                                   # zensical --strict, "No issues found"
```

Page is 120 lines (trailing blank line), one `OWNER_URL` occurrence.

### 1. The harness asserts the page as it stands, with nothing tautological

- `e2e/docs_deploy.py:228-233` — `0 <= flat.find("export APP_URL=") < flat.find('psql "$APP_URL"')`.
  `find` returns `-1`, so a *deleted* export FAILs instead of raising `ValueError`; nothing can
  satisfy this with empty text.
- `:234-242` — the regex pins `export APP_URL='postgresql://app:<pw>@<host>:25060/defaultdb`, so a
  regression to the pool (`25061`), to `migrator`, or to a pool-named path (`shop-pool`) turns
  red; the app `-A -t` read is required and the owner `-A -t` oracle is refused.
- `:243-256` — Compose's `-U app -d shop` read, the "Both recipes already put the zone on the
  role" + `db/grants.sql` provenance, and the "only the app's `DATABASE_URL` names the pool /
  migrations and grants name the database" rule. All three are content assertions; the two-URL
  rule cannot regress silently through the export, the URL regex, or the prose pin.
- **MINOR** — `:240` now asserts only the *absence* of `psql "$OWNER_URL" -A -t`. Because
  `index.md:45` legitimately holds `psql "$OWNER_URL" -c "ALTER ROLE …"`, an owner
  `-c 'SHOW TimeZone'` read would pass. Cheapest strengthening:
  `flat.count('"$OWNER_URL"') == 1` plus the ALTER on that same snippet.

### 2. m1–m4 resolved; two residue MINORs, no new ambiguity that blocks a step

- **m1 resolved.** `index.md:48` names "the `app` user on the direct port (25060), not the pool"
  and `:49` pins `postgresql://app:`, so a doadmin credential now fails authentication loudly
  instead of printing a false `UTC`. Residue (MINOR): the label dropped *where* `app`'s password
  comes from (Connection Details' **User** / the pool URI); the pool URI at `:27`/`:33` carries
  it, one hop away — no false pass, so not re-graded MAJOR.
- **m3 resolved.** `:44-45` is now conditional with a runnable branch, and `:57-60` no longer
  states an action the recipes already performed; `[CI](../ci.md#what-it-fails-on)` resolves to
  `ci.md:20` `## What it fails on`. Residue (MINOR): `$OWNER_URL` is defined on neither page's
  fence — `digitalocean-paas.md:66`, inside the section `index.md:74` links. With only
  `index.md` open an agent gets `psql: error: could not translate host name ""`; on Compose the
  owner route is `docker compose exec db psql -U owner …`, not `$OWNER_URL`. An agent holding the
  DO recipe *does* understand it (`:62-66`), so the cost is one grep — MINOR, not MAJOR. A
  six-word parenthetical ("DO: `$OWNER_URL` from the
  [recipe](digitalocean-paas.md#cluster-roles-and-the-pooler)") removes it.
- **m4 resolved.** The Compose read-back is now a commented line inside the same fence (`:51`) and
  keeps `-A -t`, so a fence-collecting reader sees the command; inventory fence 2 stays `static`
  and validates the psql flags.

### 3. Machine-reader defects in the four-item list, ingress, or checklist

- Four-item list (`:62-77`): `#cluster-roles-and-the-pooler`, `#compose`,
  `#measure-your-ingress` all resolve; `make docs-check` (`--strict`) passes. No new defect.
- Ingress (`:79-107`): both `curl` lines label the preset and its header; both log commands are
  labelled and `tail -1`'d, so grep-the-fence is unambiguous. Pre-existing (not new): `tail -1`
  can show a concurrent request and the prose does not say the newest line is the one you sent —
  MINOR only.
- Checklist (`:109-115`): unchanged, still matches `log-events.md` (`stage0` privilege, second
  `boot` with `stage: config` → `preset`), recorded as declined-by-node below.
- MINOR (harness prose): `inventory.toml`'s index fence-2 reason still calls `APP_URL` "a
  container on a Compose network", but that line is now `-U app -d shop` with no `APP_URL`. The
  gate passes; only the printed accounting text is stale.

Surviving **MINOR** at `index.md:45` (undefined-in-place, DO-shaped `$OWNER_URL`),
`index.md:68-70` (m2 unchanged: names `BLASTIT_MAX_WORKERS` rather than
`BLASTIT_PROXY_<name>_MAX_CONNECTIONS`; settings.md:152-154 makes the default true, so wording
only), and `e2e/docs_deploy.py:240` (owner oracle narrowed to `-A -t`). None is BLOCKER/MAJOR;
no `declined-by-node` MAJOR remains.

---

## Round-2 verdict

**PASS** — 0 BLOCKER, 0 page-level MAJOR, 4 MINOR, plus 1 MAJOR in
`e2e/docs_deploy.py` tagged `declined-by-node` (this round's `paths` stop at
`docs/deploy/index.md` + `python/tests/runnable_docs/inventory.toml`; the
harness owner must file it as a Follow-up — see R1, it is red **today**).

The round-1 BLOCKER is gone: the pooler section no longer interleaves an
unrunnable command per preset. Every step is executable on the preset it
names, or says there is nothing to run, and the only DO command left
(`:48-49`) is valid against both the direct port and the app role. An agent can
walk the page top to bottom without hitting `docker: command not found` or
`role "app" does not exist`.

Evidence run in this worktree:

```console
$ uv run --no-project python e2e/docs_deploy.py
FAIL  index: both URLs are exported before the commands that read them
FAIL  index: the app and the one-off jobs are described as two destinations
docs_deploy: 39/41 documented pooler promises hold        # exit 1
$ uv run --no-project python python/tests/runnable_docs/run.py
49 fences: 27 executed, 22 accounted, 0 failed · 20.0s
```

Checked and true this round: 25061/25060 vs `pgbouncer:5432`/`db:5432`;
`*` on both presets; DO roles default to `UTC` (`digitalocean-paas.md:85`);
Compose's `db/grants.sql` runs `ALTER ROLE app SET timezone = 'UTC'`
(`compose-traefik.md:195`); a DO pool URI's path is the pool name
(`docs/research/connection-pooling.md:33`); Connection Details has both a
Database/Pool and a User selector, so "both come from Connection Details"
(`:33-34`) is now accurate; `stage0` carries `preset: null`
(`log-events.md:48-53`); all link targets and anchors exist; 119 lines.

---

## MAJOR — declined-by-node (out of this round's paths)

### R1 — `e2e/docs_deploy.py:228-234` and `:248-251` — two index promises are now stale, so `make e2e` fails

The page correctly dropped the DO `ALTER` and renamed the section, but the
harness still asserts the old text:

```python
"export APP_URL=" in flat
and flat.index("export APP_URL=") < flat.index('psql "$APP_URL"')
and "export OWNER_URL=" in flat
and flat.index("export OWNER_URL=") < flat.index('psql "$OWNER_URL"')

"Two destinations" in flat,
```

`export OWNER_URL=` and `Two destinations` no longer appear
(`index.md:23` is "Two databases, two URLs"), so `docs_deploy.main()` returns
1 and `e2e/run.py:111` aborts the release-gate e2e run. This is a contract
failure, not a page failure: the page is right, and the two assertions
describe the pre-pooler rewrite. Suggested shape (which also removes the
`flat.index` raise-instead-of-FAIL mode):

```python
"export APP_URL=" in flat and flat.index("export APP_URL=") < flat.index('psql "$APP_URL"')
"Two databases, two URLs" in flat and "the one-off grant" in flat
```

`declined-by-node` means **only** that `e2e/**` is outside this round's
`paths`. If no node with `e2e/**` is scheduled before the release gate, this
must be re-graded FAIL, not waived.

---

## MINOR

### m1 — `index.md:47` — "from Connection Details" does not say to pick the `app` user

> `# DigitalOcean, 25060: the app role, from Connection Details`

Connection Details defaults to `doadmin`; the `app` credentials appear only
after the **User** dropdown is switched to `app`. Both URIs are on 25060 and
both print `UTC`, so an agent that pastes the panel's default password gets
`UTC` from the wrong role and closes the section on a false pass. One clause
("pick `app` in **User**") removes the last way to "pass" this check without
testing it.

### m2 — `index.md:68-71` — the sizing sentence names the wrong knob

> Each worker holds one upstream connection (`BLASTIT_MAX_WORKERS`, 16 by
> default), so workers × instances is the client count the pool must accept

`settings.md:151-155` says the per-instance connection count is
`BLASTIT_PROXY_<NAME>_MAX_CONNECTIONS` ("keep it at the worker count"), the
pool's *client* limit is `MAX_CLIENT_CONN` (100), and `size`/`DEFAULT_POOL_SIZE`
is its server connections. As written, an agent sizing a DO pool from `16 × 2`
can conclude the recipe's `size 18` is undersized and raise it.
"`BLASTIT_PROXY_<name>_MAX_CONNECTIONS` × instances clients; the pool's `size`
is its server minimum" costs the same.

### m3 — `index.md:38-61` — "nothing to run" then "Set the zone before the first check"

> so every pooled session inherits it and there is nothing to run.
> … Set the zone before the first `uv run blastit check`.

On both recipes the second sentence describes work the recipes already did, so
a skimming reader sees a prerequisite with no action. It is only an
instruction on a database that is not `UTC` — exactly the `Etc/UTC` case the
page (correctly, per the node) no longer explains, where `:52` "must print
exactly `UTC`" and the `dead_grant` rule disagree and no command is offered.
A conditional ("If it does not say `UTC`, set it on the role before the first
`uv run blastit check`") costs six words and covers the `anything-else` reader
without reintroducing the `options` guardrail.

### m4 — `index.md:52-53` — the Compose read-back lives in prose, and presumes a running stack

> It must print exactly `UTC`; on Compose read `db` the same way:
> `docker compose exec db psql -U app -d shop -A -t -c 'SHOW TimeZone'`.

An agent that collects fenced `console` blocks (the convention the
runnable-docs inventory encodes) never sees this command, and nothing on this
page says the Compose stack must be `up` before the read-back — the recipe
page owns that. Acceptable if the line budget needs the fence slot; otherwise
add it as a commented line inside the existing fence.

---

## Declined by node (recorded, not graded)

- **M5 / `index.md:111-113`** — `config` is a `stage` on the second `boot`
  line (`log-events.md:43-60`), so grepping `"event":"config"` finds nothing.
  The checklist must not change, so this stays. Cheapest future fix: reuse the
  two log commands already on the page (§2), so the reader filters
  `"event":"boot"` once and reads `stage`.
- **`Etc/UTC` sentence** — moved out; the follow-up to `docs/ci.md` is the
  right owner (`paths` exclude it).
- **`options`-in-`DATABASE_URL` guardrail** — deliberately not re-added; with
  the zone stated as already on the role, nobody is routed to `options`, and
  `reference/manifest.md` owns the rule. m3 covers the residual case without
  restoring it.

---

## Resolved since round 1

B1 (preset-labelled, runnable pooler steps), M1 (both URLs, no unresolved
"the second"), M2 (`uv run blastit check`), M3 (link to
`ci.md#what-it-fails-on`), M4 (log commands for both presets, sample labelled
trimmed), m1–m5 (routed-route marker, concrete headers, "the one-off grant",
preset table not duplicated, privilege sentence) — all verified in the file,
not taken on trust.
