# MB.2.11 — `ai-ergonomics` review (verdict pass, fresh context, attempt 5)

- **Lens:** `ai-ergonomics`, fresh context. **Date:** 2026-11-13.
- **Pages read:** `docs/deploy/index.md` (121), `docs/deploy/digitalocean-paas.md` (150),
  `docs/deploy/compose-traefik.md` (259), `docs/reference/settings.md` §"Behind a
  connection pooler" (150-173).
- **Agreement check:** `docs/research/connection-pooling.md` (105).
- **Question:** can an agent that has never seen Blastit apply both recipes in one pass —
  no invented port, database, password or preset variable — and does the gate notice when
  a later edit re-breaks the one trap the node is about (a zone read through the pool)?
- **Commands run myself, on a quiet tree:**
  `uv run --no-project e2e/docs_deploy.py` → **40/40**, exit 0;
  `make docs-test` → **47 fences: 28 executed, 19 accounted, 0 failed**, exit 0;
  `make docs-check` → `No issues found` (`Build finished in 0.19s`), exit 0.
  (Two earlier `make docs-test` runs of mine were red at the two `install` fences while
  another gate held the lock and rebuilt `dist/` mid-run — see f9. An isolated
  `run.py docs/getting-started.md` was 3/3 green, and a direct
  `uv pip install` of the built wheel matched it byte for byte.)
- **Anchors read at their targets:** `django.md:39` (`## Manifest`),
  `manifest.md:176` (`### Protocol postgresql`), `ci.md:49` (`## For agents: --json`),
  `settings.md:150` (`## Behind a connection pooler`), `security.md:60`
  (`## The mechanisms`), `settings.md:95` (`## The client address`).

## The three claimed fixes, verified

| # | Claimed fix | Evidence | Holds? |
|---|---|---|---|
| m1 | `docs/deploy/index.md`'s fence exports its own URLs, with direct `:25060/defaultdb` placeholders, so a copy cannot fall back to `psql` defaults | `index.md:43-44` export `OWNER_URL` and `APP_URL` before `:45-46` consumes them; both are literal `postgresql://…@<host>:25060/defaultdb?sslmode=require`; `:37-40` still names the trap ("never the pool's URI, whose path is the pool name") | **yes** — and I re-ran the old failure shape: only *deleting* an export line turns the gate red (M4 below), so the fix is the page's, not the prose's |
| m2 | `e2e/docs_deploy.py` pins export-before-use order and rejects an owner-role oracle | `e2e/docs_deploy.py:229-232` compares `flat.index("export APP_URL=")` with `flat.index('psql "$APP_URL"')` and the same for `OWNER_URL`; `:236` requires `'psql "$APP_URL" -A -t -c'` and refuses `'psql "$OWNER_URL" -A -t'` | **yes for order and oracle** — the read is named as the app's; the *contents* of `APP_URL` are still unpinned (f1) |
| m3 | `inventory.toml` no longer says `POOLER_URL` / "the pool's to answer" | `python/tests/runnable_docs/inventory.toml:138`: "`OWNER_URL`/`APP_URL` name a managed cluster and a Compose stack this gate does not run … the zone is read on the direct port as `app`"; `grep -rn POOLER_URL docs/ python/ e2e/` returns no page or gate hit | **yes** — the printed `accounted:` line now describes the direct read |

40/40 includes both index promises and the DO/Compose two-URL arithmetic, so the
regression this node removed is pinned at the level it was removed.

## Findings (all `MINOR`; no `BLOCKER`, no `MAJOR`)

### f1 — `MINOR` — the gate still cannot tell a direct URL from the pool's

`e2e/docs_deploy.py:229-236` never looks at what `APP_URL` holds. I mutated copies of
`index.md` and re-ran `index_checks` against them (`/tmp/opencode/index.md`, imports the
real module, only `dd.INDEX` repointed):

```text
--- baseline:                                  5/5 pass; failures=[]
--- M5 APP_URL -> 25061/shop-pool:             5/5 pass; failures=[]
--- M1 both APP_URLs -> 25061/shop-pool:       5/5 pass; failures=[]
```

So reverting `APP_URL` to the **pool's** URI — the exact inversion `index.md:35-40` warns
about, and the behaviour that makes a pooled `SHOW TimeZone` answer `Etc/UTC` — is still
5/5 green. This is the gap attempt 4 named as f2, now narrowed to loyalty-of-contents:
the read is pinned to `$APP_URL`, not to the direct port. The page is right today
(`index.md:43-44`); only the audit trail is blind. One line closes it:

```python
checks.that(
    flat.count(":25060/defaultdb") == 2 and "postgresql://app:" in flat,
    "index: both URLs are literal direct `:25060/defaultdb` URIs, `APP_URL` as `app`",
)
```

### f2 — `MINOR` — the order check counts a commented export, and crashes instead of failing

`e2e/docs_deploy.py:229-232` is a substring-and-offset test over flattened prose, not over
the fence. Mutations, same harness:

```text
--- M2 `$ # export APP_URL=` (commented out):  5/5 pass; failures=[]
--- M3 zone read as `$OWNER_URL`:              ValueError: substring not found (at :230)
```

M2 is the failure mode f1 of attempt 4 was written for: the line is still "there" as a
comment, the gate is green, and a human or agent following the fence gets `APP_URL=""` and
psql's local-socket error. M3 is cosmetic but agent-hostile: renaming the read aborts the
whole run with a Python traceback at `flat.index(...)` before the named promise can print
`FAIL`, so the only diagnostic is a line number. Read the block body (the module already
has `block()` for titled fences; this one is the unnamed `console` fence at
`index.md:42-47`) and assert line shapes, or guard the offsets:

```python
if 'psql "$APP_URL"' in flat:  # else the named FAIL below is the report
```

### f3 — `MINOR` — the DO-only fence is unlabeled inside a preset-shared section

`index.md:42` is a DigitalOcean block: `doadmin`, `<host>:25060`, `sslmode=require`. The
section (`## Pooler`, `:24`) and the lead-in (`:37-38`, "Fill both URLs from the cluster's
Connection Details") cover **both** presets, and only `:49-51` switches to Compose. An agent
deploying Compose therefore meets a `psql "$OWNER_URL"` whose variable exists only inside
the `grant` service (`compose-traefik.md:143-151`), and the fix is prose at `:50`.
Two smaller costs on the same fence: `<password>` has no stated source for `app` —
`digitalocean-paas.md:56-58` says "its password is the pool's", the index does not — and
`:37` "Connection Details" is where `doadmin`'s URI comes from, not `app`'s. A comment line
above the exports makes the fence self-describing in one edit:

```console
# DigitalOcean: owner URI from Connection Details, `app` URI from Users & Databases
```

### f4 — `MINOR` — the DO heredoc still exits 0 on a failed grant

`digitalocean-paas.md:65`:

```console
$ psql "$OWNER_URL" -v ddl="$MIGRATOR_PASSWORD" <<'SQL'
```

No `-v ON_ERROR_STOP=1`, so a failed `ALTER SCHEMA public OWNER TO migrator` (`:70`) or a
denied `GRANT` (`:72-77`) scrolls past and psql exits 0. Its Compose twin has the flag
(`compose-traefik.md:148`) and `:186-189` says why ("`grant` runs with `ON_ERROR_STOP=1`
and `app` waits for it"). The zone read-back at `index.md:45-46` catches a failed *zone*;
a failed *grant* surfaces much later as a permission error on a query, which an agent will
misattribute to `blastit.toml`. Carried unchanged from attempts 2-4.

### f5 — `MINOR` — nothing checks that the placeholders were substituted

`digitalocean-paas.md:111-139` ships five `REPLACE_WITH_*` values, three of them secrets
(`:116`, `:118`, `:132`). `:137-140` lists them in prose and says to keep `app.yaml` out of
git, but `doctl apps create` (`:144`) accepts the literals: a missed
`REPLACE_WITH_50_CHAR_SECRET` deploys that string as Django's signing key, with a healthy
`/_blastit/health` and no error anywhere. For a one-pass agent the missing step is
mechanical, and it is the highest-value remaining edit:

```console
# must print nothing; each hit is a value the deploy must not ship
$ grep -n REPLACE_WITH app.yaml
```

### f6 — `MINOR` — the verification loop the page asks for has no command on DO

`index.md:95-100` says "Both presets log JSON; find the `request` event it produced and
read `remote_addr`", and `:105-108` tells the reader what to do with it. Compose gets the
command (`compose-traefik.md:252` `docker compose logs app | head -1`); the DO page names
only `doctl apps create`/`apps update` (`:144`, `:141`) — nowhere `doctl apps logs <app-id>
--type run`. An agent must guess the subcommand to close the loop this section exists for.

### f7 — `MINOR` — carried wording: one hour is PgBouncer's default; the research still reads absolute

`index.md:36-37` gives "(`server_lifetime`, one hour, recycles them)" as the reason a
pooled read can be stale — true of the pinned PgBouncer (`compose-traefik.md:107`), not a
promise of DO's managed pooler, whose lifetime and restart story are undocumented
(`connection-pooling.md:92`) and whose recipe needs neither (the `ALTER` at
`digitalocean-paas.md:67` precedes pool creation at `:85-92`). `settings.md:163`'s
"restart the pooler" has the same scope. Separately, `connection-pooling.md:43` now
carries the useful fact ("a backend keeps the session GUCs it logged in with until it is
recycled; `server_lifetime` defaults to 3600 s") — good — but `:13`, `:22` and `:82` still
say PostgreSQL "reports it at startup" with no new-server-session qualifier; only `:66`
carries it. The citable source should not read more absolute than the page
(`index.md:35-37`, `settings.md:159-164`).

### f8 — `MINOR` — "~7 in 1 GB" is the cap's divisor, not a memory budget

`digitalocean-paas.md:134` and `compose-traefik.md:91` carry
`BLASTIT_VIEW_MEM_ESTIMATE: 140MiB  # one view; ~7 in 1 GB`, next to `mem_limit: 1g`
(`compose-traefik.md:158`). 1024/140 is the process-cap divisor (`settings.md:111`); the
admission and retirement check is the 85 % watermark (`settings.md:112`), i.e. ~870 MiB of
non-reclaimable memory on a 1 GB instance. 7 × 140 = 980 MiB is above that, so "~7" is an
upper bound on process count, not a route budget an agent may plan against. "~7 by the
cap's divisor (the watermark decides admission)" costs four words and removes the
over-subscription trap for an agent adding routes.

### f9 — `MINOR` — `make docs-test` misdiagnoses a concurrent gate as a stale wheel

Evidence from this host, with the gate's own log as the control: my two full runs were
**2 failed**, both `install` fences, both with

> `blastit-0.1.2.dist-info/sboms/blastit-py.cyclonedx.json in the installed environment
> differs from …whl (sha256 …): a wheel cached by an earlier run was installed, not the one
> this run built`

while `/tmp/opencode/dt4.log` (the gate's run) recorded `47 fences … 0 failed`. Root
cause: every maturin build writes a new CycloneDX `serialNumber` UUID and
`metadata.timestamp` (`…/sboms/blastit-py.cyclonedx.json`), so `dist/` bytes change on each
rebuild; `python/tests/runnable_docs/harness.py:721-762` compares installed files with the
wheel *at verification time*, and another gate rewriting `dist/` between the install and
the comparison produces this message. A direct `uv pip install` of the same wheel into a
fresh venv matched it (`65432418aa70` both sides), which rules the packaging out. The
message is the only part that matters for an agent: it blames a stale wheel where the truth
is "another gate rebuilt `dist/`". Say so, and tell the reader to re-run on a quiet tree
(or snapshot the wheel into the run directory before installing).

## Research ↔ pages

| Topic | Research | Pages | Agree? |
|---|---|---|---|
| app → pool, one-offs → direct | 14, 19, 80 | `digitalocean-paas.md:116,129`; `compose-traefik.md:86,131,142`; `index.md:27-30` | yes |
| zone is a role default; `options` refused | 13, 22, 82 | `index.md:32-34`; `digitalocean-paas.md:67,82`; `compose-traefik.md:194-195`; `settings.md:159-161` | yes (f7) |
| stale pooled sessions, `server_lifetime` | 43 | `index.md:35-37`; `settings.md:161-163` | yes (f7) |
| dbname is the pool name | 23, 81 | `digitalocean-paas.md:86,94`; `index.md:39-40` | yes |
| sizing: workers × instances vs pool `size` | 24, 35, 46 | `index.md:66-71`; `settings.md:152-157`; `compose-traefik.md:117-118` | yes |
| DO bindable form (three-part) | 19 | `digitalocean-paas.md:86,104-105,129` | yes |
| transaction-mode limits / prepared statements | 9, 10, 38, 85 | `digitalocean-paas.md:87-96`; `settings.md:166-169` | yes |
| image pin, SCRAM, healthcheck | 23, 46, 48, 83 | `compose-traefik.md:107-123` | yes |
| TLS ceiling (pooler cert) | 36, 61, 87 | `compose-traefik.md:208-210`; `settings.md:171-173` | yes |
| fail closed without privileges | 88 | `index.md:3-5`; `settings.md:50-56` | yes |

No research recommendation contradicts a page, and `grep -rn POOLER_URL docs/ python/ e2e/`
is now empty of page and gate hits.

## Post-read corrections (doc-writer, same day, after this read)

- **f1 is closed**: `e2e/docs_deploy.py` now regex-pins
  `export APP_URL='postgresql://app:…:25060/defaultdb` (and keeps the
  export-before-use and owner-oracle checks), so re-pointing `APP_URL` at
  `:25061/shop-pool` goes red; the suite is **41/41**.
- The research note's startup claim is qualified to "the startup of each new
  server session" and `server_lifetime` 3600 s is cited
  (`connection-pooling.md:13,22,44,82`), so the carried note item is resolved.
- **f4–f9 remain**: the DO heredoc lacks `ON_ERROR_STOP=1`, unsubstituted
  `REPLACE_WITH_*` values deploy without a mechanical check, the gate's
  export-order test is a flattened-substring test, the one-hour figure is
  PgBouncer's default rather than the platform's promise, and a concurrent
  `dist/` rebuild can redden the two `install` fences. None blocks a deploy.

## Verdict

All three claimed fixes hold where it matters: the fence exports what it reads with literal
direct URIs (m1), the gate pins the order and refuses the owner oracle (m2), and the audit
reason describes the direct read (m3). I re-ran the old failure shapes: deleting an export
line is caught, reading as the owner is caught; what remains unguarded is a *re-pointed*
`APP_URL` (f1) and a *commented* export (f2), both gate-side, neither wrong on the page.
The remaining page costs are copy-surface and wording — an unlabeled DO-only fence inside a
shared section (f3), a heredoc that swallows failed grants (f4), five placeholders with no
mechanical check (f5), and one missing log command (f6) — plus three carried research/sizing
notes (f7, f8) and one misleading gate diagnosis under concurrency (f9). None blocks a
deploy, none is silent on the happy path, and no item makes an agent reach for a wider
grant: every failure that survives prints a real psql/permission error.
**0 `BLOCKER`, 0 `MAJOR`, 9 `MINOR`.**

VERDICT: PASS
