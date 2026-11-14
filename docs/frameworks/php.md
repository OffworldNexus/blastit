# PHP (Symfony, Laravel)

Serve a Symfony or Laravel app from `public/index.php` with one manifest block — no
rewrite rules, no PHP server beside it. `0.2.0` ships it, with `check -- <suite>`.

Write `blastit.toml` in your project root, next to `composer.json`:

```toml title="blastit.toml"
version = 1

[[route]]
name = "app"
path = ['.*']

[route.static]
root = "public"
immutable = ["/assets/*"]

[route.php]
entry = "public/index.php"

[route.ingress]
body = "form"
```

A file under `public/` is answered by the static responder; every other path reaches the
front controller. Blastit executes only `entry`, and a `[route.php]` route needs no
`app` — the front controller is the app. `.php`/`.phar`/`.phtml` files under a static
root are refused with `404`; only `entry` runs, so keep other sources out of `public/`.

| Key | What it does |
|---|---|
| `entry` | required; a relative `.php` path (no `..`, no backslash), and with `root = "public"` it is `public/index.php`, not `index.php` |
| `workers` | optional, `1`–`64`; omit it — Blastit derives the per-route budget |

`[route.static]` is optional: without it every path reaches the front controller.
`spa = true`, `CONNECT` and `TRACE` never reach the front controller and are boot errors.
`path` entries are regexes matched whole, `immutable` entries are globs, and a route
takes one codec: a JSON API beside the form app is **another** `[[route]]` **before** this
catch-all, with its own `name`, `path = ['/api/.*']`, `[route.php]`
(`entry = "public/index.php"`) and `[route.ingress]` (`body = "json"`) — first match wins,
in manifest order. A body that does not match the codec is a `400`; no `[route.ingress]`
is a `413`; an upload has no codec: `raw` with `allow_raw = true`, scoped to its path.

A route that talks to PostgreSQL needs `[proxy.db]` (`protocol = "postgresql"`,
`input = { url = "DATABASE_URL" }`) and its own `[route.db]` grant; without one it gets no
connection. `blastit dev --learn` records the routes and exact queries you click through;
`check --learn` writes them into `blastit.toml`.

## Local

Blastit runs PHP itself: the host's NTS `libphp.so` (8.2–8.4) is embedded in the route's
own worker. The library is a distro package — `libphp-embed` on Debian/Ubuntu,
`php8X-embed` for your minor on Alpine (e.g. `php84-embed`) or `php-embedded` on
Fedora/RHEL — or point [`BLASTIT_LIBPHP`](../reference/settings.md) at yours. Installing
it is the one privileged step; Blastit never writes `/etc` and needs no `sudo`.

No usable NTS `libphp` is not a degraded mode: the boot stops before it binds, naming the
package and `BLASTIT_LIBPHP`. Install Blastit, then run `composer install` in the project
root:

```console
$ uv tool install blastit
```

Then, from that directory:

```console
$ blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

Nothing is left running after Ctrl-C. Blastit does not install dependencies, migrate,
warm caches or read `.env`.

A manifest it cannot use stops the boot earlier:
`blastit dev: manifest ./blastit.toml: manifest has 1 error(s)` then
`  - route "app" (index 0): php entry "index.php": must be under static root "public"`.
Fix that error. With a front controller that answers `/users/42`:

```console
$ curl -i http://127.0.0.1:8080/users/42
HTTP/1.1 200 OK
Content-Type: application/json

{"route":"user.show","id":"42"}
```

`immutable` values are served-path globs (`/assets/*`, `/build/*` for Encore's or Laravel
Vite's `public/build/`), matched whole; `*` stops at `/`. Point each at fingerprinted
output only, never a path that answers HTML or an unhashed original.

```console
$ curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.css
HTTP/1.1 200 OK
Content-Type: text/css; charset=utf-8
Cache-Control: public, max-age=31536000, immutable
```

A query string is data, never PHP options: the front controller's `/` route echoes what
PHP saw, `-d allow_url_include` included ([security](../security.md)).

## CI/CD

With `vendor/` installed, one command runs your suite and audits every request and query:

```console
$ blastit check -- vendor/bin/phpunit
blastit check: 24 requests, 0 statements, 1/1 routes, clean
```

Bootstrap once, never as the gate: `--learn` records instead of denying:

```console
$ blastit check --learn -- vendor/bin/phpunit
```

`check` puts the front controller on loopback as `BLASTIT_URL=http://127.0.0.1:8080`
and runs the command after `--`, your suite and required: `check` exits `2` without one.
The suite must speak HTTP: Symfony's `KernelBrowser` and Laravel's `$this->get()` run it
in-process and send nothing, so use Symfony's `HttpBrowser`, Laravel's
`Http::get(getenv('BLASTIT_URL').'/users/42')`, or
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

The image must carry the same NTS `libphp` — `libphp-embed` in a Debian-based image;
the first route's uid `21000` needs write access to Symfony's `var/` or Laravel's
`storage/` and `bootstrap/cache/`. Locally it exits `1`: production refuses to run
without its capabilities. See [Deployments](../deploy/index.md) ·
[the manifest](../reference/manifest.md#php) · [static assets](static-and-spa.md).
