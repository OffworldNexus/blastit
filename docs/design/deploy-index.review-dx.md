# DX review — docs/deploy/index.md (rounds 2–4)

## Verdict (round 4)

**PASS** — 0 BLOCKER, 0 MAJOR, 2 declined-by-line-budget, no new findings.

The four edits close R3-M1 and both item-list contradictions:

- `:56-60` now answers keep vs. remove: remove the grant where `check` targets
  the deployed `UTC` database (no `set_config` path exists there to cover), and
  keep it only where `check` runs against a database that reports another
  spelling. That is the `Etc/UTC` case `digitalocean-paas.md:87` documents for
  CI; the sentence no longer offers "cover it" as if it were possible on the UTC
  database. The conditional also makes the "before your first check" premise a
  scenario, not an instruction, so the checklist order no longer contradicts it.
- `:65-66` "only DigitalOcean reaches it" removes the "edge is the only peer"
  contradiction with `:11` "DO's edge and its own services".
- `:68-70` "minimum *server* connections" fixes the 16-workers-vs-`size 18`
  client/server ambiguity.
- `:38-39` drops "every pooled session inherits it"; step 2 already says open
  backends keep their login zone, so step 1 no longer overclaims.

No new contradiction: the ingress section, step 3 and the checklist are
unchanged; the page is 120 lines and both gates are green.

Declined by line budget (not graded): `$OWNER_URL` is the DO recipe's variable
(`digitalocean-paas.md` step 3, linked from item 3) and the Compose env; DO's
managed pool has no restart, and step 3 states the creation order
("DO creates the pool after the role").

Evidence: `uv run --no-project python e2e/docs_deploy.py` -> 42/42;
`make docs-check` -> no issues.

---

## Verdict (round 3)

**FAIL** — 0 BLOCKER, 1 MAJOR, 4 MINOR, 4 declined-by-node.

Round 3 removes the contradiction it was sent to remove: `:38-40` no longer says
"there is nothing to run" and `:57-60` no longer orders the reader to set the
zone; step 2's remedy is conditional (`:42-45`). The Compose read-back moved into
the fence (`:51`), the log fence is labelled and tails (`:95-96`), the settings
link drops `#presets` (`:21`), item 3 links both recipes (`:74-75`), and the
ingress remedy now names the header the edge **sets** (`:105-107`). Evidence run
in this worktree:

```console
$ uv run --no-project python e2e/docs_deploy.py
docs_deploy: 42/42 documented pooler promises hold
$ make docs-check
Build finished in 0.18s
```

What survives is the keep/remove answer. `:57-60` says the grant is a
`dead_grant` on a database that says `UTC`, then says CI "names the fix: cover
it, or remove it". On that UTC database "cover it" is **not** the keep case: the
app sends no `set_config` there (`digitalocean-paas.md:85-87`), so the real code
path cannot be covered; the keep case is "`check` runs against a database that
reports another spelling — a fresh `postgres:18` says `Etc/UTC`"
(`digitalocean-paas.md:87`). The page never says which database `check` runs
against, and the linked CI section (`ci.md:24-26`) only repeats the generic
"cover that code path, or remove the grant". A skimmer still cannot decide, and
both wrong choices produce a red `check`: remove -> `missing_grant` in CI; keep
-> `dead_grant` against the deployed database.

**Fix (one clause, the round-2 R2-M1 fix minus its dropped half):**

> Both recipes set the zone on the deployed database, so a `connection` grant
> for `set_config('TimeZone', …)` is a `dead_grant` against it. Keep it only if
> `check` runs against a database that reports another spelling — CI's fresh
> `postgres:18` says `Etc/UTC` ([CI](../ci.md#what-it-fails-on),
> [roles](digitalocean-paas.md#cluster-roles-and-the-pooler)).

Also change "before your first `uv run blastit check`" to "before the app's
first connection": the recipes run at deploy time, and `:111` still puts `check`
before the first deploy, so the current sentence conflates the deployed
database with the database `check` uses.

## Round-2 status

| Round-2 finding | Round-3 status | Evidence |
|---|---|---|
| R2-M1 zone/dead-grant dead-end | **survives, narrowed** -> R3-M1 | `:57-60`; keep case only at `digitalocean-paas.md:87`, not on the linked CI section |
| Minor 1 Compose read-back in prose | fixed | `:51`: commented `docker compose exec db psql ...` inside the fence |
| Minor 2 log labels / `tail -1` | fixed | `:95-96`: both variants labelled, both tailed |
| Minor 3 `[settings](../reference/settings.md#presets)` | fixed | `:21` links the settings root |
| Minor 4 item 3 links only DO | fixed | `:74-75` links DO and Compose |
| Minor 5 "`x-forwarded-for` rightmost" in the variable | fixed | `:105-107` names the header the edge **sets** and points at the chain rule |
| D1-D4 (checklist order, "Two databases", "ingress is live", `stage0`/`config`) | unchanged | recorded below; node-declined |

## MAJOR (surviving)

### R3-M1 — `docs/deploy/index.md:57-60` — the dead-grant paragraph names no database, and "cover it" is impossible on the one it names

Quote:

> Both recipes set the zone before your first `uv run blastit check`, so on a
> database that says `UTC` the `connection` group's grant for
> `set_config('TimeZone', …)` is a `dead_grant`; [CI](../ci.md#what-it-fails-on)
> names the fix: cover it, or remove it.

Failure modes:

1. **The chronology is still unresolved.** `:111` puts `uv run blastit check`
   before the first deploy; the recipes are the deploy. So the recipe cannot
   "set the zone before your first check" unless `check` targets the deployed
   database, which the page never says. The old page named the two databases
   ("required on one that says another spelling — a fresh `postgres:18` says
   `Etc/UTC`"); this rewrite dropped that half and kept only the dead case.
2. **"Cover it" is not actionable on a UTC database.** The page's own premise
   is that the database says `UTC`; on that database Django sends no
   `set_config` (`digitalocean-paas.md:85-87`), so there is no code path for the
   suite to cover. The only honest keep is "run `check` against a database that
   says another spelling" (`Etc/UTC`), which is exactly the sentence that was
   removed.
3. **Both readings end red.** Remove because the deployed database is UTC ->
   CI's `Etc/UTC` database reports `missing_grant`; keep because CI's database
   is `Etc/UTC` -> the deployed database reports `dead_grant`. CI names each
   finding, so this is recoverable, but the page still leaves the reader to
   discover the right branch by failing.

Fix: restore the complement clause (above), or link
`digitalocean-paas.md#cluster-roles-and-the-pooler` from the closing and say
"keep it for CI, whose Postgres says `Etc/UTC`". The harness's
`index: the zone is the role's, written by the recipe, not a step the reader
redoes` check passes either way; it does not assert this decision.

## MINOR (round 3)

1. **`:45` — the remedy uses `$OWNER_URL`, which this page never exports.** The
   DO recipe defines it at `digitalocean-paas.md:66`, Compose at
   `compose-traefik.md:146`; a reader who hits the non-UTC branch must leave the
   page to find it. Say "`OWNER_URL` from the recipe" or link the step.
2. **`:54-55` — "Changed a live role? Restart the pooler" gives DO no action.**
   "DO creates the pool after the role" describes creation order, not a live
   change; settings says a pool's open backends keep the old zone until
   `server_lifetime` (one hour) recycles them (`settings.md:161-166`). Say "on
   DO, wait for the pool's backends to recycle, or recreate the pool", or scope
   the step to Compose.
3. **`:64-66` contradicts `:11`.** The preset table says "DO's edge and its own
   services" may reach the port; item 1 says "on App Platform the edge is the
   only peer". Say "no client reaches the port except through the edge" and
   align the table cell, or drop "only".
4. **`:67-70` — "its *minimum*" omits "server connections".** With "16 workers
   x instances" in the same sentence, a reader can think the recipe's `size 18`
   must accept 32 clients; `settings.md:151-155` says `size`/`DEFAULT_POOL_SIZE`
   is server connections and `MAX_CLIENT_CONN` is the client limit. Two words:
   "its minimum *server* connections".

The ingress section has no new finding: both log viewers are labelled and tailed
(`:95-96`), the sample is marked trimmed (`:91-92`), and the remedy names the
header and the chain rule (`:105-107`). The four-item list still costs a skimmer
in item 1 (the "only peer" contradiction) and item 2 (server vs client
minimum); both are above.

---

## Verdict (round 2)

**FAIL** — 0 BLOCKER, 1 MAJOR, 5 MINOR, 4 declined-by-node.

The page passes the skim test everywhere except one paragraph. At 119 lines it
still leads with the preset table, the two-URL table, the four preset limits,
the ingress probe and the checklist as executable blocks. Round 1's M1, M3 and
M4 are fixed: no command on the page is now a DO/Compose hybrid, the ingress
step has both log viewers, and the Connection Details sentence is scoped to DO.
What survives is the zone/dead-grant paragraph (`:59-61`): it tells a reader who
was just told "there is nothing to run" to "set the zone", and its one link does
not answer the keep-or-remove question it raises. That is a red `blastit check`
on the first deploy with no next step on this page.

## Round-1 status

| Round-1 finding | Status | Evidence |
|---|---|---|
| M1 Pooler presets hybrid / wrong URLs | fixed | `:38-50`: one read-back, labelled `# DigitalOcean, 25060`, as `app`; Compose read-back in prose at `:52-53`; step 3 at `:55-57` is Compose-only |
| M2 checklist placement | declined | node fixes section order and keeps the checklist; recorded below as D1 |
| M3 no log command / untrimmed sample | fixed | `:94-97` has both viewers; `:91-92` says the sample is trimmed; residual below |
| M4 "Connection Details" DO-only claim | fixed | `:32-34` now says "On DO both come from Connection Details (the pool URI's path is the pool name); the Compose file already writes both" |
| Minor 1 preset note | fixed | `:19-21`; residual anchor nit below |
| Minor 3 / 4 "one-off grant", duplicate reason | fixed | `:29`, `:31-34` |
| Minor 6 four-line zone paragraph | worse | see R2-M1 |

## MAJOR (surviving)

### R2-M1 — `docs/deploy/index.md:59-61` — "nothing to run" then "set the zone", and the dead-grant decision dead-ends

Quote:

> A database that already says `UTC` makes the `connection` group's grant for
> `set_config('TimeZone', …)` a `dead_grant`; [CI](../ci.md#what-it-fails-on) says
> when the group is required. Set the zone before the first `uv run blastit check`.

Failure modes:

1. **It contradicts the section four lines above.** `:38-40` says "Both recipes
   already put the zone on the role … so … there is nothing to run"; `:61` says
   "Set the zone before the first `uv run blastit check`". A skimmer cannot tell
   whether an action is pending, and a literal reader tries to redo a step the
   page just said was done.
2. **The instruction produces the error it names.** Setting the zone to `UTC` is
   exactly what stops Django sending `set_config` — `python/tests/test_check.py:336-353`
   selects the no-group manifest only when the server reports `UTC`, and states "a
   grant that never runs is an audit error in its own right"; `docs/ci.md:68` makes
   `dead_grant` `severity: "error"`. So the first `check` against the deployed
   database is red, and this page does not say remove the grant (`docs/ci.md:26`
   does) or keep it (`docs/deploy/digitalocean-paas.md:87`: "Keep it for CI, whose
   Postgres says `Etc/UTC`"). Keep or remove depends on which database `check`
   runs against, and the page never names it.
3. **The link promises more than it delivers.** `docs/ci.md#what-it-fails-on`
   defines `missing_grant` and `dead_grant` (`:25-26`) but never says "when the
   `connection` group is required". Only the DO recipe says it (`:87`).
4. **Both recipes punt to here.** `docs/deploy/compose-traefik.md:206-207` states
   the same dead grant with no keep/drop advice, and links `index.md#pooler` as
   "the zone rule". The index is the only page that can settle it, and it doesn't.

Fix (three lines, and it removes the contradiction):

> Both recipes set the zone before your first `uv run blastit check`, so a
> `connection` grant for `set_config('TimeZone', …)` is dead against the deployed
> database. Keep it only if `check` runs against a database that reports another
> spelling — a fresh `postgres:18` says `Etc/UTC`
> ([CI](../ci.md#what-it-fails-on), [roles](digitalocean-paas.md#cluster-roles-and-the-pooler)).

If the node's "ordering rule" must stay verbatim, at minimum change the last
sentence to "The recipes set the zone before your first check" — never an
imperative to set it.

## MINOR (surviving)

1. **`:52-53` — the Compose read-back is still inline prose after a fence labelled
   DO.** A Compose skimmer's eye lands on `# DigitalOcean, 25060` (`:47`), and the
   command they need is a 108-character line inside a sentence. It is scannable
   only after reading step 1's "nothing to run". Fix: start it on its own line
   with a bold **Compose:** lead, or make it a second fence. Do not put
   `docker compose exec` in a checked fence without inventory work: the static
   auditor knows `up/down/logs/build/run/ps/pull`
   (`python/tests/runnable_docs/harness.py:2639-2646`) and rejects unknown
   subcommands (`:2900`).
2. **`:94-97` — the log commands have no `# Compose` / `# DigitalOcean` labels,
   and the DO one has no `tail -1`.** The curl fence directly above (`:84-89`)
   labels both variants; this one does not, so the reader must infer line 1 = Compose,
   line 2 = DO. `doctl apps logs` without a tail prints every `request` event, so
   "the sample" (`:91`) is not one line. Add the two comments and
   `| tail -1` (the static audit already accepts this shape).
3. **`:20-21` — `[settings](../reference/settings.md#presets) lists the rest`**
   overpromises the anchor: `docs/reference/settings.md:175-182` lists only
   `BLASTIT_PRESET`. Drop `#presets` (or say "lists the preset knob") and let the
   page link the settings root.
4. **`:74-75` — "[the recipes]" links only the DO page.** Compose readers own the
   same broad backstop in `db/grants.sql` (`compose-traefik.md:184-196`). Link both
   (`digitalocean-paas.md#cluster-roles-and-the-pooler` and
   `compose-traefik.md#compose`), or say "the DO recipe" and link Compose separately.
5. **`:105-106` — `x-forwarded-for` rightmost is a chain rule, not a header value.**
   The sentence tells the reader to point `BLASTIT_CLIENT_IP_HEADER` at
   "(`do-connecting-ip`, or `x-forwarded-for` rightmost)". `settings.md:128-138`
   defines the variable as a header name and gets "rightmost" from a `*` trusted
   list; pasting the parenthetical into the variable yields an invalid name. Say
   "`do-connecting-ip` on DO; on Compose keep `x-forwarded-for` and check the
   trusted chain ([client address](...))".

## Declined by the node (recorded, non-blocking)

- **D1 — checklist placement (round-1 M2).** `:108-114` still holds two post-start
  checks, is titled "Before the first deploy", and sits after a post-deploy probe
  (`:79`). The page still reads pre-deploy → post-deploy → pre-deploy. The node
  fixes this order, so this is recorded, not graded.
- **D2 — section title "Two databases, two URLs" (`:23`).** The table describes one
  database and one pool: two *destinations*. The table corrects the title
  immediately, and the node declined the rename.
- **D3 — heading "Check that the ingress is live" (`:79`) and item 4's "ingress
  decision" (`:76`).** The probe measures whether `remote_addr` is trustworthy,
  not uptime. `{ #measure-your-ingress }` is load-bearing (three pages link it) and
  the node declined the retitle.
- **D4 — checklist `stage0`/`config` wording (`:111-113`).** Accurate:
  `docs/reference/log-events.md` documents the three `boot` lines with those
  `stage` fields and `preset: null` on `stage0`. Keep.

## What not to change

- The preset table, its "Who may reach the port" column, and the four-item
  "What a preset cannot do for you" framing: the right shape, and item 1 no
  longer repeats the table's values or claims App Platform publishes a port.
- The `#measure-your-ingress` id and the two labelled `curl` variants at `:84-89`.
- The trimmed-sample note at `:91-92` and the one-line JSON example: the field
  elision now matches `docs/reference/log-events.md:22-23`.
