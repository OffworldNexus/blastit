# Result — MB.56.4.2: `e2e/run_prod.py`

**Verdict: done.** `uv run --no-project e2e/run_prod.py` proves `TESTING_PLAN.md`
§6 in real containers on this host (7 rows, 0 failed, 0 skipped), `make fast-check`
is green and `make docs-check` builds the site.

## What landed

| Path | What |
|---|---|
| `e2e/run_prod.py` | The production suite: one root leg (`--preset digitalocean-paas`) and one rootless leg (`BLASTIT_PRESET=compose-traefik`), both against the shipped binary and `postgres:18`. One `PASS`/`FAIL`/`SKIP` line per leg plus a summary; `--binary`, `--keep`. Ports from 28080, scratch under `${TMPDIR:-/tmp/opencode}/blastit-e2e-prod`, every container/volume/network/image named `blastit-prod-*` and removed in a `finally`. |
| `e2e/apps/prod/public/index.php` | `/whoami` (view pid/uid, `REMOTE_ADDR`, `HTTPS`), `/fast` + `/slow?ms=` (fair queuing, drain), `/catalog/*` and `/warehouse/*` (two views, two policies), `/env`, `/shell` (no process function callable), `/egress` (an undeclared TCP peer, through both the stream layer and `socket_create`). |
| `e2e/apps/prod/Blastfile` | The **root** leg's manifest: the **block** store the DigitalOcean page writes (`host/port/user/password/database/sslmode` from `$DB_*`), five views, one grant per view. |
| `e2e/apps/prod/Blastfile.url` | The **rootless** leg's manifest: the **URL** store the Compose page writes (`postgres $DATABASE_URL` naming `pgbouncer`). |
| `e2e/apps/prod/Dockerfile.rootless` | `ARG BASE` + the binary at `/usr/share/blastit/lib/blastit`, the shim beside it, and a `0500` `blastit-init` owned by 10000 with the seven file capabilities — the `compose-traefik` §4 recipe. |
| `e2e/apps/php/Dockerfile` | Bakes `e2e/apps/prod` into `/prod` and installs the `libphp8.4` shim into `/usr/share/blastit/lib`, because production shim discovery is **exe-relative** (`BLASTIT_PHP_SHIM_DIR` is a `Development`-scope search root only). |
| `e2e/README.md` | The production section (stores, targets, prerequisites, scratch dir, `SYS_PTRACE`, the watermark percentage, the capability-build FAIL), the `apps/prod/` fixture row, and `make e2e-prod`. |
| `docs/security.md` | "The full harness" now names `run_apps.py` and `run_prod.py` (the deleted `e2e/run.py`/`e2e/rootless/run.py` are gone) and what the latter observes: `/proc/<pid>/status` uid and `Seccomp:`, capability sets, `NoNewPrivs:`, the environ scan and the drain. |
| `docs/deploy/compose-traefik.md` | The runtime directory is `0711` (the `0700` line locked every view out of its own `0700` directory; the harness caught it) with the why beside it. |
| `Makefile` | `make e2e-prod` builds `-p blastit --release` and runs `e2e/run_prod.py`; the help header names it. |

## Which leg proves which page's store

| Preset | Page it deploys by | Store in the manifest | What the leg asserts |
|---|---|---|---|
| `digitalocean-paas` | `docs/deploy/digitalocean-paas.md` §1 | `postgres { host $DB_HOST … sslmode $DB_SSLMODE }` | `database_url` is **absent**, `db_password_in_env` false, `DB_HOST`/`DB_PORT`/`DB_USERNAME`/`DB_DATABASE` present (the rendered socket), and the reads work through it. |
| `compose-traefik` | `docs/deploy/compose-traefik.md` §5 | `postgres $DATABASE_URL` = `postgresql://apps:<pw>@pgbouncer:5432/apps?sslmode=disable` | the worker's `database_url` is a libpq **socket** URL (`host=`) with no password, no `DB_*` slot rendered, PgBouncer on the path (`pg.refused` while it is stopped). |

## What the two legs prove beyond the unit gate

**`root preset (digitalocean-paas)`** — `privilege: root` and `seccomp: true`; the
preset's four settings on every boot event after stage 0; five routes sharing no
uid, all ≥ 20000, with `/whoami`'s own `posix_geteuid()` equal to the
`process.spawned` uid for `front`, and the kernel agreeing (`Uid:` in
`/proc/<pid>/status`) while the view's `Seccomp:` is `2`; **every** child with all
five capability sets zero and `NoNewPrivs: 1`; the catalogue read (plain and
`WHERE id = ?`) and the orders read answered, while `catalog` is refused `orders`
and `warehouse` refused `products` (SQLSTATE `42501`); a trusted
`X-Forwarded-Proto: https` decides the view's `HTTPS`, and `do-connecting-ip`
decides `REMOTE_ADDR`; no process — stage 1 included — carries the database
password in `/proc/<pid>/environ`, and the scan first proves it can read a view
(it finds that view's own `DB_HOST`); `/shell` reports no process function while
`docker exec` runs `id`; `/egress` reports no connection (both spellings) with a
kernel sentence, while the same `fsockopen` from `docker exec` reads
`canary-ok`; fair queuing serves a second client's `/fast` ahead of a slow
client's backlog that demonstrably queued (`queued_ms` ≥ 2 s over 6 × 0.7 s);
`SIGTERM` finishes a request whose view is in `/proc/<pid>/stat` state `S`, the
container exits `0`, and the log reports this `drain` then `stopped`.

**`rootless preset (compose-traefik)`** — Traefik v3 `:80`/`:443`, PgBouncer,
`--memory 1g`, `BLASTIT_VIEW_MEM_ESTIMATE=140MiB`; `privilege: capabilities` with
exactly the seven; `/usr/share/blastit/lib/blastit-init` is `0500`, owned by
10000, carries all seven, and neither the shared binary nor the shim the runtime
`dlopen`s nor `python3` carries one; uid `10001` is refused that copy while
`10000` runs it; zero capabilities and `NoNewPrivs: 1` in every child (a held
request keeps a view in flight); a direct request naming
`X-Forwarded-For: 198.51.100.9` / `X-Forwarded-Proto: https` is believed, while
the same headers forged through Traefik do not decide `REMOTE_ADDR`/`HTTPS`; the
URL store renders a socket and no password; the same denials and sandbox as the
root leg; PgBouncer on the path; fair queuing through the in-container client;
a renamed decoy entry and an in-place rewrite both answer an empty `500`, the
decoy never runs, the sentence says `changed since this boot recorded it`, a
restore does not revive it, a `docker restart` serves the recorded tree again;
and `SIGTERM` drains `blastit-init` with the in-flight request answered.

## Old → new map

| Old (`e2e/rootless/`, `e2e/scenario.py`, deleted in `MB.56.4.1`) | New |
|---|---|
| `rootless/run.py --local` | `leg_rootless` + `leg_root` in `run_prod.py` |
| `compose.yml`, `traefik.yml`, `Dockerfile`, `Dockerfile.prebuilt` | Direct `docker run` on a private `blastit-prod-net-*` network; Traefik's dynamic file is written into the run's work dir; the rootless image is `apps/prod/Dockerfile.rootless` from a binary-only staging context (no in-image Rust build) |
| `check_rootless_logs` | `preset_rows`, the `capabilities` list, the `process.spawned` uid assertions, `child_containment` |
| `check_preset_logs` | `preset_rows` (every boot event after stage 0, so a PHP boot with no `preload` line still proves it) |
| `check_proxy_headers` | `forwarded_headers` (forged chain rejected) + the direct positive row |
| `caps_and_limits` | `capabilities_probe` (the file capabilities are the claim) |
| `check_container` | `child_containment`, `capabilities_probe`, the two `--version` execs |
| `check_php_entry_swap_leaves_sentinel_untouched` | `tampered_entry` (rename-over decoy, in-place rewrite, restore, restart) |
| `scenario.py` demo routes | `e2e/apps/prod/public/index.php` (only the §6 subjects) |
| `check_logs.py` | No password in any response or `/proc/<pid>/environ`; `pg.refused`/`sql.denied` presence |

## Dropped cases and why

| Case | Why |
|---|---|
| `--droplet`, `--janitor`, `--cleanup`, SSH keys, `git archive HEAD` shipping | Needs `DIGITALOCEAN_ACCESS_TOKEN` and a paid droplet; the *preset's* promises are asserted locally by the root leg. |
| `check_negative_boot` (a `--cap-drop ALL` rootless boot refused) | Already pinned in `crates/blastit/tests/e2e_prod_refusals.rs`; a container replay duplicates a Rust suite. |
| `check_rooted_startup` (missing-manifest ordering as root) | Same owner: `e2e_prod_refusals` pins both messages and both directions. |
| `php_entry_swap`'s "route uid cannot `chmod`/unlink the app tree" | Not in §6's list; the entry pin (four states) and the `0500`/ownership checks are the deployment promise. |
| `SELF_PROBE`'s python `os.setuid(0)` attempt and the `manage.py shell` probe | The direct promise — uid 10001 cannot run `blastit-init`, 10000 can — is asserted instead. |
| `docker inspect` `User`/`CapDrop`/`CapAdd` echo | The boot's `capabilities` list and `/proc` are the facts; the echo restated the flags the driver passed. |
| `scenario.py`'s catalogue/order/note/error/static/learn rows | §4 (`run_apps.py`, `e2e_dev`) and §5 (`e2e_check_fmt`) stories. |
| `docker compose` orchestration | `docker run` on the same topology: no compose-plugin version dependency, one style shared with `run_apps.py`; `docs_deploy.py` still checks the compose text. |
| In-image wheel build (Rust toolchain in the image) | The node mounts the artifact under test; the rootless image is the fixture image plus a capable copy of that binary, so a stale wheel cannot hide. |

## Deliberately **not** mirrored from the DigitalOcean recipe

* **The pool at `25061` and `verify-full` + the cluster CA.** A local stack has no
  managed cluster CA to pin, and `sslmode=require` against a self-signed local
  certificate would prove a different thing than the page promises. The store
  *shape* (block, `$DB_*`, no URL) is what the root leg proves; the pooled path
  and `pg.refused` are proved in the rootless leg through PgBouncer.
* **`PRE_DEPLOY`/`doctl apps create` and the `migrate`/`grant` jobs.** Those need
  the platform's API and managed roles; `docs_deploy.py` keeps the pages' pooler
  text honest, and the local `postgres:18` applies the same schema out of band.
* **The Compose page's published ports and ACME resolver.** The app publishes
  nothing (as the page says) and only Traefik is published, on loopback, with its
  self-signed certificate; the leg asserts the scheme Traefik reports, not a
  trusted chain.

## Two harness concessions (recorded, not hidden)

* The root container runs with `--cap-add SYS_PTRACE`. Container root has no
  `CAP_SYS_PTRACE`, and a `PR_SET_DUMPABLE(0)` view's `/proc/<pid>/environ` is
  `EACCES` without it, so without the capability the credential scan would read
  nothing and pass vacuously. It is no longer aspirational: the leg finds the
  view's own `DB_HOST` first (proving the read), and every child still drops
  every capability (`child_containment`). The rootless leg deliberately does not
  add it — its boot must report exactly the seven documented capabilities.
* The rootless runtime directory is `mode=0711`, not the page's original `0700`:
  the view directories are `0700` per uid, but the *parent* must stay traversable
  or a view cannot reach its own directory (measured: the first `0700` run failed
  with `view_tmp … must be an existing absolute directory`). `0711` keeps the
  directory unlistable. The page was corrected rather than the harness widened to
  `0755`.

## Evidence

```console
$ uv run --no-project e2e/run_prod.py
PASS  binary                    …/target/release/blastit (16903664 bytes)
PASS  fixture image             blastit-prod-php:<pid>
PASS  postgres                  postgres:18 on 127.0.0.1:28080 (schema applied)
PASS  canary                    an HTTP peer on 172.22.0.3:8081
PASS  root preset               root preset (digitalocean-paas)
PASS  rootless image            blastit-prod-rootless:<pid>
PASS  rootless preset           rootless preset (compose-traefik)

7 passed, 0 failed, 0 skipped (a SKIP names the host fact it needs)
```

```console
$ make fast-check      # green (guard, rs-fmt, rs-lint, rs-doc, rs-test, py-lint,
                       #  plan-validate, universal-smoke, install-test)
$ make docs-check      # zensical build --strict, no issues
```

## Follow-ups

* `make check` does not include `e2e` or `e2e-prod` (by design: both need
  Docker and a release build); wiring `e2e-prod` into `.github/workflows` is a
  CI node.
* `e2e/apps/php/Dockerfile` bakes `/prod` for this suite; the application suite
  ignores it.
