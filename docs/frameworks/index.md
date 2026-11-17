# Frameworks

Blastit starts your app and calls it: pick the runtime you deploy, and read only what your
framework changes.

## Pick your runtime

| Runtime | What you write | Pages |
|---|---|---|
| **[Python (WSGI)](python/index.md)** | `entrypoint python "module:attr"` | [Connector](python/index.md) · [Django](python/django.md) · [Flask, SQLAlchemy and raw WSGI](python/flask.md) |
| **[PHP (embedded `libphp`)](php/index.md)** | `entrypoint php "public/index.php"` | [Connector](php/index.md) · [Symfony](php/symfony.md) · [Laravel](php/laravel.md) |
| **[Static sites and SPAs](static-and-spa.md)** | `static "/" { root "public" }` | [Static and SPAs](static-and-spa.md) |

## Nine things to decide

Every runtime answers these nine things. Each answer has one home — or two where filling the file
and proving it are different jobs — and a page below adds only where its framework changes the
answer.

1. How does Blastit start my code? ([entrypoint](../reference/blastfile.md#sections))
2. Where do the built files live, and what is cached for a year? ([static mounts](../reference/blastfile.md#static-mounts))
3. How does my app reach PostgreSQL without the credential? ([stores](../reference/blastfile.md#stores))
4. How does a body reach my code, and who validates it? ([view directives](../reference/blastfile.md#view-directives))
5. What happens to my environment variables and secrets? ([stores](../reference/blastfile.md#stores), [settings](../reference/settings.md#the-app-and-the-file))
6. What answers the platform health check? ([port and health](../reference/settings.md#port-and-health))
7. Where do schema changes run? ([migrations](../deploy/anything-else.md#migrations))
8. How are the grants filled in ([`--learn`](../reference/blastfile.md#what-learn-writes)) and proven complete ([`--json`](../ci.md#for-agents-json))?
9. Which parts become their own process, and how many run at once? ([privilege layout](../reference/settings.md#privileges-and-the-uid-layout), [threads](../reference/settings.md#admission-and-threads))

A PHP app has no request audit in 0.2.0, so question 8's proof is Python's; [the connector](php/index.md#what-ci-can-prove) names its gate.

## Roadmap

Node and Tomcat join this tree as their connectors land, each with a connector page and one page
per framework in this shape.
