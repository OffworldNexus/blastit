# Symfony

A Symfony app is a front controller plus a cache built before the first request; start with the
[PHP connector](index.md).

```mermaid
flowchart LR
    client(["client"]) --> ingress["ingress"]
    ingress --> sched["scheduler"]
    sched -. "a file under public/" .-> static["static mount"]
    sched --> view["view app<br/>libphp, one request"]
    view -- "socket" --> dbproxy["dbproxy"]
    dbproxy --> db[("PostgreSQL")]
```

## The Blastfile

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
    PUT, PATCH "/*" { body json }

    postgres {
        query 'SELECT id, name, price FROM products WHERE id = $?'
        insert orders(customer_id, total)
    }
}
```

`postgres $DATABASE_URL` is the store Doctrine reads, and `static "/" { root "public" }` serves the
front controller's own directory ([the connector](index.md)).

## Warm the cache at build time

Build with `APP_ENV=prod` and a `bin/console cache:warmup` step into `var/cache`, then set the same
`APP_ENV=prod` and `APP_DEBUG=0` in the app's runtime environment — the warmed cache is only used
when the front controller sees that environment. `chown -R 21000:21000 var/` in the image gives the
view that runs the app the cache and logs it writes.

## Doctrine reads DATABASE_URL

Doctrine DBAL reads `$DATABASE_URL` and prepares statements server-side by default, so a statement's
text is stable without a switch. [The grant directives](../../reference/blastfile.md#grants) are
what the view's `postgres` block takes.

## Trusted proxies

`framework.trusted_proxies` is what makes `X-Forwarded-*` believed and generated URLs use `https`.

## Migrations

`bin/console doctrine:migrations:migrate --no-interaction` applies the migrations the image shipped.

## Where next

[Laravel](laravel.md).
