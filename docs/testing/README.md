# Testing in this repository

The automated tiers are `cargo test` / `pytest` for code, and the
runnable-docs gate for everything a page promises.

## Test curation audits

Fresh-context test-suite audits (`AUDIT-<scope>-<YYYY-MM-DD>.md`) written by `test-curator` at
epic and milestone gates (skill `testing`).

## Runnable-docs gate (`make docs-test`)

Every `console` fence in `README.md` and the public `docs/**` pages is executed or
accounted with a written reason, and every `yaml`/`ini`/`toml`/`sql` snippet a
reader copies is parsed — including the ones indented inside a list item or an
admonition. `python/tests/runnable_docs/` holds the harness, `inventory.toml` maps
each fence to *how* it runs (and why, when it is accounted instead).

```console
$ make docs-test                 # build the wheel, then run all 38 fences
$ make check                     # everything, with docs-test last-but-one
```

### What the gate refuses

| A page shows | The gate does |
|---|---|
| a `console` fence with lines but no `$ ` prompt | fails: the mode would run nothing and report a pass. Restore the prompt, or declare the fence `example`/`static` with a written `reason` |
| an accounted fence (`static`, `example`, `install-external`, `pgstub`) with no `reason` | fails: a mode that runs nothing must say why in `inventory.toml` |
| a deploy command with an unknown subcommand, flag or `-f`/`--file`/`--spec`/`--manifest` file | fails: `docker`, `docker compose`, `psql` and `doctl` are checked against their real flag tables, and a workspace-relative path must exist or be a file the page itself shows (`yaml title="app.yaml"`). `blastit.toml` is the one file a reader always writes themselves |
| a flag in that table the tool does not accept | fails before anything is built: `psql`'s table is compared with `psql --help` on every run, and `harness.py --audit-flags` re-derives every table from the installed `docker`/`docker compose`/`psql` (`psql -C` was in the table and in no psql release) |
| an absolute path outside this repository (`-f /nope/compose.yaml`) | left unchecked (it is the reader's host), and the fence's verdict line says how many were left |
| a `yaml`, `ini`, `toml`, `sql` block that does not parse | fails, naming the page, the block's first line and the broken line: YAML by structure (indentation, `key: value`/`- item`, flow collections, block scalars — not a full parser), `ini` by `configparser` or as a dotenv file, `toml` by `tomllib`, `sql` by non-emptiness, no `…` and a terminating `;` |
| a `{`-line in a `static` fence that is not JSON, or a boot line missing `event`/`stage`/`seccomp`/`privilege`/`bounding_set` | fails (see `docs/reference/log-events.md`) |
| a wheel in `dist/` that is not the workspace version | fails: `wheel_path()` reads `[workspace.package] version` from `Cargo.toml` and needs exactly one `dist/blastit-<version>-*.whl` for the host's CPython |
| `Cargo.toml` and the newest release `CHANGELOG.md` announces disagreeing | fails by name, before anything is built |

### Offline and `HOME` contract

The gate never reaches the network: `base_env` sets `UV_OFFLINE=1` for every `uv`
call and every fence, so an install that would need the index fails instead of
downloading; the wheel comes from `dist/`, and the only pinned dependency
(`pytest==9.2.0`, for the `blastit check` fences) is read from a uv cache that
already holds it. `HOME` is otherwise unused — the fences get their own throwaway
`HOME`, and a CPython the uv cache does not manage is taken from `PATH`.

**The one gate prerequisite**, once per machine — the one command in this section
that installs something from the network:

```console
$ uv run --no-project python python/tests/runnable_docs/harness.py --warm-cache
harness: /home/you/.cache/uv now holds pytest==9.2.0
```

A cache that is already warm elsewhere can be handed over instead:
`BLASTIT_DOCS_UV_CACHE=/path/to/uv-cache make docs-test`. A cold cache is reported
by the harness in its first second — before it installs anything — with that
command in the message (`make docs-test`'s wheel build still runs first).

`make docs-test` also builds the wheel first, which needs `cargo`/`rustup` on
`PATH` (with `RUSTUP_HOME`/`CARGO_HOME` pointing at the toolchain when `HOME` is
scrubbed) and `maturin`. `scripts/build-wheel.sh` finds it with a bare
`uvx maturin`, so a `HOME` whose uvx cache is cold **downloads** it on the first
build, and only `MATURIN='uvx --offline maturin'` makes the build fail closed
instead. Those are build costs, not gate costs: the build may use the network,
nothing in `python/tests/runnable_docs/` does.

```console
# the gate on its own, with a HOME that never built anything:
$ env -i HOME=/tmp/dh PATH="$PATH" \
    uv run --no-project python python/tests/runnable_docs/run.py
harness: pytest==9.2.0 is not in the uv cache this gate reads (/tmp/dh/.cache/uv), …
# the one prerequisite, with the same HOME:
$ env -i HOME=/tmp/dh PATH="$PATH" \
    uv run --no-project python python/tests/runnable_docs/harness.py --warm-cache
harness: /tmp/dh/.cache/uv now holds pytest==9.2.0
$ env -i HOME=/tmp/dh PATH="$PATH" \
    uv run --no-project python python/tests/runnable_docs/run.py
38 fences: 26 executed, 12 accounted, 0 failed
# and the whole target, with the build toolchain where rustup expects it:
$ env -i HOME=/tmp/dh RUSTUP_HOME=… CARGO_HOME=… PATH=… make docs-test
```

`env -i HOME=… make docs-test` without `RUSTUP_HOME`/`CARGO_HOME` stops in
`scripts/build-wheel.sh` ("rustup could not choose a version of cargo"), before
the gate runs at all.

### Troubleshooting

| Message | Fix |
|---|---|
| `pytest==9.2.0 is not in the uv cache this gate reads` | run `--warm-cache` above, with the same `HOME`, or set `BLASTIT_DOCS_UV_CACHE` |
| `has no blastit-<version> wheel` / `holds N wheels for version` | `rm -r dist && scripts/build-wheel.sh`, or delete the wheels this run did not build (`dist/` is generated) |
| `Cargo.toml says the workspace version is X, but … CHANGELOG.md … Y` | bump both (`scripts/dev-version.sh`) or add the changelog entry |
| `no CPython 3.X to install …` | `uv python install 3.X` |
| `names a file that neither exists in the workspace nor is a file the page shows` | fix the path, or add `yaml title="<that file>"` to the block the page shows |
| `is not a flag … accepts` | check the real `--help`; extend the harness's flag table only if the tool really has it, then run `harness.py --audit-flags` |
| `is in the harness's <tool> flag table, but … rejects it` | the table drifted: run `harness.py --audit-flags`, delete the flag no release accepts (or replace it with the one `--help` prints), and re-run the gate |
