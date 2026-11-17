---
id: MB.21.6
title: Make the DigitalOcean deploy page and its docs check agree on the Blastfile CA spelling
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["docs/deploy/digitalocean-paas.md", e2e/docs_deploy.py]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-17T05:46"
updated: "2026-11-17T05:56"
---

# Goal

The epic gate's `make e2e` fails `digitalocean: the proxy's ca_cert input and the service env
agree` because `e2e/docs_deploy.py:192` still greps the retired TOML spelling
`ca_cert = "DATABASE_CA_CERT"` while `docs/deploy/digitalocean-paas.md` (rewritten for `Blastfile`
in `MB.21.5.1`) describes the split store block in prose only. The page must show the directive the
reader writes (`ca_cert $DATABASE_CA_CERT` inside the `postgres { … }` block, with `sslmode`
`verify-full`), and the check must prove the directive and the `key: DATABASE_CA_CERT` service env
agree — not merely that the page mentions the variable.

# Context

- The gate's one failure is `e2e/docs_deploy.py:192`:
  `'ca_cert = "DATABASE_CA_CERT"' in page and "key: DATABASE_CA_CERT" in services`.
  `uv run --no-project e2e/docs_deploy.py` reports `44/45 documented pooler promises hold`.
- The page promises the split block in prose only (`docs/deploy/digitalocean-paas.md:64-67`:
  "the split `postgres` block with `$DATABASE_CA_CERT` beside `$DATABASE_URL`") and never shows a
  directive. "Beside `$DATABASE_URL`" is itself the retired TOML shape: the parser refuses a URL and
  a block in one store (`crates/blastit-manifest/src/blastfile/parser.rs:909`
  `StoreTwoForms`) and refuses the `url` word inside a block (`parser.rs:939` `UrlSlot`), so a store
  that carries `ca_cert` **is** the split block.
- Accepted spellings: `docs/reference/blastfile.md:131` documents `ca_cert` as a `$IDENT` holding
  the PEM; `parser.rs:938` accepts it. `sslmode` is also a `$IDENT` holding
  `disable|require|verify-ca|verify-full` — never a bare word, so the page writes
  `sslmode $DATABASE_SSLMODE` and the service env supplies `verify-full`.
- What the app gets from a split store (`crates/blastit-env/src/postgresql.rs:303-325`, reference
  `:136-144`): `host` → the view's socket dir, `port` → the local listener port, `user` → `blastit`,
  `database` → unchanged, `sslmode` → `disable`, and `password`/`ca_cert` → removed. There is **no**
  `DATABASE_URL` for the web service in block form, so the page's `settings.py` line must read the
  slot variables it actually receives; the migrate job keeps its own direct URL.
- DigitalOcean bindables (verify each with `doctl apps spec` / DO's app-spec docs before shipping —
  never invent a name): pool-scoped `${db.shop-pool.HOSTNAME}`, `PORT`, `USERNAME`, `DATABASE_URL`;
  db-scoped `${db.CA_CERT}` and `${db.PASSWORD}`. Give the `databases:` entry `db_name: defaultdb`
  and `db_user: app` so `${db.PASSWORD}` is the platform-managed `app` password. The pool's user is
  `app`, so the split block's `user` and `password` are the pool's login.
- Sweep the rest of `e2e/docs_deploy.py` for retired spellings and fix only real mismatches:
  `postgresql://` occurrences in the page are legitimate libpq URLs (`OWNER_URL`, the migrate job) —
  keep them; `[route` and `allow_raw` are absent; `BLASTIT_MAX_WORKERS` is a host env var, not a
  store slot (only `workers` *inside* a store block is refused), so it stays. Expect no sweep edits
  — verify, do not rewrite.
- Fence census constraint: `crates/blastit-manifest/tests/docs_fences.rs` pins `PAGES` and
  `STATEMENT_LISTS`, and this deploy page has **no** row. A `text title="Blastfile"` document fence,
  or a plain keyword-led `text` fence (first non-comment line starts with
  `app|entrypoint|postgres|trait|view|static`), fails that suite. Show the block as inline code, or
  as a plain `text` illustration whose first non-comment line is not one of those six words (for
  example a `#` comment then `sslmode $DATABASE_SSLMODE`).

# Acceptance

```console
$ uv run --no-project e2e/docs_deploy.py
$ # negative proof: delete the page's `ca_cert $DATABASE_CA_CERT` line, rerun, restore
$ make docs-check
$ make doc-tone
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
$ CARGO_NET_OFFLINE=true cargo test --locked -p blastit-manifest --test docs_fences
$ make e2e
$ wc -l docs/deploy/digitalocean-paas.md
```

1. `uv run --no-project e2e/docs_deploy.py` exits 0 and prints `docs_deploy: 45/45 documented
   pooler promises hold`. The total stays 45: replace the one failing `checks.that` with one
   assertion, and rewrite the two web-service URL checks to the pool-scoped slots (web → pool port
   25061 via `${db.shop-pool.PORT}`, migrate → direct 25060 as `migrator`) so the two-URL rule is
   still counted rather than eyeballed.
2. The replacement check proves agreement, not mention: `ca_cert $DATABASE_CA_CERT` and
   `sslmode $DATABASE_SSLMODE` appear on the page, `key: DATABASE_CA_CERT` and
   `key: DATABASE_SSLMODE` with `verify-full` appear in the manifest's `services:` block, and the
   retired `ca_cert = "` spelling appears nowhere.
3. Negative proof: with the page's `ca_cert $DATABASE_CA_CERT` line deleted, the script exits 1 and
   names the CA agreement check; restore the line and rerun to `45/45`. Record both tails.
4. `make docs-check` (Zensical `--strict`), `make doc-tone`, the pytest run and
   `cargo test -p blastit-manifest --test docs_fences` all exit 0.
5. `make e2e` exits 0 (the epic gate's command), and `wc -l docs/deploy/digitalocean-paas.md`
   reports the new length — record it and keep the page a deployment recipe (≤ 235 lines).

# Notes

- Keep the page a deployment recipe: no kernel/WASM/broker prose, no TOML, no second way to do the
  same thing. The block is the one shape a CA can ride, so the page stops saying "beside
  `$DATABASE_URL`" and shows the block with the pool's slots.
- Delete, never annotate: no "v1", "previously", "the old spelling", and no link to a page about the
  change (`python/tests/test_docs_assets.py` refuses those words; read its docstring before prose).
- If the page shows the `settings.py` consequence, show only the keys the app receives from the
  block (`DATABASE_HOST`, `DATABASE_PORT`, `DATABASE_USERNAME`, `DATABASE_DATABASE`) plus the
  existing `ALLOWED_HOSTS` line, and say the password is not in the app's environment. Do not add a
  `DATABASE_URL` to the web service: a block gives none, and an unused knobs is the bug this gate
  catches.
- This node exists because the epic gate caught the mismatch, so the check must guard the Blastfile
  reality from now on: it fails on a page that only mentions the variable name, and on a page that
  still carries the `=` spelling.
- Touch only the two paths: `docs/deploy/digitalocean-paas.md` and `e2e/docs_deploy.py`. If the
  fence census needs a `PAGES`/`STATEMENT_LISTS` row, stop and open a follow-up request instead of
  editing the fixture.
