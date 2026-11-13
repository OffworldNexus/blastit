"""The deploy pages' pooler wiring, asserted without Docker.

Both presets now assume a connection pooler: DigitalOcean's managed pool in
`docs/deploy/digitalocean-paas.md`, PgBouncer in `docs/deploy/compose-traefik.md`.
The documented trap is a recipe that says "use a pooler" while its snippet still
points the app at the database, or swaps the two URLs. This reads the pages' own
`compose.yaml` / `app.yaml` blocks and asserts the wiring, so the recipe a
developer copies matches what the page claims.

It runs first in `e2e/run.py`, before any container starts: no Docker, no
network, ~1 ms. A page edit that breaks the two-URL rule fails the gate at the
line that names the rule.

    uv run --no-project e2e/docs_deploy.py
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

DOCS = Path(__file__).resolve().parents[1] / "docs" / "deploy"
INDEX = DOCS / "index.md"
COMPOSE = DOCS / "compose-traefik.md"
DIGITALOCEAN = DOCS / "digitalocean-paas.md"

#: The files the pages show, by the `title=` of their fenced block.
_BLOCK = re.compile(
    r"^```[a-z]+ title=\"(?P<title>[^\"]+)\"\n(?P<body>.*?)^```$", re.M | re.S
)


class Checks:
    """Collects one line per promise; any failure makes the run red."""

    def __init__(self) -> None:
        self.failures: list[str] = []
        self.total = 0

    def that(self, condition: bool, promise: str) -> None:
        self.total += 1
        if condition:
            print(f"ok    {promise}")
        else:
            print(f"FAIL  {promise}")
            self.failures.append(promise)

    def urls(self, block: str, host_port: str) -> int:
        """Count `@host:port/` occurrences in one snippet."""
        return len(re.findall(re.escape("@" + host_port + "/"), block))


def block(page: Path, title: str) -> str:
    """The body of the page's ```` ```yaml title="…" ```` block."""
    for match in _BLOCK.finditer(page.read_text(encoding="utf-8")):
        if match.group("title") == title:
            return match.group("body")
    raise SystemExit(f"docs_deploy: {page} shows no `{title}` block")


def compose_checks(checks: Checks) -> None:
    page = COMPOSE.read_text(encoding="utf-8")
    compose = block(COMPOSE, "compose.yaml")

    checks.that("  pgbouncer:" in compose, "compose: a `pgbouncer` service exists")
    checks.that(
        re.search(r"image: edoburu/pgbouncer:v\d+\.\d+\.\d+-p\d+", compose) is not None,
        "compose: its image is pinned to a version tag (never `latest`)",
    )
    for setting in (
        "POOL_MODE: transaction",
        "MAX_PREPARED_STATEMENTS",
        "DEFAULT_POOL_SIZE",
        "MAX_CLIENT_CONN",
        "DB_NAME: shop",
    ):
        checks.that(
            setting in compose, f"compose: PgBouncer sets `{setting.split(':')[0]}`"
        )
    checks.that(
        "AUTH_TYPE: scram-sha-256" in compose,
        "compose: PgBouncer authenticates with SCRAM (PostgreSQL 18 passwords)",
    )
    checks.that(
        "healthcheck:" in compose.split("pgbouncer:", 1)[1].split("migrate:", 1)[0],
        "compose: PgBouncer has a healthcheck",
    )
    checks.that(
        "pgbouncer: { condition: service_healthy }" in compose.split("  app:", 1)[1],
        "compose: the app waits for the pool to be healthy",
    )
    checks.that(
        "DB_PASSWORD: ${DB_PASSWORD:?}" in compose,
        "compose: the pool's password comes from the environment",
    )

    # The two-URL rule, counted rather than eyeballed: one pooler URL (the app),
    # two direct ones (`migrate` and the `grant` job, both outside blastit).
    checks.that(
        checks.urls(compose, "pgbouncer:5432") == 1,
        "compose: exactly one URL points at the pool (the app's)",
    )
    checks.that(
        checks.urls(compose, "db:5432") == 2,
        "compose: exactly two URLs point at the database (migrate and grant)",
    )
    app = compose.split("  app:", 1)[1]
    checks.that(
        "DATABASE_URL: postgresql://app:${DB_PASSWORD:?}@pgbouncer:5432/shop"
        in compose,
        "compose: the app's DATABASE_URL names `pgbouncer`, not `db`",
    )
    checks.that(
        "environment: *app-env" in app
        and "@pgbouncer:5432/shop"
        in compose.split("x-app-env:", 1)[1].split("services:", 1)[0],
        "compose: the app service and the shared env both point at the pool",
    )
    checks.that(
        "ALTER ROLE app SET timezone = 'UTC'" in page,
        "compose: the app role's session zone is UTC (grants.sql)",
    )
    checks.that(
        "pg_isready" in page and "service_healthy" in page,
        "compose: the page explains the pool's health gate",
    )
    checks.that(
        "DB_HOST: db" in compose and "DB_USER: app" in compose,
        "compose: PgBouncer's backend is the database as `app`",
    )
    checks.that(
        'title="db/grants.sql"' in page,
        "compose: the grant SQL the `grant` service mounts is on the page",
    )
    env = block(COMPOSE, ".env")
    required = set(re.findall(r"\$\{([A-Z_]+):\?", compose))
    checks.that(
        all(f"{name}=" in env for name in required),
        "compose: every `${...:?}` value is defined in the documented `.env`",
    )
    checks.that(
        "ALLOWED_HOST: shop.example.com" in compose,
        "compose: Django's `ALLOWED_HOST` is the ingress host",
    )


def digitalocean_checks(checks: Checks) -> None:
    page = DIGITALOCEAN.read_text(encoding="utf-8")
    flat = " ".join(page.split())  # prose wraps; assert on flattened text
    manifest = block(DIGITALOCEAN, "app.yaml")
    jobs, services = manifest.split("services:", 1)

    checks.that(
        'value: "${db.shop-pool.DATABASE_URL}"' in services,
        "digitalocean: the web service reads the pool's bindable URL",
    )
    checks.that(
        re.search(
            r'key: DATABASE_URL, scope: RUN_TIME, value: "\$\{db\.DATABASE_URL\}"',
            services,
        )
        is None,
        "digitalocean: no service reads the direct bindable as its DATABASE_URL",
    )
    checks.that(
        re.search(
            r'postgresql://migrator:[^@"]+@[^/"]+:25060/defaultdb\?sslmode=require',
            jobs,
        )
        is not None,
        "digitalocean: the migrate job's URL is the direct port 25060 as `migrator`",
    )
    checks.that(
        "shop-pool" in page and "25061" in page and "25060" in page,
        "digitalocean: the page names the pool, its port (25061) and the direct port (25060)",
    )
    checks.that(
        "name `shop-pool`" in page
        and "shop-pool --db defaultdb --size 18 --user app" in page,
        "digitalocean: the console step and the doctl step name the same pool for `app`",
    )
    checks.that(
        'ca_cert = "DATABASE_CA_CERT"' in page and "key: DATABASE_CA_CERT" in services,
        "digitalocean: the proxy's `ca_cert` input and the service env agree",
    )
    checks.that(
        'value: "${APP_DOMAIN}"' in services or '"${APP_DOMAIN}"' in services,
        "digitalocean: `ALLOWED_HOST` is the platform's domain bindable",
    )
    checks.that(
        "Connection Pools" in page and "doctl databases pool create" in page,
        "digitalocean: one explicit pool step, with its doctl equivalent",
    )
    checks.that(
        "doctl databases user create" in flat and "platform manages" in flat,
        "digitalocean: the pool's user is a platform-managed database user",
    )
    checks.that(
        "ALTER ROLE app SET timezone = 'UTC'" in page,
        "digitalocean: the app role's session zone is UTC",
    )
    checks.that(
        "prepare_threshold=None" in page,
        "digitalocean: the page says what Django does about prepared statements",
    )
    checks.that(
        "<cluster-id>" not in page,
        "digitalocean: no command needs an id the page never shows how to get",
    )
    checks.that(
        "REPLACE_WITH_YOUR_TAG` below is this `$GIT_SHA" in page,
        "digitalocean: the tag placeholder is the `$GIT_SHA` the build pushed",
    )
    checks.that(
        "databases:" in manifest and "cluster_name: myshop-db" in manifest,
        "digitalocean: the spec still attaches the cluster",
    )


def index_checks(checks: Checks) -> None:
    page = INDEX.read_text(encoding="utf-8")
    flat = " ".join(page.split())
    checks.that(
        "SHOW TimeZone" in flat and "It must print exactly `UTC`" in flat,
        "index: the session zone is read on the direct port, not guessed",
    )
    checks.that(
        "export APP_URL=" in flat
        and flat.index("export APP_URL=") < flat.index('psql "$APP_URL"')
        and "export OWNER_URL=" in flat
        and flat.index("export OWNER_URL=") < flat.index('psql "$OWNER_URL"'),
        "index: both URLs are exported before the commands that read them",
    )
    checks.that(
        re.search(r"export APP_URL='postgresql://app:[^']*:25060/defaultdb", flat)
        is not None,
        "index: `APP_URL` is the direct port and the real database, not the pool's URI",
    )
    checks.that(
        'psql "$APP_URL" -A -t -c' in flat and 'psql "$OWNER_URL" -A -t' not in flat,
        "index: the zone is read as `app` on the direct port, never as the owner",
    )
    checks.that(
        "docker compose exec db psql -U app -d shop" in flat,
        "index: the Compose read runs as `app` on the direct service",
    )
    checks.that(
        "Two destinations" in flat,
        "index: the app and the one-off jobs are described as two destinations",
    )


def main() -> int:
    checks = Checks()
    index_checks(checks)
    compose_checks(checks)
    digitalocean_checks(checks)
    failures = len(checks.failures)
    print(
        f"docs_deploy: {checks.total - failures}/{checks.total} documented pooler promises hold"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
