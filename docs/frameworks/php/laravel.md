# Laravel

A Laravel app is a front controller plus caches and a Vite build made before the image starts; the
[connector](index.md) has the host install and the gate.

## The Blastfile

```text title="Blastfile"
app "shop"

entrypoint php "public/index.php"

postgres {
    host $DB_HOST
    port $DB_PORT
    user $DB_USERNAME
    password $DB_PASSWORD
    database $DB_DATABASE
}

static "/" {
    root "public"
    immutable "/build/assets/*"
}

view app {
    GET, DELETE, OPTIONS "/*"
    POST "/*" { body form }
    PUT, PATCH "/*" { body json }

    postgres {}
}
```

The split store is Laravel's `DB_*` wiring, and `immutable "/build/assets/*"` is Vite's hashed
output; `*` stops at `/`, so this one pattern covers the files under `assets/`. [The
connector](index.md) has the store slots, and [the reference](../../reference/blastfile.md#stores)
has the directives.

## Bind parameters in PDO

Set `PDO::ATTR_EMULATE_PREPARES => false` in `config/database.php`; the default emulates prepares.
An emulated prepare inlines the values, and an inlined statement cannot be learned.

## Build the caches

`npm ci && npm run build`, `php artisan route:cache` and `php artisan view:cache` run at build time.
Leave `config:cache` off: it snapshots the build's environment, while Blastit renders `DB_*` inside
each view worker. `chown -R 21000:21000 storage bootstrap/cache` in the image gives the view that
runs the app the directories it writes.

## Trusted proxies

Listing the edge in `TrustProxies` is what makes `$request->ip()` and `secure` URLs follow
`X-Forwarded-*`.

## Migrations

`php artisan migrate --force` applies them non-interactively, because Laravel prompts before it
migrates in production.

## Where next

[Symfony](symfony.md).
