# MB.21.6 — DigitalOcean page and its docs check agree on the Blastfile CA spelling

STATUS: done (the epic gate's docs line is fixed; `make e2e` now stops on a *different*,
pre-existing leg — see "Follow-ups")

## What changed

`docs/deploy/digitalocean-paas.md` (209 → 232 lines; no TOML, no `=` directives)

- Step 1 shows the store the reader writes, instead of describing it:

  ```
  postgres {
      host $DATABASE_HOST
      port $DATABASE_PORT
      user $DATABASE_USERNAME
      password $DATABASE_PASSWORD
      database $DATABASE_DATABASE
      sslmode $DATABASE_SSLMODE
      ca_cert $DATABASE_CA_CERT
  }
  ```

  It replaces "the split `postgres` block with `$DATABASE_CA_CERT` beside `$DATABASE_URL`",
  which described a shape the parser refuses (`UrlSlot`; `StoreTwoForms`). Every name is a
  `$IDENT` the parser accepts (`parser.rs:938`), matching the reference's directives.
- Step 1 states the consequence of a block form: the app gets `DATABASE_HOST` (socket
  directory), `DATABASE_PORT`, `DATABASE_USERNAME`, `DATABASE_DATABASE`, never the password and
  no `DATABASE_URL`, so `settings.py` builds `DATABASES` from those four — and the `migrate`
  job keeps reading its own `DATABASE_URL`.
- Step 5 and the `app.yaml` `services:` block now fill those slots from the pool's bindables
  (`${db.shop-pool.HOSTNAME|PORT|USERNAME|PASSWORD|DATABASE}`, port 25061),
  `DATABASE_SSLMODE` = the literal `verify-full`, and `DATABASE_CA_CERT` = `${db.CA_CERT}` (the
  one cluster value the web service reads). The old `DATABASE_URL: ${db.shop-pool.DATABASE_URL}`
  line is gone: a block form produces no URL for the app.
- Step 4 says every `${db.shop-pool.*}` is that pool's name, and the TLS table row says the leg
  is `verify-full` with the cluster's CA.
- The fence is `text title="Blastfile: the store block"` — **not** `text title="Blastfile"` and
  not a plain keyword-led `text` fence: `docs_fences` pins `PAGES`/`STATEMENT_LISTS` and this
  page has no row in either, so a document fence (or a plain `text` fence whose first
  non-comment line is `postgres`) fails `cargo test -p blastit-manifest --test docs_fences`.
  The node's Notes suggest a `#` comment first line for a plain fence; that does not work,
  because `first_statement` skips comments and still classes the body keyword-led.

`e2e/docs_deploy.py` (still 45 promises)

- `'ca_cert = \"DATABASE_CA_CERT\"' in page and "key: DATABASE_CA_CERT" in services` is replaced
  by one assertion that parses the page's `Blastfile: the store block` fence and proves: the
  seven slots and their exact variables; every variable present as a `key:` in `services:`;
  `DATABASE_SSLMODE` = `verify-full`; `DATABASE_CA_CERT` = `${db.CA_CERT}`; and the retired
  `ca_cert = "` spelling absent. A failure names every gap, so a red line says what drifted.
- The two web-service URL checks are rewritten to the split form: every connection value the
  web service gets is a `${db.shop-pool.*}` bindable, and no
  `${db.DATABASE_URL|HOSTNAME|PORT|USERNAME|PASSWORD|DATABASE}` reaches a service (the
  cluster's own are `doadmin`'s on 25060); `${db.CA_CERT}` is the one cluster value allowed.
  The migrate job's direct 25060 URL as `migrator` is still asserted.
- Sweep: nothing else was stale. `postgresql://` (the `OWNER_URL` and the migrate job's libpq
  URL) is legitimate; `25061`/`25060`/`shop-pool`/`name \`shop-pool\``/`${APP_DOMAIN}` hold;
  `allow_raw`, `[route` and a store-block `workers` are asserted nowhere, and
  `BLASTIT_MAX_WORKERS` in the prose is a host env var, not a store slot.

`docs/knowledge/LESSONS.md`: one line recording the fence-census gotcha (outside the node's two
paths — as are `RESULT.md`/`LOG.md`; the dispatch brief asks for the line).

## Acceptance

```console
$ uv run --no-project e2e/docs_deploy.py
...
ok    digitalocean: every connection value the web service gets is the pool's (25061)
ok    digitalocean: no service reads the cluster's connection values where the pool's belong
ok    digitalocean: the migrate job's URL is the direct port 25060 as `migrator`
ok    digitalocean: the proxy's `ca_cert` and `sslmode` inputs and the service env agree
ok    digitalocean: the spec still attaches the cluster
docs_deploy: 45/45 documented pooler promises hold          # exit 0
```

Negative proofs (each mutation applied to the page, harness rerun, page restored, 45/45 again):

```console
# 1. delete the page's `ca_cert $DATABASE_CA_CERT` line
FAIL  digitalocean: the proxy's `ca_cert` and `sslmode` inputs and the service env agree (block is {'host': 'DATABASE_HOST', ..., 'sslmode': 'DATABASE_SSLMODE'})
docs_deploy: 44/45 documented pooler promises hold          # exit 1

# 2. delete the spec's `key: DATABASE_CA_CERT, scope: RUN_TIME` line
FAIL  digitalocean: the proxy's `ca_cert` and `sslmode` inputs and the service env agree (unset: ca_cert $DATABASE_CA_CERT; `DATABASE_CA_CERT` is not the cluster's `${db.CA_CERT}`)
docs_deploy: 44/45 documented pooler promises hold          # exit 1

# 3. restore the retired spelling `ca_cert = "DATABASE_CA_CERT"`
FAIL  digitalocean: the proxy's `ca_cert` and `sslmode` inputs and the service env agree (block is {...}; the retired `ca_cert =` spelling is back)
docs_deploy: 44/45 documented pooler promises hold          # exit 1
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.34s                                       # exit 0

$ make doc-tone
doc-tone: ok — 30 user-facing page(s), no banned phrase       # exit 0

$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 0.59s                                            # exit 0

$ CARGO_NET_OFFLINE=true cargo test --locked -p blastit-manifest --test docs_fences
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # exit 0

$ wc -l docs/deploy/digitalocean-paas.md docs/blastfile.md
  232 docs/deploy/digitalocean-paas.md
   98 docs/blastfile.md                                       # untouched
```

`app.yaml` also parsed as YAML with PyYAML (web env keys: the five `DATABASE_*` pool
bindables, `DATABASE_SSLMODE: verify-full`, `DATABASE_CA_CERT: ${db.CA_CERT}`, then
`ALLOWED_HOST`, `DJANGO_SECRET_KEY`, `BLASTIT_PRESET`, `BLASTIT_VIEW_MEM_ESTIMATE`).

```console
$ timeout 1800 make e2e
...
E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
...
docs_deploy: 48/48 documented pooler promises hold     # <- the docs gate returned 0
...
Traceback (most recent call last):
  File ".../e2e/php2shell.py", line 1231, in main
    return self_test(arguments.page, arguments.matrix)
IsADirectoryError: [Errno 21] Is a directory:
  '.../docs/frameworks/php.md'
make: *** [Makefile:501: e2e] Error 1                  # exit 2
```

The docs line is green inside `make e2e` (that run predates the final collapse to 45 checks —
it printed 48/48 from the same `docs_deploy.main()`); the final revision is proven at 45/45
above. `make e2e` now stops **after** it, on a different leg (below).

## Follow-ups (outside this node's paths)

1. **`make e2e` is still blocked, by the frameworks restructure (`MB.20`/`MB.22`), not by this
   node.** Four references still name the page the restructure moved to
   `docs/frameworks/php/index.md`: `e2e/php2shell.py:75` (`PAGE = ROOT / "docs" / "frameworks" /
   "php.md"`), `e2e/run.py:148` (`--page /opt/e2e/php.md`), `e2e/compose.yaml:133`
   (`../docs/frameworks/php.md:/opt/e2e/php.md:ro`) and a comment at `e2e/php/Dockerfile:14`. The
   page of record is gone, so the php leg dies in `self_test` before it probes anything. Fix: point
   all four at `docs/frameworks/php/index.md` and re-check the sections they cite (§1 install,
   §2 static root).
   Second-order effect, cleaned up here: the compose mount re-creates the missing source as an
   empty **root-owned** `docs/frameworks/php.md/` directory, which makes `check_doc_tone.py`'s
   `docs/**/*.md` walk and `php2shell`'s `PAGE` read fail with `IsADirectoryError` instead of
   "file not found" — it breaks `make doc-tone` for a page nobody edited. `rmdir docs/frameworks/
   php.md` (from the writable parent) restores `doc-tone: ok … no banned phrase`; it returns on
   the next `make e2e` until the mount is repointed.
2. `docs/deploy/compose-traefik.md:171-173` carries the same retired sentence ("the split
   `postgres` block with `$DATABASE_CA_CERT` beside `$DATABASE_URL`") with no directive and no
   `docs_deploy` assertion. Its shown `compose.yaml` is an in-compose database
   (`sslmode=disable`), so the page is not wrong, but its prose describes a case it never shows.
3. `make docs-test` is red on this shared tree, unrelated: `docs/reference/cli.md fence 1
   dev-flags` runs `blastit dev --no-reload`, which the binary does not accept yet (another
   session's docs-first `MB.24` work).

## Review Resolution

**MAJOR — the `ALLOWED_HOSTS` step was lost.** `docs/deploy/digitalocean-paas.md` (now 234
lines, cap 235) again tells the reader, in the store block's paragraph: "`settings.py` reads the
host the spec passes down: `ALLOWED_HOSTS = [os.environ["ALLOWED_HOST"]]` — Django does not read
`ALLOWED_HOST` on its own." Without it `ALLOWED_HOST: ${APP_DOMAIN}` (`app.yaml`) is inert and
Django answers **400 on every request behind the edge**. A new check now guards the sentence
(the page had none before, so deleting it used to leave the harness green):

```console
$ uv run --no-project e2e/docs_deploy.py      # ALLOWED_HOSTS sentence deleted from the page
FAIL  digitalocean: the page tells Django to read the `ALLOWED_HOST` the spec passes down
docs_deploy: 45/46 documented pooler promises hold          # exit 1
```

**MINOR — presence is not pairing.** The one CA/TLS assertion now pins every
slot→bindable line as an exact pair, from one `pool` map (`DATABASE_HOST`→`HOSTNAME`,
`DATABASE_PORT`→`PORT`, `DATABASE_USERNAME`→`USERNAME`, `DATABASE_PASSWORD`→`PASSWORD`,
`DATABASE_DATABASE`→`DATABASE`), so a swapped bindable is red instead of merely present:

```console
$ uv run --no-project e2e/docs_deploy.py      # DATABASE_HOST given ${db.shop-pool.PORT}
FAIL  digitalocean: every connection value the web service gets is the pool's (25061)
FAIL  digitalocean: the proxy's `ca_cert` and `sslmode` inputs and the service env agree (not the pool's own bindable: DATABASE_HOST)
docs_deploy: 44/46 documented pooler promises hold          # exit 1
```

Both mutations restored; the counts above supersede the 45/45 and 232-line figures earlier in
this file:

```console
$ uv run --no-project e2e/docs_deploy.py
docs_deploy: 46/46 documented pooler promises hold          # exit 0   (46: +1 ALLOWED_HOSTS)
$ make docs-check
No issues found
Build finished in 0.30s                                     # exit 0
$ make doc-tone
doc-tone: ok — 30 user-facing page(s), no banned phrase     # exit 0
$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 0.96s                                          # exit 0
$ wc -l docs/deploy/digitalocean-paas.md docs/blastfile.md
  234 docs/deploy/digitalocean-paas.md
   98 docs/blastfile.md                                     # untouched
```

`cargo test -p blastit-manifest --test docs_fences` still 11 passed (the store fence is an
illustration; the new prose adds no fence).
