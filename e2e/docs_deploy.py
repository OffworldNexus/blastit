"""The deploy pages' pooler wiring, asserted without Docker.

Both presets now assume a connection pooler: DigitalOcean's managed pool in
`docs/deploy/digitalocean-paas.md`, PgBouncer in `docs/deploy/compose-traefik.md`.
The documented trap is a recipe that says "use a pooler" while its snippet still
points the app at the database, or swaps the two URLs. This reads the pages' own
`compose.yaml` / `app.yaml` blocks, the compose page's `Dockerfile` block, and
the DigitalOcean page's `Blastfile` store block, and asserts the wiring, so the
recipe a developer copies matches what the page claims.

The session zone is the same trap one layer down, and `MB.18` moved it out of the
index: `docs/deploy/index.md` sets the two-URL expectation, and
`docs/deploy/anything-else.md#session-state` owns the procedure (role default, the
direct-port read-back, the `dead_grant`). So the index is also asserted to *not*
carry that material again — the duplication the restructure removed cannot come
back silently.

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
ANYTHING_ELSE = DOCS / "anything-else.md"

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
    dockerfile = block(COMPOSE, "Dockerfile")
    checks.that(
        "ENV PYTHONUNBUFFERED=1 BLASTIT_EXEC=/usr/share/blastit/lib/blastit"
        in dockerfile,
        "compose: the image's `ENV` line exports `BLASTIT_EXEC` for the "
        "capability-free `--role=dbproxy` child",
    )


def digitalocean_checks(checks: Checks) -> None:
    page = DIGITALOCEAN.read_text(encoding="utf-8")
    flat = " ".join(page.split())  # prose wraps; assert on flattened text
    manifest = block(DIGITALOCEAN, "app.yaml")
    jobs, services = manifest.split("services:", 1)

    #: The store's variables and the pool bindable each must be filled from.
    #: Asserted as an exact pair, not as two independent presences: a spec that
    #: put `${db.shop-pool.USERNAME}` in the password slot (or the port in the
    #: host slot) boots and fails on the first login.
    pool = {
        "DATABASE_HOST": "HOSTNAME",
        "DATABASE_PORT": "PORT",
        "DATABASE_USERNAME": "USERNAME",
        "DATABASE_PASSWORD": "PASSWORD",
        "DATABASE_DATABASE": "DATABASE",
    }
    checks.that(
        all(
            f'value: "${{db.shop-pool.{bindable}}}"' in services
            for bindable in pool.values()
        ),
        "digitalocean: every connection value the web service gets is the pool's (25061)",
    )
    # The two-URL rule with the web on the split form: no `${db.…}` connection
    # value may reach a service, because the cluster's own are `doadmin`'s on
    # 25060. (`${db.CA_CERT}` is the one cluster value allowed through.)
    checks.that(
        re.search(
            r"\$\{db\.(DATABASE_URL|HOSTNAME|PORT|USERNAME|PASSWORD|DATABASE)\}",
            services,
        )
        is None,
        "digitalocean: no service reads the cluster's connection values where the pool's belong",
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
    # The page used to describe the CA in prose while the harness grepped the
    # retired TOML key, so the recipe and its guard had stopped meeting. Assert
    # the block the reader writes, every slot's variable as an env the service
    # sets, and the TLS pair (`sslmode` + `ca_cert`) against the values that
    # fill them. A page that dropped the CA step loses the `ca_cert` line and
    # the `DATABASE_CA_CERT` env together, and the retired `=` spelling cannot
    # come back.
    store = block(DIGITALOCEAN, "Blastfile: the store block")
    slots = dict(re.findall(r"^[ \t]+(\w+) \$([A-Z_]+)$", store, re.M))
    wanted = {
        "host": "DATABASE_HOST",
        "port": "DATABASE_PORT",
        "user": "DATABASE_USERNAME",
        "password": "DATABASE_PASSWORD",
        "database": "DATABASE_DATABASE",
        "sslmode": "DATABASE_SSLMODE",
        "ca_cert": "DATABASE_CA_CERT",
    }
    #: Each store slot's variable, and the pool bindable that must fill it. The
    #: pairing is asserted as one line, because a spec that names `USERNAME` for
    #: the password slot is a login that fails closed at the worst moment.
    unpaired = [
        name
        for name, bindable in pool.items()
        if f'key: {name}, scope: RUN_TIME, value: "${{db.shop-pool.{bindable}}}"'
        not in services
    ]
    gaps = []
    if slots != wanted:
        gaps.append(f"block is {slots or 'absent'}")
    if unpaired:
        gaps.append(f"not the pool's own bindable: {', '.join(unpaired)}")
    if "key: DATABASE_SSLMODE, scope: RUN_TIME, value: verify-full" not in services:
        gaps.append("`verify-full` is not the service's `DATABASE_SSLMODE`")
    if 'key: DATABASE_CA_CERT, scope: RUN_TIME, value: "${db.CA_CERT}"' not in services:
        gaps.append("`DATABASE_CA_CERT` is not the cluster's `${db.CA_CERT}`")
    if 'ca_cert = "' in page:
        gaps.append("the retired `ca_cert =` spelling is back")
    checks.that(
        not gaps,
        "digitalocean: the proxy's `ca_cert` and `sslmode` inputs and the service env agree"
        + (f" ({'; '.join(gaps)})" if gaps else ""),
    )
    checks.that(
        'ALLOWED_HOSTS = [os.environ["ALLOWED_HOST"]]' in page,
        "digitalocean: the page tells Django to read the `ALLOWED_HOST` the spec passes down",
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


#: The session-zone material, as `docs/deploy/index.md` used to teach it and as
#: `anything-else.md#session-state` teaches it now. The index sets the expectation
#: and hands off; a page that spells any of this out again is the duplication
#: `MB.18` removed.
_ZONE_MATERIAL = (
    "SHOW TimeZone",
    "set_config",
    "dead_grant",
    "ALTER ROLE",
    "options=-c",
)


def index_checks(checks: Checks) -> None:
    page = INDEX.read_text(encoding="utf-8")
    flat = " ".join(page.split())
    checks.that(
        "only the app's `DATABASE_URL` names the pool" in flat
        and "migrations and grants name the database" in flat,
        "index: the app names the pool, migrations and grants name the database",
    )
    checks.that(
        "anything-else.md#pooler" in page,
        "index: the pooler decision hands off to the page that explains it",
    )
    carried = [needle for needle in _ZONE_MATERIAL if needle in flat]
    checks.that(
        not carried,
        "index: the session-zone procedure is not taught here "
        f"({', '.join(carried) if carried else 'none of setup/read-back/dead_grant found'})",
    )


def anything_else_checks(checks: Checks) -> None:
    page = ANYTHING_ELSE.read_text(encoding="utf-8")
    flat = " ".join(page.split())
    checks.that(
        "{ #session-state }" in page,
        "anything-else: the session-state anchor the other pages link to exists",
    )
    checks.that(
        "SHOW TimeZone" in flat and "must print exactly `UTC`" in flat,
        "anything-else: the session zone is read back, with the exact value it must print",
    )
    # The read-back has to be the app's own role on the database's direct port:
    # a pool's open backends keep the zone they logged in with, and the pool's
    # URI names the pool in its path, so a pool URL would prove nothing.
    read_back = next(
        (line.strip() for line in page.splitlines() if "SHOW TimeZone" in line),
        "",
    )
    checks.that(
        read_back.startswith("$ psql 'postgresql://app:")
        and ":5432/" in read_back
        and "-A -t -c 'SHOW TimeZone'" in read_back
        and "pgbouncer" not in read_back.lower(),
        "anything-else: the read-back runs as `app` on the direct port, never the pool: "
        + (read_back or "(no `SHOW TimeZone` command)"),
    )
    checks.that(
        "ALTER ROLE app SET timezone = 'UTC'" in flat,
        "anything-else: the fix is the role's default, not a per-request `SET`",
    )
    checks.that(
        "options=-c timezone=UTC" in flat and "refused" in flat,
        "anything-else: the URL route to a session setting is documented as refused",
    )
    checks.that(
        "Restart the pooler" in flat,
        "anything-else: a live role change says the pool must be restarted",
    )
    checks.that(
        "dead_grant" in flat and "../ci.md#what-it-fails-on" in page,
        "anything-else: the `UTC` database's group grant is a `dead_grant`, with the CI link",
    )


def main() -> int:
    checks = Checks()
    index_checks(checks)
    anything_else_checks(checks)
    compose_checks(checks)
    digitalocean_checks(checks)
    failures = len(checks.failures)
    print(
        f"docs_deploy: {checks.total - failures}/{checks.total} documented pooler promises hold"
    )
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
