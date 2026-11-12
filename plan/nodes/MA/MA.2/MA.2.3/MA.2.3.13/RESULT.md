# RESULT — MA.2.3.13: `make e2e` on any CPython

STATUS: done. `rm -rf dist && make e2e` exits 0 on this host (CPython 3.14) —
Docker app leg (Django demo + Flask/SQLAlchemy E2b) and the rootless capability
leg — and `make check` is green.

## What was wrong, and why the glob alone was not the fix

`make e2e` builds the wheel on the host (`scripts/build-wheel.sh`, CPython 3.14
here) and hands `dist/` to `e2e/Dockerfile.prebuilt`, whose `COPY` glob was
`dist/blastit-*-cp313-*.whl`. Nothing matched, and BuildKit reports a pattern
that matched nothing as a missing directory:

```console
#9 [5/9] COPY dist/blastit-*-cp313-*.whl /tmp/dist/
#9 ERROR: lstat /dist: no such file or directory
ERROR: failed to build: failed to solve: lstat /dist: no such file or directory
```

Dropping `cp313` from the glob would only have moved the failure: `blastit._blastit`
is **not abi3**, so a `cp314` wheel cannot be installed into a `python:3.13` image
(`pip` says "is not a supported wheel on this platform"). The image's interpreter
has to follow the wheel's tag, which is what the drivers now compute.

## Changes (`e2e/**`, plus the mandatory LESSONS line)

| File | Change |
|---|---|
| `e2e/prebuilt_wheel.py` (new) | Returns the CPython version of the single `dist/blastit-*.whl` (`cp314` → `3.14`). Refuses **zero** wheels, **two** wheels (listing both names) and a tag the e2e images cannot run (abi3, free-threaded). |
| `e2e/Dockerfile.prebuilt` | `ARG PYTHON_VERSION=3.13` → `FROM python:${PYTHON_VERSION}-slim-trixie`; `COPY dist/ /tmp/dist/` instead of a glob that can match nothing; the exactly-one-wheel check keeps its "found N:" listing and no longer names a version. |
| `e2e/rootless/Dockerfile.prebuilt` | Same base-image arg and `COPY`/count check; the capable copy is found from the image's own `python3` (`"$(readlink -f "$(command -v python3)")-init"`) instead of `python3.13-init`, and `CMD` resolves it the same way. |
| `e2e/compose.yaml`, `e2e/rootless/compose.yml` | Build arg `PYTHON_VERSION: "${E2E_PYTHON_VERSION:-3.13}"`; the rootless `command:` resolves the capable copy at run time (`sh -c 'exec "$(readlink -f "$(command -v python3)")-init" -m blastit'`), so no Python version is written down. |
| `e2e/run.py`, `e2e/rootless/run.py` | Export `E2E_PYTHON_VERSION` when the selected Dockerfile is a prebuilt one. The in-image `Dockerfile` builds its own wheel on its own interpreter and needs no `dist/`. |
| `e2e/demo/blastit.toml`, `e2e/grants.py` | See the next section (second blocker). |
| `e2e/README.md`, `e2e/rootless/README.md` | Describe the wheel-tag base image. |
| `docs/knowledge/LESSONS.md` | One lesson: a `COPY` glob that matches nothing blames the directory, and a non-abi3 wheel's `cp3NN` tag *is* the interpreter it needs. |

Isolation assertions untouched: `e2e/rootless/run.py`'s E1/D6 checks are byte-for-byte
unchanged (they already resolved the capable copy as `"$(readlink -f "$(command -v python3)")-init"`),
and the rootless container still starts from that copy — now `python3.14-init`
with the same seven capabilities.

## A second, pre-existing blocker the run exposed

Once the image built, the Django leg failed at the login form on **any**
interpreter: `e2e/demo/blastit.toml` predates the `[route.ingress]` body gate
(ADR-0006, runtime enforcement in MA.2.2.3), so a POST with a form body on
`/login/`, `/logout/` or `/notes/` is refused `413` by design:

```console
  FAIL a wrong password is refused
  FAIL the right password logs in
…
"method":"POST","outcome":"unmatched","path":"/login/",...,"route":"login","rule":0,"status":413
…
FAIL: 11 failure(s)
…
FAIL the password-hash read from the notes route was denied and logged
FAIL: 1 failure(s)
```

(The SQLAlchemy app passed in the same run: its POSTs carry no body. That is how
the failure is specific to the demo's forms and not to the Python version.)

`docs/frameworks/django.md` — the accepted user DX — requires
`[route.ingress] body = "form"` on a route that takes a form body, so the fixture
declares it for `login`, `logout` and `notes`, in the committed manifest **and**
in the generator (`e2e/grants.py`, `INGRESS`), with a header line that says why.
That is a stronger posture than before (the bodies are now validated and
re-encoded by `blastit-ingress`), not a relaxed assertion. Running the app leg
with the manifest reverted (`git show HEAD:e2e/demo/blastit.toml`) reproduced
exactly the failure above; restoring it is the fix.

## Acceptance

### `rm -rf dist && make e2e` → `EXIT=0`

Wheel the host produced (CPython 3.14), and the image the driver picked for it:

```console
$ rm -rf dist && make e2e
scripts/build-wheel.sh
🐍 Found CPython 3.14 at /home/remy/.cache/uv/archive-v0/uJWfWrFj-SC_y89E/bin/python3
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl
E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
 Image blastit-e2e Building
#3 [internal] load metadata for docker.io/library/python:3.14-slim-trixie
```

App leg (Django demo, its logs, then the Flask + SQLAlchemy app and its log):

```console
PASS: 0 failure(s)
logs
  ok   every line is JSON (0 not: [])
  ok   the database password appears nowhere in the logs
  …
  ok   the password-hash read from the notes route was denied and logged
  ok   the destructive statement was refused and logged
  …
  ok   /login/ was routed to login
  ok   /notes/ was routed to notes
PASS: 0 failure(s)
the app
  …
  ok   the write route answers (200)
  ok   the row the write route inserted is the one the read route reads ([{'body': 'sqlalchemy', 'id': 1}])
  ok   a statement no route grants is refused (500)
  ok   and nothing was deleted ([{'body': 'sqlalchemy', 'id': 1}, {'body': 'sqlalchemy', 'id': 2}])
PASS: 0 failure(s)
blastit's log
  ok   exactly the statement no route grants was denied (…'sqlstate': '42501'…)
PASS: 0 failure(s)
```

Rootless capability leg (no summary line: the exit status is the verdict):

```console
[rootless 21:45:21] the dedicated interpreter's file capabilities: /usr/local/bin/python3.14-init cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap=ep
shared:
D6: what the container's own processes hold
  ok   the layout runs more than one process (10)
  ok   uid 20001 (pid 21) holds nothing and has no_new_privs (inh=0000000000000000 prm=0000000000000000 eff=0000000000000000 bnd=0000000000000000 amb=0000000000000000 nnp=1)
  …
  ok   http runs under uid 20001 (D6 keeps the layout)
  ok   scheduler runs under uid 20002 (D6 keeps the layout)
  ok   dbproxy runs under uid 20003 (D6 keeps the layout)
  ok   a plain python3, as the worker's uid (10001) (uid 10001) holds nothing and cannot become root (… setuid=refused)
  ok   the capable copy is mode 0500 and owned by init's uid (mode=500 uid=10000)
  ok   init's own uid can run it — which is why nothing else may use that uid (Python 3.14.10)
  ok   the worker's uid cannot run it (sh: 1: /usr/local/bin/python3.14-init: Permission denied)
EXIT=0
```

### The exactly-one-wheel check still refuses zero and two wheels

The Dockerfile check (same for both prebuilt files; `--progress=plain`):

```console
#10 0.175 e2e/rootless/Dockerfile.prebuilt wants exactly one blastit wheel in dist/, found 2:
#10 0.175 /tmp/dist/blastit-0.0.0-cp314-cp314-manylinux_2_39_x86_64.whl
#10 0.175 /tmp/dist/blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl
#10 ERROR: process "/bin/sh -c set -e; …" did not complete successfully: exit code: 1
```

```console
#10 0.163 e2e/Dockerfile.prebuilt wants exactly one blastit wheel in dist/, found 0:
#10 0.163
#10 0.163 clean up dist/, or keep only the one this image should install
#10 ERROR: process "/bin/sh -c set -e; …" did not complete successfully: exit code: 1
```

and the drivers fail before Docker starts, with the same information:

```console
$ cp dist/blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl \
     dist/blastit-0.0.9-cp313-cp313-manylinux_2_39_x86_64.whl   # a stale second wheel
$ E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
a prebuilt e2e image installs exactly one wheel from /home/remy/offworld-nexus/blastit/dist, found 2:
  blastit-0.0.9-cp313-cp313-manylinux_2_39_x86_64.whl
  blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl
build it with scripts/build-wheel.sh, and clean up the other wheels
EXIT=1
```

### `make check` → `EXIT=0`

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (3 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 14 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked -- -D warnings
…
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 5.03s
…
protected critical tests hold no skip construct
…
Build started
No issues found
Build finished in 0.18s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

## Follow-ups (not in this node)

- `e2e/venv-smoke.sh` still defaults to `dist/blastit-*cp314*.whl`, which is
  right for its `modelw/base` CPython 3.14 image but means a 3.13 host must pass
  `WHEEL=`. It is a different image contract (that platform pins 3.14), so it was
  left alone; if that platform ever moves, the script needs the same treatment.
- `e2e/*.py` is not `ruff format`-clean (the editor formatter rewrites whole files
  on edit). The driver edits here were applied without reformatting to keep this
  diff reviewable; a `ruff format e2e` node would be a separate, mechanical change.
- `e2e/sqlalchemy/blastit.toml` still declares no `[route.ingress]` because
  neither of its routes sends a body (its POSTs carry the value in the query
  string). Worth remembering if that fixture ever grows a form.
- `plan/BUDGET.md` and `plan/JOURNAL.md` were already modified before this node
  started; they are the lead's, untouched here (`plan/STATE.md`, regenerated while
  reading state, was restored to HEAD).
