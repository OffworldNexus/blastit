# Frameworks

Blastit starts your app through one of three interfaces, and all three share the same `Blastfile`,
the same trust boundary and the same CLI. Pick the interface, then read only what your framework
changes.

```mermaid
flowchart LR
    client(["client"]) --> ingress["ingress"]
    ingress --> sched["scheduler"]
    sched --> py["view · python<br/>WSGI callable"]
    sched --> php["view · php<br/>public/index.php"]
    sched --> mounts["static mount<br/>scheduler's uid"]
    py -- "socket" --> dbproxy["dbproxy"]
    php -- "socket" --> dbproxy
    dbproxy --> db[("PostgreSQL")]
```

`entrypoint` is declared once, at the top level of the file, and every view below it is identical:
its own process, its own uid, its own grants. Two things change per runtime — what the entrypoint
names and which files it may read.

## Pick your runtime

| Interface | What the file says | Install | Pages |
|---|---|---|---|
| **Python (WSGI)** | `entrypoint python "module:attr"` | `blastit` + one of the CPython minors in the [install table](../install.md#one-package-per-language-version) | [Connector](python/index.md) · [Django](python/django.md) · [Flask, SQLAlchemy and raw WSGI](python/flask.md) |
| **PHP (PSR-7/PSR-15, front controller)** | `entrypoint php "public/index.php"`, on the host's NTS `libphp` (one shim per minor the [install table](../install.md#one-package-per-language-version) lists) | `blastit` + `libphp` | [Connector](php/index.md) · [Symfony](php/symfony.md) · [Laravel](php/laravel.md) |
| **Static & SPA** | `static "/"` with a `root`, and no `entrypoint` | `blastit` | [Static sites and SPAs](static-and-spa.md) |

## What each interface answers

| Capability | Python (WSGI) | PHP (PSR-7/PSR-15) | Static & SPA |
|---|---|---|---|
| What Blastit starts | the WSGI callable `entrypoint python` names | the front controller, inside the host's embedded NTS `libphp` | nothing: the scheduler reads files |
| Routing | your app's own WSGI routing | the front controller's router | exact file, a directory's `index`, then `fallback` |
| Static assets | `static "/static"` with `root "staticfiles"`, beside the views | `static "/"` with `root "public"`; `.php` sources are `404` | the mount is the whole app |
| PostgreSQL | `postgres $DATABASE_URL`, grants per view | the same store, or a split `DB_*` block | none — a mount reads no store |
| A request body | `body json`, `form`, `none` or `raw unvalidated` on the view | the same directive on the view | a `GET` carrying a body is `413` |
| Filling the grants | `blastit check --learn` | `blastit dev --learn`, then `blastit fmt` | nothing to learn |
| The CI gate | `blastit check` runs `pytest` and the audit | `blastit fmt --check` plus an HTTP smoke run | `blastit check` audits every `root` |
| Production | `blastit --preset …` under real uids | `blastit --preset …` | `blastit --preset …` |

!!! note "`--learn` covers what the audit cannot"

    Python's `check` observes real requests and closes the gaps itself. For PHP it is one
    `blastit dev --learn` pass that writes the views and their exact `query` lines: send every
    route, stop with ++ctrl+c++, then read `git diff` ([the workflow](php/index.md#learn-the-grants)).

## Where next

* [Python (WSGI)](python/index.md) · [PHP](php/index.md) · [Static & SPA](static-and-spa.md)
* [Deployments](../deploy/index.md) · [CI/CD](../ci.md) · [The Blastfile](../blastfile.md)
