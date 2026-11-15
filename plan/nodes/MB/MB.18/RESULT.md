# MB.18 — Restructure `/deploy`: architecture + topology model first, preset mirrors second, generic levers third

## Summary

`/deploy` is now **the model, then two mirrors of it, then the levers**:

- **`docs/deploy/index.md` (91 lines)** — what you are deploying (one container, `blastit` as
  the front door, a view per route namespace, credentials only in `dbproxy`), a Mermaid
  topology (client → edge → ingress · scheduler → view, `dbproxy` → pooler → database, and the
  migration path that skips the pool), fair queuing as an *expectation* with a pointer, a
  six-row table of the choices with the page that decides each, and one line per next page.
  It contains no procedure and no `SHOW TimeZone`.
- **`digitalocean-paas.md` (208) / `compose-traefik.md` (308)** — the same six sections in the
  same order (`What the preset sets` → `The topology` → `What we thought the config would be`
  → `The steps` → `What this platform does not do for you` → `Where next`), the same diagram
  shape with the platform's own nodes, and no instruction from the other platform: the only
  cross-reference is one line under `Where next`.
- **`anything-else.md` (282)** — the generic lever page: 17 levers in one table (default,
  when to change, what breaks), then the six that need more than a row (privilege, client
  address, the pooler, migrations, egress, the edge), then Docker / systemd / Kubernetes.
- The timezone material is **one short subsection** (`#session-state`, 5 bullets and one
  `psql` read-back) inside the pooler section of the generic page, with the `connection`
  group's `dead_grant` pointing at `ci.md#what-it-fails-on`. It is in neither preset page
  except as the `ALTER ROLE` line each recipe actually runs.

Both critics' BLOCKERs and MAJORs are resolved. The one thing attempt 1 could not own —
`e2e/docs_deploy.py` asserting the old index — is retargeted in attempt 2
(`docs_deploy: 45/45`, exit `0`), with an inverse assertion so the duplication cannot come
back silently.

## Per-page line counts (`wc -l`)

| Page | Lines | Console fences | Budget |
|---|---|---|---|
| `docs/deploy/index.md` | 91 | 0 | ≤ 120 (node) |
| `docs/deploy/digitalocean-paas.md` | 208 | 6 | — |
| `docs/deploy/compose-traefik.md` | 308 | 2 | — |
| `docs/deploy/anything-else.md` | 282 | 4 | — |
| `docs/deploy/fair-queuing.md` | 54 (unchanged) | 0 | — |

Compose is long because it is the complete five-file stack (`dynamic.yml`, `.env`,
`db/grants.sql`, `Dockerfile`, `compose.yaml` = 175 of its lines); its prose is ~45 lines.
Attempt 2 cut every paragraph that explained rather than instructed:

| Cut | Lines | Why it was explanation |
|---|---|---|
| Compose: the "a private-range trusted list is worse than `*`" note | 3 | the same trap is in `settings.md#client-address` and `anything-else.md#client-ip-header`, and only this page carried it |
| Compose: the `ON_ERROR_STOP`/missing-table comment in `db/grants.sql` | 2 | it described a footgun the `ON ALL TABLES` fix removed |
| Compose: the `pgbouncer` and `migrate` YAML comments | 2 | both facts are in the values table one screen up |
| Compose: the capability paragraph after the Dockerfile | 1 | the Dockerfile and the security link carry it |
| Compose: "`user:` is a *dedicated* uid because…" | 0 (tightened) | de-duplicated the values-table row |
| DigitalOcean: "the pool's bindable URL ends in `sslmode=require`…" | 2 | the values-table TLS row states it next to the decision |
| DigitalOcean: "the edge to the service is one platform hop…" | 1 | the diagram and the section title already say it |
| DigitalOcean: "keep `*` honest" | 1 | one sentence instead of three |
| DigitalOcean: the transaction-mode recap in step 4 | 0 (tightened) | — |

## Evidence

| Gate | Command | Result |
|---|---|---|
| docs unit | `make docs-unit` | `83 passed, 65 subtests passed in 0.59s` |
| docs build | `make docs-check` | `No issues found` · `Build finished in 0.29s` |
| docs runnable | `make docs-test` | `46 fences: 28 executed, 18 accounted, 0 failed · 23.3s`, `exit=0` |
| guard | `uv run --no-project tools/guard.py` | `guard: OK (5 staged/untracked file(s) checked)` |
| plan | `uv run --no-project tools/plan.py validate` | `plan OK` |
| deploy promises | `uv run --no-project e2e/docs_deploy.py` | `docs_deploy: 45/45 documented pooler promises hold`, `exit=0` |
| e2e (whole run) | `E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py` | `exit=0`, `docs_deploy: 45/45` at `e2e/run.py:111` and then 162 `^  ok` deployment checks through Docker |

Fences executed per page (`make docs-test`): `digitalocean-paas.md` 1 `presets` + 5 `static`,
`compose-traefik.md` 1 `presets` + 1 `static`, `anything-else.md` 1 `static` + 1 `example`
(curl, no live edge) + 1 `static` (the `SHOW TimeZone` read-back) + 1 `static`,
`index.md` none.

Anchors other pages link to, re-checked in the built site: `deploy/index.html#pooler`,
`#measure-your-ingress`, `deploy/anything-else/#root-or-rootless`, `#memory-and-process-caps`,
`deploy/compose-traefik/#rootless` — all present, `make docs-check` green with
`invalid_link_anchors = true`.

**Visual (node item 6).** Zensical emits the `mermaid` block, so `make docs-check` cannot see
a syntax error. Each diagram was parsed and rendered with mermaid 12 in a browser
(`window.mermaid.parse` + `render`): all three parse, and the intrinsic `viewBox` width is
**352 px**, so at a 375 px viewport (344 px of content) the diagram renders at ~1:1 with no
downscaling and no horizontal scroll. Screenshot: `.playwright-mcp/deploy-diagram-375-v3.png`
(labels ≥ 12 px, vertical flow, `migrate → db` dashed on its own rank). The earlier labels
(`edge · TLS ends here / adds the client address`, `transaction, 18 server connections`) were
trimmed after the `dx` critic flagged them as unreadable at mobile width.

## Critic findings and resolutions

Reviews: `docs/design/deploy-restructure.review-dx.md` (`dx`, 2 BLOCKER · 11 MAJOR · 16 MINOR)
and `docs/design/deploy-restructure.review-ai-ergonomics.md` (`ai-ergonomics`, 2 BLOCKER · 9
MAJOR · 6 MINOR — that critic hit its step limit after the investigation, so `doc-writer`
transcribed its report verbatim; the findings are the critic's).

| # | Lens | Finding | Resolution |
|---|---|---|---|
| B1 | dx | `e2e/docs_deploy.py` still asserted the old index (`SHOW TimeZone`, `export APP_URL=`) and a DO `ALTER ROLE … timezone`, so `make e2e` aborted before Docker | **Resolved (attempt 2).** DO's assertion was fixed in attempt 1 by pinning the zone in `grants.sql`; `index_checks` is retargeted (see Attempt 2 below), and `make e2e` now runs green end to end |
| B2 | dx / A4 | `ALLOWED_HOST` is an env var Django never reads (`ALLOWED_HOSTS` is the setting), so every request is a 400 | Fixed: step 1 of both pages now states `ALLOWED_HOSTS = [os.environ["ALLOWED_HOST"]]`. `docs/frameworks/django.md` is follow-up 2 |
| A1 | ai | `index.md:4` claimed a WASM ingress gate; no `wasmtime`/`wasmer` in the workspace, ingress is native Rust (WASM is Milestone C) | Fixed: removed (the claim was introduced by this draft, not inherited) |
| A2 | ai | No success oracle after the restructure | Fixed on the index (`#measure-your-ingress` + pointer), and the zone read-back now has a runnable `psql … 'SHOW TimeZone'` on the generic page |
| M1 / A3 | dx / ai | `blastit --preset NAME` shown with no output; it is a fail-closed boot, and neither deploy file passes the flag | Fixed: both fences kept (the harness runs them) with the one-line reading: exits `1`, names `cap_*`, points at `blastit dev`; the deploy file sets `BLASTIT_PRESET` |
| M2 / A8 | dx / ai | Both pages pointed at the Django guide's `queries = []` manifest, which denies every statement, and neither named `blastit check` | Fixed: step 1 of both pages — "`queries` filled in … deploy only a manifest `blastit check` passes" + `ci.md#what-it-fails-on` |
| M3 | dx | Fair queuing promised *route* fairness; flows are clients | Fixed: "ordered by client flow … one client cannot starve the rest" (matches `fair-queuing.md`) |
| M4 | dx | The index pooler paragraph was the page's only scroll-back (9 concepts, arithmetic from another page) | Fixed: 6 lines, arithmetic kept to one clause, the "connection budget" back-reference deleted |
| M5 / A9 | dx / ai | "a view never opens a TCP connection" is false with `egress = true` | Fixed: "a view dials no TCP socket unless its route sets `egress = true`" |
| M6 | dx | "What this platform does not do for you" was the same three copied items, one with no instruction | Fixed: items 1–2 now say what *this* platform's trust and numbers are (with one action each); item 3 stays as the shared verification step |
| M7 / A7 | dx / ai | Compose loaded `.env` three contradicting ways, and "never reaches your shell history" was false once sourced | Fixed: Compose reads `./.env`; `TAG` comes from `git rev-parse`; nothing is sourced |
| M8 | dx | `db/grants.sql` granted on `shop_product`, a table the repo does not create, with `ON_ERROR_STOP=1` and `app` waiting on it — the deploy aborts | Fixed: `GRANT … ON ALL TABLES IN SCHEMA public` + `ALTER DEFAULT PRIVILEGES` for later migrations, one commented note on narrowing |
| M9 | dx | Diagram too wide at 375 px | Fixed: labels trimmed to ≤ 24 chars; measured 352 px intrinsic → ~1:1 (see Visual) |
| M10 | dx | "Where next" sent a reader who just deployed to the other platform | Fixed: verify first (`measure the client address`), cross-link second |
| M11 | dx | The DO `ca_cert` reason was inverted, so a reader could delete the verification they were told to add | Fixed: verified `crates/blastit-dbproxy/src/upstream.rs:79-86` (`Require` + CA ⇒ `VerifyMode::Full`) and rewritten on both pages |
| A4 | ai | DigitalOcean mixed `<id>`/`<password>`/`<host>`/`REPLACE_WITH_*`/`yourregistry` | Fixed: one `REPLACE_WITH_*` dialect, with one sentence mapping each token to its source |
| A5 | ai | "publish no other route to it" was Compose's instruction copy-pasted | Fixed: "a service is reachable only through DO's edge — which is why `*` is safe here" |
| A6 | ai | Compose's `compose.yaml` referenced `python3.13-init` and `db/grants.sql` before those files were defined | Fixed: steps reordered — Traefik, values, grants, image, compose, start |
| A10 | ai | "your framework is not a process of its own: a view process … is forked from it" contradicted itself | Fixed: "your framework runs inside it: one view process per route namespace, forked from `blastit`" |
| A11 | ai | The DO `app` user was created twice (console and `doctl`) | Fixed: "or the first command below" |
| MINORs | both | default-valued preset rows, duplicated rows across the two tables, `pg_isready` labelled as the app's health, unlabelled trimmed boot line, `ALLOWED_HOST` stated three times, "seven capabilities" repeated, three placeholder dialects, "every step below takes `$CLUSTER_ID`" | All fixed; m1 ("What we thought the config would be") and m15 (console + `doctl` both shown) kept deliberately — m1 is the node's own section name, m15 is the platform's two real paths |

## Mismatches found in the shipped pages (fixed here)

1. **`cap_add` was missing `SETPCAP`** on the Compose app service while the page's own expected
   boot line says `"bounding_set":"dropped"`, and `e2e/rootless/compose.yml:103` ships seven
   capabilities. Added `SETPCAP` to the documented list (and `anything-else.md` already said
   "those six plus `SETPCAP`").
2. **The DO `ca_cert` explanation was backwards** (`require` + CA is a *verified* leg, not an
   unused one) — `crates/blastit-dbproxy/src/upstream.rs:79-86`.
3. **"the CA is read only for `verify-full`"** appeared twice on the DO page; both replaced.
4. **Compose `grants.sql` failed on a fresh database** (`shop_product` does not exist, the grant
   job runs `ON_ERROR_STOP=1`, and `app` waits on it).
5. **The DO `ALTER ROLE app SET timezone` line must run before `SET ROLE migrator`** (a
   `NOCREATEROLE` role may not alter another role); placed as the owner.
6. **`ALLOWED_HOST`** (both pages) and **`queries = []`** (both pages): a reader following the
   pages produced a deployment that 400s and 42501s. Both now named, with the fix.
7. `docs/deploy/index.md` said "validates input in a **WASM** gate" — introduced by this
   restructure, false for 0.2.0 (no WASM dependency; VISION puts it in Milestone C). Removed.

## Attempt 2 — the `e2e/docs_deploy.py` retarget

`e2e/docs_deploy.py` is now in this node's paths. `index_checks` no longer reads the zone
procedure; the promises moved to where the pages moved:

| Promise | Old home | New home |
|---|---|---|
| `SHOW TimeZone` with the exact value it must print | `index_checks` | `anything_else_checks` |
| the read-back runs as `app`, on the direct port, never the pool | `index_checks` (an exported `APP_URL` + export-before-use order) | `anything_else_checks`, asserted on the command line itself: starts `psql 'postgresql://app:`, contains `:5432/`, ends `-A -t -c 'SHOW TimeZone'`, never names `pgbouncer` |
| `ALTER ROLE app SET timezone = 'UTC'` is the fix | `index_checks` | `anything_else_checks` (and still asserted on both recipe pages) |
| the URL route is refused (`options=-c timezone=UTC`) | — | `anything_else_checks` |
| a live role change restarts the pooler | — | `anything_else_checks` |
| a `UTC` database turns the `connection` grant into a `dead_grant` | — | `anything_else_checks`, with the `ci.md#what-it-fails-on` link |
| the app names the pool, migrations and grants name the database | `index_checks` | `index_checks` (kept) |
| the index must hand off, not re-teach | — | `index_checks`: links `anything-else.md#pooler`, and carries **none** of `SHOW TimeZone`, `set_config`, `dead_grant`, `ALTER ROLE`, `options=-c` |

Two promises were dropped rather than moved, and said so on the line that replaces them:
`export APP_URL=` (no page exports `APP_URL` any more; the URL is inline, so the
export-before-use ordering has no subject) and `docker compose exec db psql …` (the generic
page shows one command instead of two platform variants). The direct-port promise is
strictly stronger now: it reads the command rather than a URL-building step.

**Teeth, proved by mutation.** Each of these was applied to the working tree, run, and
reverted:

| Mutation | Result |
|---|---|
| append "See `SHOW TimeZone` on the direct port." to `index.md` | `FAIL  index: the session-zone procedure is not taught here (SHOW TimeZone)` · `44/45` · script exit `1` |
| change the read-back URL to `@pgbouncer:5432/` | `FAIL  anything-else: the read-back runs as app on the direct port, never the pool:` followed by the command it read · `44/45` |
| replace `dead_grant` with `grant` in `anything-else.md` | `FAIL  anything-else: the UTC database's group grant is a dead_grant, with the CI link` · `44/45` |

Restored: `docs_deploy: 45/45`, script exit `0`.

**Ordering claim.** `e2e/run.py:111` still runs `docs_deploy.main()` first and returns `1`
before any container start; it was not edited (not in this node's paths), and its comment
there still says "the two deploy pages" although the checks now also cover the index and the
generic page. The full run confirms it falls through: `E2E_DOCKERFILE=Dockerfile.prebuilt
uv run --no-project e2e/run.py` → `exit=0`, 162 `^  ok` deployment checks against real
containers, and the `docs_deploy: 45/45` block (the log's last block only because the parent's
stdout is buffered behind the child's direct writes). Docker and a built wheel were available
here; nothing is claimed unproven.

## Files

- `docs/deploy/index.md` — rewritten as the model (91 lines, no console fences).
- `docs/deploy/digitalocean-paas.md` — rewritten as the App Platform instance of it.
- `docs/deploy/compose-traefik.md` — rewritten as the Compose instance of it.
- `docs/deploy/anything-else.md` — rewritten as the lever page; the session-zone material now
  lives here once.
- `python/tests/runnable_docs/inventory.toml` — deploy entries re-cut: index 0 fences,
  DigitalOcean 6, Compose 2, anything-else 4 (the new `psql` read-back), each with a reason.
- `e2e/docs_deploy.py` — `index_checks` retargeted: the two-URL promise stays, the zone
  procedure moved to the new `anything_else_checks`, and the index is asserted to carry none
  of it. Module docstring names the generic page as the owner.
- `docs/design/deploy-restructure.review-dx.md`, `docs/design/deploy-restructure.review-ai-ergonomics.md`
  — the two critic artefacts (excluded from the site by `zensical.toml`).

`docs/deploy/fair-queuing.md`, `docs/VISION.md`, `docs/security.md`, `docs/reference/*` and all
code are untouched.

## Follow-ups

1. **Done (attempt 2): `e2e/docs_deploy.py` retargeted.** `index_checks` keeps the two-URL
   promise and the hand-off link, and now asserts the index carries *none* of
   `SHOW TimeZone` / `set_config` / `dead_grant` / `ALTER ROLE` / `options=-c`; the new
   `anything_else_checks` owns the seven session-zone promises at
   `anything-else.md#session-state` (anchor, exact value, the app-role/direct-port command,
   the role default, `options=` refused, restart the pooler, the `dead_grant` + CI link).
   `docs_deploy: 45/45`, exit `0`; `e2e/run.py:111` now falls through to Docker.
2. **`docs/frameworks/django.md`** teaches the opposite of a deployable app: its manifest has
   `queries = []` on the catalog route, and slot 4 says "name the host in `ALLOWED_HOSTS`" with
   no `os.environ` wiring, while every deploy page passes `ALLOWED_HOST`. Worth one line in
   slot 4 and a note that `queries` is filled by `check --learn`.
3. **`docs/security.md#the-mechanisms`** still describes `egress = true` as "any address and
   port … until the HTTP proxy milestone"; unchanged here (MB.3.8 owns the sync), but the
   bridge sentence "the only syscall and egress wall" is now the load-bearing claim on two
   deploy pages.
4. `docs/reference/settings.md` links `../deploy/index.md#pooler` and `#measure-your-ingress`,
   and `docs/troubleshooting.md` links `#measure-your-ingress`: both anchors kept, but the
   *content* they land on is now a short pointer. If a future restructure moves them, four
   other pages need the same edit.
