# PHP (Symfony, Laravel)

Blastit runs `public/index.php` in its own worker: no PHP server beside it, no rewrite
rules, and the manifest block that says so is short.

## 1. Executor

```toml title="blastit.toml"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "app"
path = ['.*']

[route.php]
entry = "public/index.php"

[route.static]
root = "public"

[route.db]
queries = []

[route.ingress]
body = "form"
```

`entry` is the only file Blastit executes: a `[route.php]` route needs no `app`, and
`.php`/`.phar`/`.phtml` files under the static root are refused with `404`, so keep
other sources out of `public/`.

```console
$ uv tool install blastit
```

Blastit embeds the host's NTS `libphp` — `libphp-embed` on Debian/Ubuntu,
`php85-embed` (or `php84-embed`, `php83-embed`) on Alpine, `php-embedded` on
Fedora/RHEL — or `BLASTIT_LIBPHP` names one. Installing that package
(`apt install libphp-embed php-pgsql`) is the one privileged step for PHP itself;
Blastit never writes `/etc`. Without it the boot stops before it binds and names
the package: it is not a degraded mode.

We support supported PHP versions: 8.3, 8.4 and 8.5.

The boot reads the minor from the distro's own package, so the install is enough. It
understands the dotted names (`libphp8.5.so`, `libphp-8.5.so`), Alpine's packed
`libphp83.so`, and the per-minor directory (`/usr/lib/php85/libphp.so`), and it looks
where each distro installs. A library whose path states one minor and whose own bytes
state another is refused before the first request, with both named.

To use a library the distro did not install, put a root-owned `0644` copy in
`/usr/local/lib` (or `/usr/local/lib/php85`), never `/tmp`: the copy is enough. Set
`BLASTIT_LIBPHP` only if it lives elsewhere — it must be an absolute path.

## 2. Static assets

A file under `public/` is answered by the static responder; every other path reaches the
front controller. Point `immutable` at fingerprinted build output only — `/build/*` for
Encore, `/build/assets/*` for Laravel's Vite.

## 3. Database

`getenv('DATABASE_URL')` is the local socket URL, and `$pdo->prepare()` with
`PDO::ATTR_EMULATE_PREPARES => false` keeps `$1` in the statement text, so one template
is one grant; Laravel's `env('DATABASE_URL')` reads the same variable. A route with no
`[route.db]` gets no listener at all, and nothing in `blastit.toml` holds a password: the
real URL lives in the proxy and connects upstream from Blastit's own process.

## 4. Ingress

The block declares `form` for the front controller. A JSON API is a separate route
**above** the catch-all, with its own `name`, `path = ['/api/.*']`, `[route.php]`, the
`[route.db]` grants it needs and `body = "json"`; an upload is `raw` with
`allow_raw = true` on its own route, never on the catch-all.

## 5. Environment and secrets

The front controller reads `DATABASE_URL` and looks like any other app. Keep `APP_KEY`,
mail and session secrets in the platform's environment, and do not ship a `.env` into
the static root.

## 6. Health

No route in `routes.yaml` or `routes/web.php` sees `/_blastit/health`: the scheduler
answers it before the front controller runs.

## 7. Migrations

Run `php bin/console doctrine:migrations:migrate` or `php artisan migrate` outside blastit,
before the new image boots.

## 8. Learn and check

```console
$ blastit dev --learn
```

Click through the app: `--learn` records the routes and exact queries and writes them in
`blastit.toml` on exit. `blastit check` runs `pytest -q` ([CI/CD](../ci.md)), so a PHP
manifest has no request audit in 0.2.0: it validates the manifest, the static roots and
the libphp gate, exits `2` without pytest, and has nothing to observe if pytest is there.

## 9. Deployment notes

```console
$ blastit --preset compose-traefik
```

The image must carry the same NTS `libphp`, and the first route's uid `21000` needs write
access to Symfony's `var/` or Laravel's `storage/` and `bootstrap/cache/`. `workers`
(1–64) may only lower the concurrency Blastit derives for the route
([Deployments](../deploy/index.md)).
