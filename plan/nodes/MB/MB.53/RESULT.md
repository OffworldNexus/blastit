# MB.53 — caller `PYTHONPATH` first, bundle directory second

## What changed

### `crates/blastit/src/check.rs` (the only production change)

1. **New pure helper** beside `bundled_python()`:

   ```rust
   fn pythonpath_with_bundle(existing: &str, directory: &str) -> String {
       if existing.is_empty() {
           directory.to_string()
       } else {
           format!("{existing}:{directory}")
       }
   }
   ```

   No environment access, no `std::env`, no I/O — the ordering is a pure
   function of its two arguments.
2. **`run_pytest` uses it**, so the join is `{caller}:{bundle}` rather than
   `{bundle}:{caller}`. The comment above it now states the real contract:
   Python searches `PYTHONPATH` in order and *before* the interpreter's own
   stdlib/`site-packages`, so the caller's explicit directory keeps its
   precedence (the protected `test_check_contract.py` hands the child
   `project.root/stub` precisely so its `stub/pytest` shadows the installed
   one), while the bundle's package root still shadows what the child
   interpreter finds by itself — which is what makes `-p blastit._check_pytest`
   importable from a wheel/`.deb`/`uv tool` install with no venv.
3. **Unit test** `check::tests::a_callers_pythonpath_precedes_the_bundle_directory`:
   empty caller → the bundle alone; non-empty caller → caller entries `FIRST`,
   bundle last, asserted with an explicit `starts_with("/project/stub:")`.
4. **No duplicate join in the workspace.** `grep -rn 'PYTHONPATH' crates/`
   finds only this site building the bundle join (`stage0/mod.rs`,
   `visibility.rs`, `config/*` only mention `PYTHONPATH` in prose/comments;
   `e2e/universal_binary.py` *sets* `PYTHONPATH` to the bundle root outright,
   with no caller entry to order). Nothing else to keep consistent.

The 12 protected failures recorded by `MB.52` are gone: `test_check_contract.py`
is `22 passed`.

## Proof (wheel rebuilt as CI does; `modelw/base:2026.07` + throwaway `postgres:18`)

### Wheel
```
$ scripts/build-wheel.sh --require-shims py
…
ok   /var/tmp/blastit-wheel.rSNVxv/_bin5: no BLASTIT_TEST_RELOAD_PAUSE (absent, as shipped)
WHEEL_EXIT=0
-rw-r--r-- 1 remy remy 11578546 Nov 20 13:00 dist/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
```

### Database (throwaway, this node only)
```
$ docker run -d --name blastit-mb53-pg -e POSTGRES_USER=blastit \
    -e POSTGRES_PASSWORD=ci-not-secret -e POSTGRES_DB=blastit -p 25432:5432 postgres:18
…
$ docker exec blastit-mb53-pg pg_isready -U blastit
/var/run/postgresql:5432 - accepting connections
```

### Setup inside the image (mirrors `ci.yml`'s `pytest-plugin` steps)
The image's `/usr/local/bin/python` is a standalone CPython 3.14 with **no**
shared `libpython`, so the venv is created from the image's libpython-bearing
CPython 3.14 (`/opt/python/global`, the same interpreter `MB.52` proved with):
a live-server production boot resolves the interpreter *the wheel's console
script ran on*, and a static interpreter can never satisfy it. Everything else
is the job verbatim (`chmod go-w /opt`, `chown` to the runner before filling,
`chown -R root:root` + `chmod -R go-w` after).

```
$ docker run --rm --network host --cap-add=SYS_PTRACE -v "$PWD:$PWD" -w "$PWD" \
    -e DATABASE_URL=postgresql://blastit:ci-not-secret@127.0.0.1:25432/blastit \
    modelw/base:2026.07 sh -euc '
      sudo --non-interactive chmod go-w /opt
      sudo --non-interactive rm -rf /opt/blastit-tests
      sudo --non-interactive mkdir -p /opt/blastit-tests
      sudo --non-interactive chown "$(id -u):$(id -g)" /opt/blastit-tests
      /opt/python/global/bin/python -m venv /opt/blastit-tests
      /opt/blastit-tests/bin/pip install -q dist/blastit-*.whl pytest pytest-django \
        pytest-xdist django "psycopg[binary]"
      sudo --non-interactive chown -R root:root /opt/blastit-tests
      sudo --non-interactive chmod -R go-w /opt/blastit-tests
      /opt/blastit-tests/bin/pytest python/tests -q \
        --ignore=python/tests/runnable_docs/fixtures -p no:cacheprovider'
```
installed tooling:
```
blastit        0.2.0
Django         6.1.3
psycopg        3.3.9
psycopg-binary 3.3.9
pytest         9.2.0
pytest-django  4.14.0
pytest-xdist   3.8.0
```

#### The previously failing protected tier, on its own
```
$ /opt/blastit-tests/bin/pytest python/tests/critical/test_check_contract.py -q -p no:cacheprovider
......................                                                   [100%]
22 passed in 15.31s
CONTRACT_EXIT=0
```

#### The live server (the leg that needs an interpreter with `libpython`)
```
$ /opt/blastit-tests/bin/pytest python/tests/test_live_server.py -q -p no:cacheprovider
.....                                                                    [100%]
5 passed in 10.98s
LIVE_EXIT=0
```

### Whole suite — the job's own command
```
.....................................................s................ ... [ 21%]
…
....................                                                     [100%]
458 passed, 1 skipped, 65 subtests passed in 86.52s (0:01:26)
SUITE_EXIT=0
```
`0 failed, 0 errors`. (The skip is `smoke`-style privilege self-skip inside the
Python tier, unchanged from `MB.52`'s run.)

### Teardown
```
$ docker rm -f blastit-mb53-pg
blastit-mb53-pg
$ docker ps -a --format '{{.Names}}' | grep -c mb53
0
```
The `/opt/blastit-tests` staging lived only inside the `--rm` container.

## Personal-host note (why the venv interpreter differs from CI)

CI's `actions/setup-python` CPython carries `lib/libpython3.14.so`; this image's
`/usr/local` CPython does not, so `python -m venv` from it left the three
live-server tests unable to boot (`… this host has no shared libpython for
CPython 3.14`). Creating the venv from `/opt/python/global/bin/python` (whose
base prefix ships `libpython3.14.so`) is the environment-mirroring fix; no
production code and no test was changed for it.

## Gate

```
$ make fast-check
…
running 16 tests
test a_binary_owned_by_another_uid_is_refused ... ok
…
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s

sh scripts/test_install.sh
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 0.2.0 (fixture))
install-test: ok (b) a wrong digest exits 1 and installs nothing
install-test: ok (c) an overridden origin without a digest fails closed with zero requests
install-test: ok (d) an archive with a symlink member is refused before unpacking
install-test: ok (e) --help exits 0 with env -i (no root, no network)
install-test: ok (f) usage errors exit 2 (bad mirror, --nightly --user, path-shaped --version)
install-test: ok (g) a piped 'curl | bash' run whose child reads stdin reaches the last line
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
FASTCHECK_EXIT=0
```
`make fast-check` → exit `0` (run twice: identical result after this file's
evidence was appended).

## Follow-ups

- `python/tests/README.md`'s recipe should note that the test venv must be
  created from an interpreter that ships a shared `libpython` (this image's
  `/usr/local` CPython does not), or the three live-server tests cannot boot
  production; today only `MB.52`'s `RESULT.md` records the substitution.
- The `--cap-add=SYS_PTRACE` need in a container is unchanged (`LESSONS.md`):
  root in a container cannot read a `PR_SET_DUMPABLE(0)` role's `/proc`
  entries without it.
