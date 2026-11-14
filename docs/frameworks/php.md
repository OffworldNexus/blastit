# PHP-FPM, Symfony and Laravel

`0.2.1` adds `[route.fastcgi]`, the PHP `--learn` write and `check -- <suite>`.

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
listen = "run/fpm.sock"
workers = 4
```

A file under `public/` is answered by the static responder; any other path reaches
the front controller. No `.php` file under `public/` is served; only `entry` runs.
A FastCGI route is its own executor: no `app`, and `[route.ingress]` applies to it.

| Key | What it does |
|---|---|
| `entry` | required; the front controller, relative to `blastit.toml` (under `root` when both are set) |
| `listen` | required; the pool's Unix socket, relative to `blastit.toml` |
| `workers` | default `4`; requests at once; `pm.max_children` must be at least this |

A request with a body (POST, PUT, PATCH) needs `[route.ingress]` (`form` or `json`;
`raw` also needs `allow_raw`), or it is a `413`.

## Local

Add the CLI (no `pyproject.toml` yet); run the pool and Blastit from the project
root. No `sudo`, no `/etc`:

```console
$ uv init --bare
$ uv add blastit
```

```ini title="php-fpm.conf"
[global]
error_log = /dev/stderr
[www]
listen = /home/you/app/run/fpm.sock
listen.mode = 0600
pm = static
pm.max_children = 4
security.limit_extensions = .php
catch_workers_output = yes
```

```console
$ mkdir -p /home/you/app/run
$ php-fpm -F -y php-fpm.conf
```

`-F` keeps it in the foreground; `-y` names your pool. The pool's `listen` is absolute;
the manifest's `listen` is relative to `blastit.toml`.

```console
$ uv run blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
```

```console
$ curl -i http://127.0.0.1:8080/users/42
HTTP/1.1 200 OK
Content-Type: application/json

{"route":"user.show","id":"42"}
```

`immutable` values are served-path globs (`/assets/*` for `public/assets/`), matched
whole: point each at fingerprinted output only — never a path that can also answer HTML
or an unhashed original:

```console
$ curl -i http://127.0.0.1:8080/assets/app.4f1c2a9b7d3e.css
HTTP/1.1 200 OK
Content-Type: text/css; charset=utf-8
Cache-Control: public, max-age=31536000, immutable
```

A query string is data, never PHP options. The home route echoes what PHP saw:

```console
$ curl -i 'http://127.0.0.1:8080/?-d+allow_url_include=1'
HTTP/1.1 200 OK
Content-Type: application/json

{"route":"home","allow_url_include":"0","query_string":"-d+allow_url_include=1"}
```

A hostile FastCGI param is refused, not forwarded; `SCRIPT_FILENAME` is pinned to
`entry`. Mechanisms in [security](../security.md) and [how it works](../how-it-works.md).

Stop the first `dev` with Ctrl-C, remove `[route.static]`, and start:

```console
$ uv run blastit dev --learn
```

Request `/assets/app.4f1c2a9b7d3e.css`: learn returns the front controller's `404` and
records the path. Ctrl-C writes `[route.static] root = "public"`; add `immutable`.

## CI/CD

Your CI image has PHP and Composer; start the pool as in Local, then one command
serves the app, runs your suite and audits every request and query it saw:

```console
$ uv run blastit check -- vendor/bin/phpunit
blastit check: 24 requests, 0 statements, 1/1 routes, clean
```

`check` puts the front controller on loopback as `BLASTIT_URL=http://127.0.0.1:8080`
and runs the command after `--`: for a FastCGI manifest that is the suite, not pytest.
The suite reads `BLASTIT_URL`, never the app; a test that does
`file_get_contents(getenv('BLASTIT_URL').'/users/42')` speaks HTTP; an in-process test
sends nothing. A route is exercised when its front controller ran, not by an asset;
a route nothing requested, or a body route with no input rule, is a finding (`1`).
If the suite only asked for assets, `--json` names the route:

```console
$ uv run blastit check --json -- vendor/bin/phpunit
```

```json
{"schema_version":1,"ok":false,"requests":3,"statements":0,"routes":{"total":1,"exercised":0},"proxies":{},"findings":[{"kind":"unexercised_route","severity":"error","route":"app","fix":{"action":"add_test"}}]}
```

[CI/CD](../ci.md#what-it-fails-on) owns the full list and [exit codes](../ci.md#exit-codes).

## Deployments

```console
$ blastit --preset compose-traefik
```

In the image, one pool per route runs as that route's uid, so `0600` stays private.
Match `pm.max_children` to `workers`. Locally the preset exits `1`: production refuses
to run without its capabilities. See [Deployments](../deploy/index.md) ·
[the manifest](../reference/manifest.md) · [static assets](static-and-spa.md).
