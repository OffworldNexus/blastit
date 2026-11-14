# PHP-FPM, Symfony and Laravel

Serve a Symfony or Laravel app from `public/index.php` with one manifest block — no
nginx, no rewrite rules. `0.2.1` adds it, with `check -- <suite>`.

Write `blastit.toml` in your project root, next to `composer.json`:

```toml title="blastit.toml"
version = 1

[[route]]
name = "app"
path = ['.*']

[route.static]
root = "public"
immutable = ["/assets/*"]

[route.fastcgi]
entry = "public/index.php"

[route.ingress]
body = "form"
```

A file under `public/` is answered by the static responder; any other path reaches the
front controller. No `.php` file under `public/` is served; only `entry` runs, and a
`[route.fastcgi]` route needs no `app` — the front controller is the app.

| Key | What it does |
|---|---|
| `entry` | required; relative to `blastit.toml`, and with `[route.static]` it includes `root`: `public/index.php`, never `index.php` |
| `workers` | optional; omit it. It caps this route at — never above — the concurrency Blastit derives; a value above the budget is a boot error |

`[route.static]` is optional: without it every path reaches the front controller.
`path` entries are regexes matched whole, `immutable` entries are globs, and a route
takes one codec: a JSON API beside the form app is a second `[[route]]` above this
catch-all, with its own `name`, `path = ['/api/.*']`, `[route.fastcgi] entry` and
`[route.ingress] body = "json"` — first match wins, in manifest order. A body that
does not match the codec is a `400`; no `[route.ingress]` is a `413`; an upload has no
codec: `raw` with `allow_raw = true`, scoped to its path.

A route that talks to PostgreSQL needs the top-level `[proxy.db]`
(`protocol = "postgresql"`, `input = { url = "DATABASE_URL" }`) and its own
`[route.db]` grant; with no `[route.db]` it gets no connection. `blastit dev --learn`
proposes a missing `root`; `check --learn` writes your queries into `blastit.toml`.

## Local

PHP 8.2 or newer must already be on `PATH`, or point
[`BLASTIT_PHP_FPM`](../reference/settings.md#backends) at it: Blastit runs it and does
not install or upgrade it. Nothing goes in `/etc`, no `sudo`, and it reads `./blastit.toml` from here:

```console
$ uv tool install blastit
```

```console
$ blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

Blastit runs PHP for the route itself: one command, no server to start beside it,
nothing left running after Ctrl-C. It does not install dependencies, migrate, warm
caches or read `.env`; run `composer install` yourself.

A manifest it cannot use stops the boot earlier, one line per error:
`blastit dev: manifest ./blastit.toml: manifest has 1 error(s)` then
`  - route "app" (index 0): fastcgi entry "index.php": must be under static root "public"`.
Fix that error. With a front controller that answers `/users/42`:

```console
$ curl -i http://127.0.0.1:8080/users/42
HTTP/1.1 200 OK
Content-Type: application/json

{"route":"user.show","id":"42"}
```

`immutable` values are served-path globs (`/assets/*` for `public/assets/`, `/build/*`
for Encore's `public/build/`), matched whole, and `*` stops at `/`: point each at
fingerprinted output only, never a path that can answer HTML or an unhashed original.

```console
$ curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.css
HTTP/1.1 200 OK
Content-Type: text/css; charset=utf-8
Cache-Control: public, max-age=31536000, immutable
```

A query string is data, never PHP options: the front controller's `/` route echoes
what PHP saw, and `SCRIPT_FILENAME` stays pinned to `entry` ([security](../security.md)).

```console
$ curl -i 'http://127.0.0.1:8080/?-d+allow_url_include=1'
HTTP/1.1 200 OK
Content-Type: application/json

{"route":"home","allow_url_include":"0","query_string":"-d+allow_url_include=1"}
```

## CI/CD

With `vendor/` installed, one command starts the app on loopback, runs your suite and
audits every request and query it saw:

```console
$ blastit check -- vendor/bin/phpunit
blastit check: 24 requests, 0 statements, 1/1 routes, clean
```

Bootstrap once, never as the gate: `--learn` records instead of denying:

```console
$ blastit check --learn -- vendor/bin/phpunit
```

`check` puts the front controller on loopback as `BLASTIT_URL=http://127.0.0.1:8080`
and runs the command after `--`, which is your suite and required: `check` exits `2`
when no command follows it. The suite must speak HTTP: Symfony's `KernelBrowser` and
Laravel's `$this->get()` run it in-process and send nothing, so use Symfony's
`HttpBrowser`, Laravel's `Http::get(getenv('BLASTIT_URL').'/users/42')`, or
`file_get_contents(getenv('BLASTIT_URL').'/users/42')`. A route is exercised when its
front controller ran, not by an asset; an unrequested route, a body route with no input
rule, or a codec that does not match is a finding (exit `1`). `--json` names unexercised
routes — one document on stdout, everything else on stderr:

```console
$ blastit check --json -- vendor/bin/phpunit
```

```json
{"schema_version":1,"ok":false,"requests":3,"statements":0,"routes":{"total":1,"exercised":0},"proxies":{},"findings":[{"kind":"unexercised_route","severity":"error","route":"app","fix":{"action":"add_test"}}]}
```

[CI/CD](../ci.md#what-it-fails-on) owns the full list and [exit codes](../ci.md#exit-codes).

## Deployments

```console
$ blastit --preset compose-traefik
```

The image needs the PHP Blastit runs; the first route's uid `21000` needs write access
to Symfony's `var/` or Laravel's `storage/` and `bootstrap/cache/`. Locally the preset
exits `1`: production refuses to run without its capabilities. See [Deployments](../deploy/index.md)
· [the manifest](../reference/manifest.md) · [static assets](static-and-spa.md).
