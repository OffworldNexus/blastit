# Result MA.1

## Summary

Lean 0.1.1 user documentation for the three environments: three-step, zero-`sudo` Getting Started; one
unprivileged `blastit check` that proves 4-way completeness with human and `--json` output; presets and
a generic recipe where production fails closed. The Django quickstart no longer ships an escape hatch:
`unrestricted` admin and the `session` group are boxed last resorts with their cost named. Three
fresh-context reviews (dx, ai-ergonomics, security) are resolved below and recorded, not hidden.

Pass 2 of this attempt closed the four remaining page findings (F16/M8, M11, S16, S14 audit) and wrote
this resolution table; pass 1 had closed the security and machine-contract findings. Pass 3 closed the
last MAJORs: the `--json` contract now carries `severity`, `unsafe` and per-proxy counts, `proxy_unused`
fails an unobserved declared proxy, `--learn --json`/`--dry-run` are specified, `manifest.md` shows a
route-level group, and the DigitalOcean migration job uses its own role. Pass 4 fixed the reviewer's
blockers: a valid `[route.db]` grant edit, manifest paths in `fix.to`, the scoped stdout/exit-code
contract, non-owner database roles in both presets, Django's `CSRF_TRUSTED_ORIGINS`, and an
`allow_raw` ingress fix.

## Evidence

### (a) No kernel/gVisor trivia in Getting Started or framework guides

```console
$ grep -RIn -iE "gvisor|chmod o\+rx|sticky|setcap|sys\.path" README.md docs/getting-started.md docs/frameworks/
$ echo "EXIT=$?"
EXIT=1
```

(no output, `EXIT=1` = no matches; `setcap` appears only in the deploy recipes, which need it.)

```console
$ grep -c '^## [0-9]\.' docs/getting-started.md
3
$ grep -n 'no `sudo`' docs/getting-started.md
52:`blastit dev` reads `./blastit.toml` and runs as you: no `sudo`, no setup, no
```

Three numbered steps, and the zero-`sudo` promise is stated in the page itself.

### (b) `make docs-check` (`uvx --offline zensical build --strict`)

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.22s
```

No warnings, broken links or anchors; nav matches the shipped pages.

### (c) The three review artefacts exist and every BLOCKER/MAJOR is covered

```console
$ ls docs/design/ma-lean-dx.review-*.md
docs/design/ma-lean-dx.review-ai-ergonomics.md
docs/design/ma-lean-dx.review-dx.md
docs/design/ma-lean-dx.review-security.md
```

`## Review Resolution` below has one row for every id in the three reviews: dx F1–F23, ai-ergonomics
B1–B6 + M1–M11, security S1–S17.

### (d) No design document and no ADR authored

```console
$ git status --short | grep -E "docs/(design|decisions)"
?? docs/design/ma-lean-dx.review-ai-ergonomics.md
?? docs/design/ma-lean-dx.review-dx.md
?? docs/design/ma-lean-dx.review-security.md
$ git status --short | grep -cE "docs/decisions|docs/design/ma-lean-dx\.md"
0
```

Only the three critic artefacts; no `docs/design/ma-lean-dx.md`, no `docs/decisions/ADR-*.md`.

### (e) The `--json` sample parses, and its counts match the human summary

```console
$ uv run --no-project python -c 'import json,re,pathlib; t=pathlib.Path("docs/ci.md").read_text(); d=json.loads(re.search(r"```json\n(.*?)```", t, re.S).group(1)); print(len(d["findings"]), {k: v["statements"] for k, v in d["proxies"].items()}, sum(v["statements"] for v in d["proxies"].values()) == d["statements"])'
6 {'db': 977, 'analytics': 0} True
```

Six `findings` match the "6 problems" line, and the per-proxy counts add up to the
977 `statements`; every `fix.to` is `"[route.db].queries"`, a path that exists
inside the route and proxy the finding names.

### (f) The pass is recorded in the plan log

```console
$ uv run --no-project tools/plan.py --agent doc-writer log MA.1 "pass 4: fixed reviewer blockers on local.md grant, ci.md --json fix paths + allow_raw, stdout/exit-code contract, non-owner DB roles in both presets, Django CSRF_TRUSTED_ORIGINS, preload build; RESULT rows refreshed"
```

Recorded as the `10:36 [doc-writer]` entry in `plan/nodes/MA/MA.1/LOG.md`.

## Files

- **Added:** `docs/ci.md`, `docs/local.md`, `docs/deploy/{index,digitalocean-paas,compose-traefik,anything-else,fair-queuing}.md`, `docs/design/ma-lean-dx.review-{dx,ai-ergonomics,security}.md`, `plan/nodes/MA/MA.1/RESULT.md`
- **Modified:** `README.md`, `docs/index.md`, `docs/getting-started.md`, `docs/how-it-works.md`, `docs/security.md`, `docs/troubleshooting.md`, `docs/frameworks/{django,flask,index}.md`, `docs/reference/{cli,log-events,manifest,settings}.md`, `zensical.toml`, `plan/nodes/MA/MA.1/LOG.md`
- **Removed (folded into the pages above):** `docs/frameworks/day-to-day.md`, `docs/infra/*.md`, `docs/reference/pytest-plugin.md`

`plan/BUDGET.md` and `plan/JOURNAL.md` are harness-managed, not documentation changes.
Pass 4 edited only this page set; no files were added or removed.

## Follow-ups

Out of scope for this node (other owners or MA.5 design), left unedited:

- `docs/reference/settings.md` / `docs/deploy/anything-else.md` — `BLASTIT_MAX_BODY_BYTES` stays one
  global 100 MiB ceiling with documented `max_body × concurrent uploads` sizing; a lower default and a
  per-route `max_body` are design work (S9).

## Review Resolution

| id | severity | resolved in | summary |
|---|---|---|---|
| F1 | BLOCKER | `docs/index.md#one-file-no-code-changes` | Homepage manifest declares `[proxy.db]` before its `tables` grant, so it boots. |
| F2 | BLOCKER | `docs/frameworks/flask.md#sqlalchemy` | Engine URL rewritten to `postgresql+psycopg://`; Flask-SQLAlchemy takes the same two. |
| F3 | BLOCKER | `docs/deploy/compose-traefik.md#image` | `libcap2-bin` installed before `setcap`. |
| F4 | BLOCKER | `docs/deploy/index.md#measure-your-ingress` | One `curl` with a header replaces the second `app`/`whoami` module. |
| F5 | BLOCKER | `docs/security.md#threat-model` | Import-time code runs in secret-free stage 1, matching `how-it-works.md`. |
| F6 | MAJOR | `docs/deploy/index.md#four-things-a-preset-cannot-do-for-you` | DO reads `do-connecting-ip`; Compose reads the rightmost `X-Forwarded-For`. |
| F7 | MAJOR | `docs/ci.md#github-actions` | Complete workflow (`name`, `on`, `permissions`) plus "commit `uv.lock`". |
| F8 | MAJOR | `docs/ci.md#what-you-see` | One run: 7/8 routes, `checkout` unexercised, 6 problems, the same 6 findings in JSON. |
| F9 | MAJOR | `docs/local.md#fill-the-manifest-in-one-pass`, `docs/frameworks/django.md` | One `learn:`/`check` grammar; route counts add up before and after. |
| F10 | MAJOR | `docs/reference/settings.md#logs` | `json` default, `pretty` in `dev`/`check`, variable wins in both. |
| F11 | MAJOR | `docs/deploy/compose-traefik.md#traefik` | `web` redirect + `websecure` TLS routers; `tls: {}` and ACME/own-cert called out. |
| F12 | MAJOR | `docs/deploy/index.md#measure-your-ingress` | Ordered "deploy, then send one request", after the presets. |
| F13 | MAJOR | `docs/local.md#add-postgresql` | One `docker run` creates the `shop` role and database; both guides link it. |
| F14 | MAJOR | `docs/local.md#add-postgresql` | Environment only; `uv run --env-file .env blastit dev` documented. |
| F15 | MAJOR | `docs/frameworks/django.md#settings` | `ALLOWED_HOSTS` plus TLS `SECURE_PROXY_SSL_HEADER` and `CSRF_TRUSTED_ORIGINS` in the settings block, with what the last two do stated once. |
| F16 | MAJOR | `docs/frameworks/django.md#manifest` | Admin `unrestricted` is a boxed last resort naming the password-hash cost and boot warning, with `path = ['/admin/.*']` and `body = "form"`; `session` attaches route-level to login/logout/notes, never globally. |
| F17 | MAJOR | `docs/ci.md#your-suite`, `docs/troubleshooting.md` | `uv add --dev pytest`; pytest exit 5 mapped to a fix, `blastit check`'s own exit `3`. |
| F18 | MAJOR | `docs/local.md#a-denial` | Copyable normalised statement, the `queries` line inside the route's `[route.db]` block, 42501 wording; JSON adds `reason`. |
| F19 | MAJOR | `docs/deploy/digitalocean-paas.md#spec` | "Replace the `[proxy.db]` block", not "add". |
| F20 | MAJOR | `docs/deploy/digitalocean-paas.md`, `docs/deploy/compose-traefik.md` | Both presets set `BLASTIT_VIEW_MEM_ESTIMATE=140MiB` and a 1 GiB container: `apps-s-1vcpu-1gb` on DO, `mem_limit: 1g` on Compose. |
| F21 | MAJOR | `docs/reference/cli.md#blastit-dev` | `--host` + `BLASTIT_DEV_EXPOSE=1` + boot line; troubleshooting rows. |
| F22 | MAJOR | `docs/local.md#add-postgresql` | Omitted grant → no listener (`ENOENT`); empty `queries` → listener denies all. |
| F23 | MAJOR | `docs/deploy/anything-else.md#kubernetes` | `*` only with the NetworkPolicy admitting the ingress namespace. |
| B1 | BLOCKER | `docs/ci.md#for-agents-json`, `docs/reference/{cli,settings,log-events}.md` | stdout is scoped to server mode; `check --json` is the one exception: one document on stdout, pytest/logs/`learn:` on stderr, exit `2` leaves stdout empty. |
| B2 | BLOCKER | `docs/ci.md#for-agents-json`, `docs/reference/cli.md` | `schema_version: 1`, nine-kind enum, `severity` on every finding (all `error` in 0.1.1; `warning` does not fail), per-proxy `statements`/`denied`, `unsafe: true` on `remove_query`/`remove_route`; exact `sql`, no ellipsis. |
| B3 | BLOCKER | `docs/ci.md#exit-codes`, `docs/troubleshooting.md` | `0` clean, `1` findings, `2` config, `3` suite failed incl. no tests collected; the troubleshooting rows now use `3`. |
| B4 | BLOCKER | `docs/ci.md#bootstrap-an-existing-project` | `--learn` writes exact `queries`, never table grants. |
| B5 | BLOCKER | `docs/reference/manifest.md#ingress` | `form` codec exists; multipart is `raw` today and needs `allow_raw`; `ingress_mismatch` added. |
| B6 | BLOCKER | `docs/ci.md#your-suite` | Documented scope: only what a request sends is audited, so fixtures/migrations are exempt and "fixtures do not change" now means no manifest edit. |
| M1 | MAJOR | `docs/ci.md#for-agents-json` | `unrouted_request` is a finding with separate `method` and `path`. |
| M2 | MAJOR | `docs/ci.md#bootstrap-an-existing-project`, `docs/reference/cli.md#blastit-check` | `--learn --json` prints exactly `{"proposed": {"routes": N, "queries": N}, "wrote": true/false}`; `--learn --dry-run` prints it with `"wrote": false` and writes nothing; the write is atomic and only after a passing suite. |
| M3 | MAJOR | `docs/ci.md#bootstrap-an-existing-project` | A bootable seed is required; the page lists what learn discovers vs what you write. |
| M4 | MAJOR | `docs/local.md#fill-the-manifest-in-one-pass` | Numeric/UUID segments become parameters; learned routes are proposals to tighten. |
| M5 | MAJOR | `docs/ci.md#what-it-fails-on`, `docs/ci.md#for-agents-json` | `proxy_unused` fails a declared proxy with zero statements; per-proxy `statements`/`denied` counts, and "clean" now means every route and proxy was observed. |
| M6 | MAJOR | `docs/ci.md#what-you-see` | Deletes are named unsafe, never automatic; `unsafe: true` marks the destructive `fix`, and deleting a route changes production routing. |
| M7 | MAJOR | `docs/reference/cli.md#blastit-check` | `blastit check -- ARGS…` passthrough documented. |
| M8 | MAJOR | `docs/frameworks/django.md#manifest` | Same fix as F16: admin escape hatch boxed below the quickstart, group route-level only. |
| M9 | MAJOR | `docs/reference/manifest.md#ingress`, `docs/ci.md` | `ingress_mismatch` compares observed content type with the declared codec. |
| M10 | MAJOR | `docs/reference/manifest.md#top-level-keys` | Globals after a table are a parse error with the exact message; route-level `groups` shown in `django.md`. |
| M11 | MAJOR | `docs/frameworks/index.md` | Three separate fences, one `app` each, with `<module>:<callable>` stated. |
| S1 | BLOCKER | `docs/local.md#a-denial`, `docs/reference/manifest.md#grants` | Literals render as `…`; learn/fold refuse literal-bearing statements; no-inline-literals claim. |
| S2 | BLOCKER | `docs/deploy/anything-else.md#kubernetes` | NetworkPolicy admits only the ingress namespace; the false "nothing untrusted is published" comment is gone. |
| S3 | BLOCKER | `docs/local.md`, `docs/security.md#dev-mode-is-not-a-sandbox` | Barrier is `environ`/`argv` only; same-uid can read `.env`/`/proc`; throwaway database only. |
| S4 | MAJOR | `docs/reference/manifest.md#ingress`, `docs/security.md#the-mechanisms` | `413` runtime default, `raw` needs `allow_raw` and a CI finding, 0.1.1 has no media/PDF CDR. |
| S5 | MAJOR | `docs/reference/manifest.md#routes`, `docs/security.md#the-mechanisms` | `egress = true` is any address/port including metadata; no allowlist. |
| S6 | MAJOR | `docs/reference/settings.md#privileges-and-the-uid-layout` | seccomp is in the fail-closed sentence; `0` warns loudly and presets abort; troubleshooting row. |
| S7 | MAJOR | `docs/deploy/compose-traefik.md#compose`, `docs/deploy/digitalocean-paas.md#spec`, `docs/deploy/index.md#four-things-a-preset-cannot-do-for-you` | Compose creates a non-owner `app` role granted by `db/grants.sql`; DO creates `app` and `migrator`; the web service uses the app role's secret URL. |
| S8 | MAJOR | `docs/deploy/digitalocean-paas.md#spec`, `docs/deploy/compose-traefik.md#compose` | The migration job/service owns the schema and has its own URL (`migrator` / `owner`), never the web service's shared app environment; the trusted-code boundary is named. |
| S9 | MAJOR | `docs/reference/settings.md#http`, `docs/deploy/anything-else.md#memory-and-process-caps` | Oversized `Content-Length` rejected before buffering; `max_body × concurrent uploads` sized against container memory. Scope: one global ceiling in 0.1.1; per-route cap deferred (Follow-ups). |
| S10 | MAJOR | `docs/local.md#start` | `--host` needs `BLASTIT_DEV_EXPOSE=1`; the boot line names address and uid; no-auth warning. |
| S11 | MAJOR | `docs/local.md#a-denial`, `docs/reference/manifest.md#grants` | Exact grants are unanalysed: warning names `pg_read_file`, `COPY … PROGRAM`, `dblink`, `lo_import`. |
| S12 | MAJOR | `docs/local.md#add-postgresql` | `-p 127.0.0.1:5432:5432` and `POSTGRES_USER=shop`, matching the URL. |
| S13 | MINOR | `docs/security.md#the-mechanisms` | "No process *after* stage 0"; stage 0 keeps the secret only until its `execve`. |
| S14 | MINOR | `docs/deploy/compose-traefik.md#image`, `docs/deploy/anything-else.md#plain-docker` | `--env-file` keeps passwords and secret keys out of shell history. |
| S15 | MINOR | `docs/deploy/anything-else.md#kubernetes` | `SETPCAP` added to the Kubernetes capability list. |
| S16 | MINOR | `docs/deploy/compose-traefik.md#compose` | `sslmode=disable` stated as plaintext on the bridge; a remote database needs `ca_cert` on `[proxy.db] input`. |
| S17 | MINOR | `docs/ci.md#ci-cd` | "Nothing **your suite exercised** was forgotten." |
