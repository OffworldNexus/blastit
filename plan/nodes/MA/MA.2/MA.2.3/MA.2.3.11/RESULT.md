# RESULT MA.2.3.11 — R24-redacted live-server denial and pinned dev watermark

STATUS: done

Files changed (both inside `paths`, nothing else touched):

- `python/tests/test_live_server.py` — both live-denied assertions now pin the
  R24-redacted form `SELECT …` and add the negative half, `"SELECT 'denied'" not
  in json.dumps(report)`. The allowed twin (`missing_sql("live-ok") == []`) is
  kept in the first test.
- `python/blastit/pytest_plugin.py` — `_live_server_environment` pins
  `BLASTIT_MEM_HIGH_WATERMARK=100` with a one-line comment naming
  `Scheduler::make_room`.

## Why the old assertions could never pass

The report's `missing[].sql` is built from the `sql.denied` events, and R24
redacts inline literals before they are displayed: `SELECT 'denied'` normalises
to `SELECT …`. The old membership assertion against the raw literal was false in
every mode; with the redaction removed it would instead be the literal that
appears, so the new pair (redacted present **and** raw absent) fails both ways
round. The second assertion is not decorative: the last run's report is
`['SELECT 1', 'SELECT …']`, and `json.dumps(report)` contains no
`SELECT 'denied'` at all.

## Host is above the 85 % watermark (negative control proves the pin matters)

`blastit-sys::mem` counts a cgroup's `memory.current` before `MemTotal −
MemAvailable`, and this session cgroup is at:

```console
$ cat /sys/fs/cgroup/memory.max /sys/fs/cgroup/memory.current
51539607552
45944885248          # 89.15 % of 48 GiB > the 85 % default
$ grep -E 'MemTotal|MemFree|MemAvailable' /proc/meminfo
MemTotal:       50331648 kB
MemFree:         5464228 kB
MemAvailable:   24098797 kB
```

With the same staged plugin set to `BLASTIT_MEM_HIGH_WATERMARK=85` instead, the
route queue-times out exactly as the reviewer described:

```console
$ … pytest python/tests/test_live_server.py::test_a_live_server_is_served_by_blastit -q
1 failed in 21.62s
# live server log:
{"event":"request","method":"GET","path":"/live/","outcome":"queue_timeout",
 "queued_ms":10001.106125,"status":503,"route":"live-ok",…}
```

Restored to `100`, the same test passes. So this host satisfies the "above the
85 % default" condition the node asks about; the pin is what makes the tier
host-independent.

## Acceptance evidence

Test harness note: `/home/remy` is mode `0700`, so a view uid cannot reach the
uv-managed interpreter there (the live server logs
`/home/remy (mode 0700) is not traversable by uid 21000` and the view dies). For
this host I copied CPython 3.14.10 to `/tmp/opencode/py314` (`chmod -R o+rX`),
made a throwaway venv, installed the CI-version packages from the offline cache
(`pytest 9.2.0`, `psycopg 3.3.9`, `django 6.1.3`, `pytest-django 4.14.0`,
`pytest-xdist 3.8.0`), and staged `python/blastit` plus the prebuilt
`target/python-critical/blastit/_blastit.cpython-314-*.so` and
`_bin/blastit-dbproxy`. No `cargo`, `make`, or `make check` was run (the target
lock belongs to the concurrent Rust node). Throwaway `postgres:15-alpine` on
`127.0.0.1:55437`.

Acceptance command, with `--python` added only because of the interpreter
reachability above:

```console
$ DATABASE_URL='postgresql://blastit:ci-not-secret@127.0.0.1:55437/blastit' \
  uv run --no-project --python /tmp/opencode/m2311v/bin/python \
    --with pytest --with psycopg --offline \
    pytest python/tests/test_live_server.py -q
.....                                                                    [100%]
5 passed in 6.56s
# final rerun after a message-only comment tweak:
# .....                                                                    [100%]
# 5 passed in 6.06s
```

Same file on the venv interpreter (second run, after restoring the pin):

```console
$ DATABASE_URL=… /tmp/opencode/m2311v/bin/python -m pytest python/tests/test_live_server.py -q
.....                                                                    [100%]
5 passed in 7.03s
```

No `skip`/`xfail`; all five tests ran (`3 requests`, the packaged project, the
project's own `live_server`, the `--blastit-live-server` override, and the
usage error).

No-regression suites:

```console
$ DATABASE_URL=… /tmp/opencode/m2311v/bin/python -m pytest python/tests/test_pytest_plugin.py -q
.......................................                                  [100%]
39 passed in 26.56s

$ DATABASE_URL=… /tmp/opencode/m2311v/bin/python -m pytest python/tests/test_check.py -q
..........                                                               [100%]
10 passed in 9.66s
```

Report contents from the accepted run, proving the R24 pair:

```console
$ /tmp/opencode/m2311v/bin/python - "$R" <<'PY'
# report = json.load(open(sys.argv[1])); print missing[].sql per block
PY
live-denied db ['SELECT 1', 'SELECT …']
raw literal present: False
```

Lint/plan:

```console
$ uvx --offline ruff check python/tests/test_live_server.py python/blastit/pytest_plugin.py
All checks passed!
$ uvx --offline ruff format --check python/tests/test_live_server.py python/blastit/pytest_plugin.py
2 files already formatted
$ uv run --no-project tools/plan.py validate
plan OK
```

## Follow-ups (out of scope here)

1. **The 85 % default is a DX trap on loaded shared hosts** (node `# Notes`
   suggested a `MA.3` docs item): a developer whose laptop/CI runner sits above
   it sees every live-server route as a 10 s `503 queue_timeout`, with no hint
   that a sizing knob caused it. Worth documenting in `docs/deploy/` /
   `docs/reference/settings.md` together with the `BLASTIT_MEM_HIGH_WATERMARK`
   override, or surfacing a boot warning when the first spawn is refused for
   memory.
2. **Live-server tests need a view-uid-readable interpreter.** On hosts with a
   `0700` home, the uv-managed Python is unreachable by view uids; the fixture
   could detect that (it already warns at boot) or document the
   `UV_PYTHON_INSTALL_DIR=/opt/uv-python` workaround in the test docstring.
   `docs/knowledge/LESSONS.md` is outside this node's `paths`, so this is
   recorded here for whoever owns that file.
