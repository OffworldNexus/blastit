# CI/CD

Blastit runs on **Linux** in the runner too; nothing **your** suite exercised is left out of `./Blastfile`.

```console
$ blastit check
$ blastit check --json
$ blastit check --learn --dry-run
```

```mermaid
sequenceDiagram
    participant suite as your suite / curl
    participant check as blastit check
    participant proxy as dbproxy
    suite->>check: one request
    check->>proxy: the app's statement
    proxy-->>check: allowed, or SQLSTATE 42501
    check-->>suite: exit 0 clean · 1 findings
```

One command per runtime below; each job is a copy-paste workflow.

## The workflow per runtime

=== "Python (WSGI)"
    ```yaml title=".github/workflows/check.yml"
    name: check
    on: [push, pull_request]
    jobs:
      blastit:
        runs-on: ubuntu-24.04
        services:
          postgres:
            image: postgres:18
            env: {POSTGRES_PASSWORD: postgres, POSTGRES_DB: shop}
            ports: ["5432:5432"]
            options: >-
              --health-cmd pg_isready --health-interval 5s --health-retries 20
        steps:
          - uses: actions/checkout@v5
          - uses: astral-sh/setup-uv@v10.3.2
          - run: uv sync --frozen
          - run: blastit check --json
            env:
              DATABASE_URL: postgresql://postgres:postgres@127.0.0.1:5432/shop
    ```

    The runner installs Blastit once (`curl -fsSL …/install.sh | sh`), and `blastit` then hands
    `check` to the `.venv/bin/blastit` this project pinned. `check` puts Blastit in front of your
    store and runs `pytest -q` itself (`uv add --dev pytest` if the project has none); a clean run
    means every view was observed.

=== "PHP"
    ```yaml title=".github/workflows/check.yml"
    name: check
    on: [push, pull_request]
    jobs:
      blastit:
        runs-on: ubuntu-24.04
        steps:
          - uses: actions/checkout@v5
          - run: sudo apt-get update && sudo apt-get install -y libphp-embed php-pgsql
          - run: curl -fsSLO https://github.com/OffworldNexus/blastit/releases/latest/download/blastit-x86_64-unknown-linux-gnu.tar.gz
          - run: tar -xzf blastit-*.tar.gz && echo "$PWD/blastit-x86_64-unknown-linux-gnu" >> "$GITHUB_PATH"
          - run: blastit fmt --check
          - run: |
              blastit dev &
              for i in $(seq 30); do curl -fsS localhost:8080/ && exit 0; sleep 0.5; done; exit 1
    ```

    `fmt --check` is the gate: the file parses and is canonical. `check` parses it and
    audits every `root`, then exits `2` without a pytest suite, so the HTTP smoke run is
    what proves a PHP app answers. Install the host's NTS `libphp` first.

=== "Static & SPA"
    ```yaml title=".github/workflows/check.yml"
    name: check
    on: [push, pull_request]
    jobs:
      blastit:
        runs-on: ubuntu-24.04
        steps:
          - uses: actions/checkout@v5
          - run: npx vite build --outDir public
          - run: curl -fsSLO https://github.com/OffworldNexus/blastit/releases/latest/download/blastit-x86_64-unknown-linux-gnu.tar.gz
          - run: tar -xzf blastit-*.tar.gz && echo "$PWD/blastit-x86_64-unknown-linux-gnu" >> "$GITHUB_PATH"
          - run: blastit check
    ```

## What it fails on

A `query` that does not parse is not a finding: the file does not load, so `check`
exits `2` with one `line:col` per problem on stderr and an empty stdout — a refusal.

| Finding | Meaning | Fix |
|---|---|---|
| `unrouted_request` | a path no view matches | add a view, or widen a pattern |
| `missing_grant` | a statement no grant allows | add `query '<sql>'` to that view's `postgres` block |
| `dead_grant` | a grant the suite never used | cover that path, or remove the grant |
| `unexercised_route` | a view the suite never hit | add a test |
| `missing_ingress` | a view took a body and declares no `body` | declare the codec; `raw` also needs `unvalidated` |
| `raw_ingress` | a `body raw` view with no `unvalidated` | add `unvalidated`, or use a codec — the production boot refuses the file |
| `ingress_mismatch` | the declared codec is not what the suite sent | fix the `body` directive |
| `no_observations` | the suite reached no view or no store | check the test database and `DATABASE_URL` |
| `proxy_unused` | a declared store saw zero statements | fix the suite's URL, or remove the store |

!!! note "Production enforces what the audit reads"

    An all-static file audits every `root` and exits `0`, or `2` naming the root it cannot
    read. A body to a view with no `body` is a `413` in production, and an unacknowledged
    `raw` view refuses the boot before it binds a listener. Session setup (`SET`, `SHOW`,
    `set_config(…)`) belongs in a `connection` trait every view `use`s.

## Bootstrap an existing project

`check --learn` discovers unrouted views *and the exact `query` lines the audit
reports missing on existing views*; `entrypoint`, `preload` and the store's
variable names are yours to write (`--dry-run` writes nothing).

## For agents: `--json`

One document on stdout and nothing else: pytest output and `learn:` lines go to stderr,
exit `2` leaves stdout empty, and a finding's `fix.to` is the `Blastfile` node it edits
(`view catalog postgres`, `trait connection`) — every kind is `error` in 0.2.0, so any
finding fails the run. Exit `0` clean, `1` findings, `2` could not start, `3` suite failed.

## Where next

* [Deployments](deploy/index.md) — the same file under real uids and capabilities.
* [Blastfile reference](reference/blastfile.md) — every directive the audit reads.
* [Local development](local.md) — fill the grants before CI sees them.
