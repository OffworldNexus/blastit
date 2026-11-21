# MB.41.2 — docs: deploy / frameworks / reference / how-it-works / security / troubleshooting

Status: done (fix round 2 of the UX/arch reviews). No branch/commit; edits stay in the worktree.

## What changed (the review list, nothing else)

1. `docs/deploy/digitalocean-paas.md` — PHP and Static tabs now install with
   `tar -xzf blastit-*.tar.gz -C /usr/local --strip-components=1`, which keeps
   `bin/blastit` (symlink) beside `share/blastit/{lib,python}`, so the PHP shims stay where the
   production boot looks for them (the executable's own directory). The Static tab gained
   `apt-get update && apt-get install -y --no-install-recommends ca-certificates curl` before it
   curls. Both images boot `entrypoint php` / a static site.
2. `docs/deploy/compose-traefik.md` §4 — each tab is now a complete image: Python = the previous
   `python:3.13-slim` + `libcap2-bin` + the `python3.13-init` capability copy; PHP =
   `debian:trixie-slim` + `ca-certificates curl libcap2-bin libphp-embed php-pgsql` + archive
   unpack + `blastit-init` copy beside the shims + `setcap` + `COPY . .` + `EXPOSE 8080`; Static &
   SPA = the same minus PHP. The "a file with capabilities turns any process of that uid into init"
   paragraph moved out of the Python fence to one runtime-neutral paragraph after the tabs. §5 now
   names the two real commands — `["/usr/local/bin/python3.13-init", "-m", "blastit"]` and
   `["/usr/local/share/blastit/lib/blastit-init"]` — instead of an undefined `blastit-init`.
3. `docs/deploy/anything-else.md` §"Plain Docker" — three complete Dockerfiles
   (`FROM` → runtime install → `COPY . .` → archive unpack / `pip install` → `HEALTHCHECK` →
   `CMD ["blastit"]`), each with its own probe (`python3 -c …` vs `curl -fsS …/_blastit/health`).
   The shared `docker run` dropped `--health-cmd`; the prose says the check lives in the image.
4. `docs/frameworks/python/django.md` — the admin's grant set is restored:
   `auth_user(id, username, password, last_name, first_name, email, is_staff, is_active,
   is_superuser, last_login, date_joined)`, `auth_group(id, name), django_content_type(id,
   app_label, model)`, `auth_permission(id, name, codename, content_type_id)`; the prose says the
   permission lookup joins `content_type_id` to `django_content_type` instead of "add the join
   tables when `check` reports them". Canonical `fmt` layout (no wrapped continuation line).
5. `docs/frameworks/static-and-spa.md` — the banner transcript reads
   `  http · scheduler · dbproxy · 0 routes`, and the sentence below `check` now says the banner and
   the document both count none. `e2e/static_assets.py` pins the new claim
   (`page_summary.endswith(" 0 routes")`) and its `_BANNER_SUMMARY` comment, `banner_line` docstring
   and inline comment no longer describe the retired one-route page.

## Evidence

```
$ grep -rc 'uvx blastit' docs/ | grep -v ':0' || echo zero
zero            # only docs/design/*.md (excluded from the public build) mention it historically
$ grep -rn '=== "Python (WSGI)"|=== "PHP"|=== "Static & SPA"' docs/deploy/*.md
docs/deploy/anything-else.md:156,166,181
docs/deploy/compose-traefik.md:133,159,184
docs/deploy/digitalocean-paas.md:84,93,107
```

Every `text title="Blastfile"` fence (16 + the DigitalOcean `store block`) extracted with its tab
indentation and checked against the built binary:

```
$ target/debug/blastit fmt --check <each fence>
ok   docs/index.md (3) · docs/blastfile.md (3) · docs/getting-started.md (3)
ok   docs/frameworks/{static-and-spa, python/index, python/django, python/flask,
                       php/index, php/symfony, php/laravel}.md
ok   docs/deploy/digitalocean-paas.md "Blastfile: the store block"
fences: 16 failures: 0
```

Final gate, run once:

```
$ make docs-check platform-matrix-check zed-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.61s
...
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised:
  4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; reaches: served 6 of 12 shims, 0 probed,
  0 refused, 6 unbuilt; 12 served once the aarch64 legs run; 0 leg(s) dropped
zed-check: OK (59 properties checked)
Ran 44 tests in 8.788s
OK
GATE_EXIT=0

$ uv run --no-project e2e/static_assets.py --self-test
PASS: the driver still asserts every refusal the page names
PASS: self-test, 17 checks
```

`git diff --check` over the edited paths is clean.

## Follow-ups

* None from this round: the remaining review MINORs (`reference/settings.md:111` ingress wording,
  `frameworks/index.md:41` PHP CI gate, `anything-else.md:213` sample password, the DigitalOcean
  "a static site skips steps 2-6" scoping line, `reference/cli.md:16` "same CLI from the wheel",
  PHP `check` exit `2` wording, `how-it-works.md` length, `inventory.toml:265,285` trailing
  whitespace, `anything-else.md` Mermaid diagram, `compose-traefik.md:298` `!!! warning`) were
  outside the fix list handed to this round.
