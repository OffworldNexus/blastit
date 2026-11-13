# Result MA.3.10 — Harden static deploy checks, wheel provenance and offline/HOME isolation

STATUS: done

Attempt 2. All five review findings are fixed; the psql flag table is now the real
`psql --help` of the version the docs target and is re-checked by the gate itself.

Files (only the node's `paths`):
- `python/tests/runnable_docs/harness.py`
- `docs/testing/README.md`

## Review findings -> what changed

| Finding | Fix |
|---|---|
| **[MAJOR]** `PSQL` lists `-C`, `-g` (and `--on-error-stop`), which psql rejects, so `psql -C /nope x` and `psql -g postgres://x` passed | Table **regenerated** from `psql --help`: PG18 is what every deploy recipe names (`postgres:18`, `docs/local.md`/`docs/deploy/*`), and `src/bin/psql/startup.c` on `REL_18_STABLE` has the identical `getopt_long` list and `"aAbc:d:eEf:F:h:HlL:no:p:P:qR:sStT:U:v:VwWxXz?01"` option string, so the 17.11 client on this host and the 18 image agree. `-C`, `-g`, `--on-error-stop` deleted, the real `-l`/`--list` added. The table is now **exactly** the printed set (70 flags; `psql --help` prints 70, the extra one being `-?`, which the probe's token pattern treats as an alias of `--help`) — no printed flag is missing, no table flag is unprinted |
| **[MAJOR]** no probe, so the table could drift again | `FLAG_AUDIT` + `assert_flag_tables()`: every table is compared with the installed binary's own `--help`; a flag the text does not print must be *accepted* by the binary (one `<tool> <flag> --help` run, `DOCKER_HOST` pointed at a socket that cannot exist, so no daemon is ever reached and nothing is started). The gate calls it **before the wheel** for the tables whose help is complete (`psql`), and `harness.py --audit-flags` audits all 17 tables (2 s) |
| **[MINOR]** `UV_OFFLINE=1` claimed but never set in `base_env` | `base_env()` sets it, so it is one property of the environment instead of three call sites; the redundant `env["UV_OFFLINE"]="1"` in `_build_base`, `_pytest_env` and `mode_install` are gone. Proven by a full run in an **empty network namespace** (below) plus the scrubbed-`HOME` gate |
| **[MINOR]** `documented_snippets` used a column-0 regex and missed indented blocks | It is `fences.scan_page()` now (already the page scanner): a block indented under a list item is found and dedented. `docs/local.md`'s indented `toml` (line 126) and `docs/frameworks/django.md`'s (line 90) are parsed; the private `_BLOCK` regex is deleted |
| **[MINOR]** README:13 claimed every console fence "is executed" | "executed or accounted with a written reason, and every `yaml`/`ini`/`toml`/`sql` snippet a reader copies is parsed — including the ones indented inside a list item or an admonition" |
| **[MINOR]** README:43 vs :60-62, and :58 vs `scripts/build-wheel.sh` | "The one **gate** prerequisite … the one command in this section that installs something from the network"; the build paragraph now says `scripts/build-wheel.sh` runs a bare `uvx maturin`, so a cold uvx cache **downloads** maturin and only `MATURIN='uvx --offline maturin'` fails closed |

Found by the new probe while regenerating (same bug class, same file): the
`docker compose` tables held `--paranoid` (compose v1), `docker compose build
--allow/--build-context/--platform/--secret` (buildx flags), `docker compose run
--pull-policy` and `docker compose pull --platform` — all rejected by the installed
compose (`unknown flag`). Deleted; no page uses any of them.

## Acceptance evidence

### `harness.py --audit-flags` (every table vs the installed tools)

```console
$ time uv run --no-project python python/tests/runnable_docs/harness.py --audit-flags
harness: flag tables checked against --help: docker, docker run, docker build, docker push, docker pull, docker logs, docker exec, docker image ls, docker compose, docker compose up, docker compose down, docker compose logs, docker compose build, docker compose run, docker compose ps, docker compose pull, psql

real 0m1.700s        # exit=0
```

(`docker` 29.9.3, `docker compose` v5.7.1, `psql` 17.11; `doctl` is not installed here,
so its 15 command tables are skipped, not silently passed.)

The psql table is complete and exact, not merely valid:

```console
$ uv run --no-project python /tmp/opencode/psql_gap.py
printed but not in the table: []
in the table but not printed: ['-?']
70 table flags, 69 printed flags
```

### Red probes (`uv run --no-project python /tmp/opencode/ma310b_probe.py`, 13 probes)

Pasted with the tool output trimmed for width (`…`); the full stream is
`/tmp/opencode/ma310b_probe.out`.

```console
1b psql table holding -C/-g/--on-error-stop  RED: `psql --on-error-stop` is in the harness's `psql` flag table, but `psql --help` does not print it and `psql --on-error-stop` rejects it. Delete it from the table (a command the page cannot run must fail here) or replace it with the flag the tool really has
1c psql table holding --frobnicate            RED: `psql --frobnicate` is in the harness's `psql` flag table, but … `psql --frobnicate` rejects it
1e the gate itself refuses psql -C            RED: (the same), raised by `Harness([], Report())` before the wheel is looked up
1f `psql -C /nope x` is refused               RED: `psql -C /nope x`: `psql -C` is not a flag `psql` accepts
1g `psql -g postgres://x` is refused          RED: `psql -g postgres://x`: `psql -g` is not a flag `psql` accepts
1a shipped psql table agrees with psql --help ok
1d psql table with the real -l/--list         ok
1h the documented grant command still passes  ok: 1
2a base_env sets UV_OFFLINE=1                 ok: UV_OFFLINE=1 UV_PYTHON_DOWNLOADS=never
3a docs/local.md yields the indented toml     ok: [('console',15),('console',42),('toml',47),…,('toml',126)]
3b docs/frameworks/django.md indented toml    ok: [('console',9),('python',20),('toml',45),('toml',90),…]
3c indented toml with a broken key is parsed  RED: docs/x.md:4: the TOML block does not parse: Expected ']]' …
3d the indented body is dedented              ok: '[[route]\nbroken ='
13 probes, 0 not behaving as required
```

`1f`/`1g` are the review's own command lines, now refused by `_static_command`.

### `make docs-test` twice — identical verdicts, 0 failed

```console
$ make docs-test > run1.log; make docs-test > run2.log
run1: 38 fences: 26 executed, 12 accounted, 0 failed · 23.9s   exit=0
run2: 38 fences: 26 executed, 12 accounted, 0 failed · 22.1s   exit=0
$ diff <(grep -E "^(PASS|FAIL) " run1.log | sed 's/[0-9]\+\.[0-9]\+s//') \
       <(grep -E "^(PASS|FAIL) " run2.log | sed 's/[0-9]\+\.[0-9]\+s//') && echo identical
verdict tables identical
$ grep PASS run1.log | grep digitalocean-paas.md | cut -c1-90
PASS  docs/deploy/digitalocean-paas.md  fence 2  static   … static
PASS  docs/deploy/digitalocean-paas.md  fence 3  static   … static
PASS  docs/deploy/digitalocean-paas.md  fence 4  static   … static   # the psql grant line
```

### Offline: the whole gate inside an empty network namespace

The strongest form of "every `uv` call runs with `UV_OFFLINE=1`": a namespace with
no route anywhere, loopback up, ambient capabilities dropped so `blastit` sees the
same unprivileged uid it sees outside.

```console
$ unshare -Unc --keep-caps sh -c 'ip link set lo up; exec setpriv \
    --bounding-set=-all --inh-caps=-all --ambient-caps=-all \
    uv run --no-project python python/tests/runnable_docs/run.py'
38 fences: 26 executed, 12 accounted, 0 failed · 21.4s
exit=0
$ grep -ciE "download|fetch" netns.log
0
```

### Scratch `HOME`, scrubbed environment (the node's third acceptance line)

```console
$ env -i HOME=/tmp/dh3 PATH="$PATH" uv run --no-project python python/tests/runnable_docs/run.py
38 fences: 26 executed, 12 accounted, 0 failed · 25.3s
exit=0
```

(`/tmp/dh3`'s uv cache was filled by `--warm-cache` in attempt 1; the cold-cache
failure message and `--warm-cache` are unchanged and still documented.)

### `make check`

```console
$ time make check
…
38 fences: 26 executed, 12 accounted, 0 failed · 22.0s
uv run --no-project tools/plan.py validate
plan OK
real 1m1.647s        # exit=0
```

## Design notes a reviewer will ask about

1. **Why `psql` is the table the gate re-checks and the others are audited on
   demand.** psql's `--help` is its *complete* option list (`getopt_long` accepts
   nothing else) and its flag set is identical in 15/16/17/18, so containment is
   exact and cannot be wrong for a host's version. Docker's help hides
   compatibility aliases (`--net`, `--disable-content-trust`, `-h`) and gains or
   loses flags between CLI releases, so a containment failure there would be the
   host's version, not the table. The audit therefore accepts a flag `--help` does
   not print **only if the binary parses it** — the property the static check
   actually promises — and `--audit-flags` is a 2 s command a reviewer can run.
2. **The audit never touches a daemon.** `DOCKER_HOST` points at a socket that
   cannot exist and every probe command ends in `--help`, which the client's flag
   parser honours before connecting; no image is pulled, no container is started.
3. **`_help_flags` treats a non-zero `--help` as "not installed".** On a host
   without the compose plugin `docker compose --help` prints docker's own usage,
   which would otherwise look like a complete flag list and red every compose
   table.
4. **`Snippet.line` is the block's first body line**, so failure messages keep
   naming the line a reader sees after the fence marker (`docs/local.md:126`).

## Follow-ups (out of this node's paths)

- `docs/knowledge/LESSONS.md` deserves: (a) `unshare -rn` leaves loopback down, so
  `ip link set lo up` comes first, and the `-r` mapping makes `blastit` see uid 0
  and refuse production mode; (b) `unshare -Unc --keep-caps` keeps the ambient
  capability set, which makes `blastit` boot in `capabilities` privilege mode and
  trip D6 — drop the sets with `setpriv --bounding-set=-all --inh-caps=-all
  --ambient-caps=-all` before running the gate.
- `COMPOSE_BUILD` is a strict subset of compose v5's help (`--builder`, `--check`,
  `--dry-run`, `-m/--memory`, `--print`, `--provenance`, `--sbom` are missing):
  a page using one of those would be a false red. Not added because no page uses
  `docker compose build` and adding v5-only flags would green a page a v2 host
  cannot run.
- `DOCKER_IMAGE` holds `docker image ls` flags while `_static_docker` accepts them
  after a bare `docker image` (the `ls` is a positional). The audit uses
  `docker image ls --help`, which is the command the table is for; tightening
  `_static_docker` is a separate change.
- `scripts/build-wheel.sh` still runs a bare `uvx maturin`, so the **build** step
  can download maturin (README now says so). `F7`'s build half stays open.
