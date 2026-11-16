# PHP (Symfony, Laravel)

Blastit runs `public/index.php` in its own worker: no PHP server beside it, no rewrite
rules, and the block that says so is short.

## 1. Executor

```text title="Blastfile"
app "shop"

entrypoint php "public/index.php"

postgres $DATABASE_URL

static "/" {
    root "public"
}

view app {
    GET, DELETE, OPTIONS "/*"
    POST "/*" { body form }

    postgres {}
}
```

`entrypoint php` names the only file Blastit executes: a PHP app needs no Python
callable, and `.php`/`.phar`/`.phtml` files under the static root are refused with `404`,
so keep other sources out of `public/`. A regular file under `public` is served first;
every other path reaches the front controller.

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
is one grant; Laravel's `env('DATABASE_URL')` reads the same variable. A view with no
`postgres` block gets no listener at all, and a `Blastfile` holds no password: the
real URL lives in the proxy and connects upstream from Blastit's own process.

## 4. Ingress

The catch-all declares `form`. A JSON API is a separate view **above** it, with its own
name, a `"/api/*"` pattern, the `postgres` grant it needs and `body json`; an upload is
`body raw unvalidated` on its own view, never on the catch-all.

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

Click through the app: `--learn` records the views and exact query lines and writes them
in the `Blastfile` on exit. `blastit check` runs `pytest -q` ([CI/CD](../ci.md)), so a PHP
file has no request audit in 0.2.0: it validates the file, the static roots and
the libphp gate, exits `2` without pytest, and has nothing to observe if pytest is there.

## 9. Deployment notes

```console
$ blastit --preset compose-traefik
```

The image must carry the same NTS `libphp`, and the first view's uid `21000` needs write
access to Symfony's `var/` or Laravel's `storage/` and `bootstrap/cache/`.
`BLASTIT_MAX_WORKERS` caps concurrency for the app ([Deployments](../deploy/index.md)).
