# RESULT — MA.3.4: run the Docker root + rootless e2e suites

STATUS: done, **with no `e2e/**` change required**. `make e2e` exits 0 at
`aaeadf4` (on top of `7440cd5`): the root app leg (Django demo + Flask/SQLAlchemy
E2b) and the rootless capability leg both pass, the fail-closed proof holds, and
`make check` is green. There was no fallout to fix — the analysis in "Why no
`e2e/**` edit" below says exactly which of the two new commits could have reached
an e2e assertion, and why none did.

## What actually ran

`make e2e` rebuilds the wheel first, so the images install code from the working
tree at `aaeadf4`, not a stale `dist/`:

```console
$ make e2e
scripts/build-wheel.sh
    Finished `release` profile [optimized] target(s) in 0.09s
🐍 Found CPython 3.14 at /home/remy/.cache/uv/archive-v0/uJWfWrFj-SC_y89E/bin/python3
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.0-cp314-cp314-manylinux_2_39_x86_64.whl
E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
…
#11 1.418 Successfully installed blastit-0.1.0
```

Docker 29.9.3, host CPython 3.14.10, one wheel in `dist/` (`cp314`, so the
prebuilt images build on `python:3.14-slim-trixie`).

## Acceptance

### 1. `make e2e` → `EXIT=0`

Both suites, five `PASS: 0 failure(s)` lines and no `FAIL`: `scenario.py
--self-test`, the Django scenario, its log checks, the SQLAlchemy app and its log
checks in the app leg, then the rootless self-test, scenario, log + D6 + E1
checks, proxy-header probe and in-container probes.

App leg (Django, production `blastit` as root) — scenario then logs:

```console
log out
  ok   logging out redirects
  ok   notes need a login again

PASS: 0 failure(s)
logs
  ok   blastit printed events
  ok   every line is JSON (0 not: [])
  ok   every line is at most 2048 bytes
  ok   the database password appears nowhere in the logs
  ok   boot and ready events
  ok   the boot event says the seccomp filters are on
  ok   one stage 0 boot event (1)
  ok   the boot event says the mode init runs in (root)
  ok   the capability set is listed exactly in rootless mode (root: [])
  ok   one preload boot event (1)
  ok   the boot event says whether the heap was immortalised (on)
  ok   the immortalised count is an integer (119919)
  ok   the pass was timed (36.36167ms)
  ok   the view process cap was derived from memory (1804)
  ok   mem_total is a byte count (67346608128)
  ok   mem_used is a byte count (50454976)
  ok   the password-hash read from the notes route was denied and logged
  ok   the destructive statement was refused and logged
  …
  ok   /debug/egress/ was routed to debug
  ok   /egress/probe/ was routed to egress

PASS: 0 failure(s)
```

Second app (Flask + SQLAlchemy, E2b) and its log:

```console
the app
  ok   the write route answers (200)
  ok   the row the write route inserted is the one the read route reads ([{'body': 'sqlalchemy', 'id': 1}])
  ok   the app was handed blastit's URL, the user and no password ('postgresql://blastit@/demo?host=/proc/self/cwd&port=5432')
  ok   a second insert sees both rows ({'added': True, 'count': 2})
  ok   a statement no route grants is refused (500)
  ok   and nothing was deleted ([{'body': 'sqlalchemy', 'id': 1}, {'body': 'sqlalchemy', 'id': 2}])

PASS: 0 failure(s)
blastit's log
  ok   every request is in the log (5 of 5)
  ok   four of them answered 200 ([200, 200, 200, 500, 200])
  ok   and each went to its own route (['notes-clear', 'notes-read', 'notes-write'])
  ok   exactly the statement no route grants was denied (…'sql': 'DELETE FROM sqlalchemy_notes', 'sqlstate': '42501'…)
  ok   refused by the proxy (SQLSTATE 42501), not by the database's own grants

PASS: 0 failure(s)
```

Rootless leg — the scenario and the shared log checks:

```console
uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt
…
PASS: 0 failure(s)
logs
  ok   every line is JSON (0 not: [])
  ok   one stage 0 boot event (1)
  ok   the boot event says the mode init runs in (capabilities)
  ok   the capability set is listed exactly in rootless mode (capabilities: ['cap_chown', 'cap_dac_override', 'cap_fowner', 'cap_kill', 'cap_setgid', 'cap_setuid', 'cap_setpcap'])
  ok   the view process cap was derived from memory (1804)
  ok   mem_used is a byte count (50245920)
  …
PASS: 0 failure(s)
```

D6's own assertions, E1's preset/ingress assertions, and the in-container
probes:

```console
D6: the boot event says which mode init runs in
  ok   the stage 0 boot event says `privilege: capabilities` (capabilities)
  ok   the preload boot event says `privilege: capabilities` (capabilities)
  ok   and names exactly the capabilities it holds (['cap_chown', 'cap_dac_override', 'cap_fowner', 'cap_kill', 'cap_setgid', 'cap_setuid', 'cap_setpcap'])
D6: the sandboxes are unchanged
  ok   view processes were forked (7)
  ok   every view runs under a uid of its own, above the layout base ([21000, 21001, 21002, 21003, 21004, 21005, 21006])
  ok   the seccomp filters are on in the boot event
  ok   nothing else runs as init's uid at boot ([])
E1: the boot event names the preset the deployment uses
  ok   a boot event says `preset: compose-traefik` (['stage0', 'config', 'preload'])
  ok   the config boot event names it (compose-traefik)
  ok   and the settings it supplied ({'BLASTIT_CLIENT_IP_HEADER': 'x-forwarded-for', 'BLASTIT_FAIR_QUEUING': 'on', 'BLASTIT_LOG_FORMAT': 'json', 'BLASTIT_TRUSTED_PROXIES': '*'})
  ok   with nothing overriding them (None)
E1: the ingress decides the scheme and the client address
  ok   a request Traefik TLS-terminated is `https` to the app (https)
  ok   and the client address is the one Traefik saw (172.19.0.1, expected 172.19.0.1)
[rootless 23:52:01] the dedicated interpreter's file capabilities: /usr/local/bin/python3.14-init cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap=ep
shared:
[rootless 23:52:01] the app container's limits: User=10000:10000 CapDrop=[ALL] CapAdd=[CAP_CHOWN CAP_DAC_OVERRIDE CAP_FOWNER CAP_KILL CAP_SETGID CAP_SETPCAP CAP_SETUID]
D6: what the container's own processes hold
  ok   the layout runs more than one process (10)
  ok   uid 20001 (pid 21) holds nothing and has no_new_privs (… bnd=0000000000000000 amb=0000000000000000 nnp=1)
  ok   http runs under uid 20001 (D6 keeps the layout)
  ok   scheduler runs under uid 20002 (D6 keeps the layout)
  ok   dbproxy runs under uid 20003 (D6 keeps the layout)
  ok   a plain python3, as the worker's uid (10001) (uid 10001) holds nothing and cannot become root (… setuid=refused)
  ok   the capable copy is mode 0500 and owned by init's uid (mode=500 uid=10000)
  ok   init's own uid can run it — which is why nothing else may use that uid (Python 3.14.10)
  ok   the worker's uid cannot run it (sh: 1: /usr/local/bin/python3.14-init: Permission denied)
EXIT=0
```

`shared:` on its own line is the point: `getcap` names nothing on the shared
interpreter, so every process that runs `python3` holds no capability.

### 2. The fail-closed proof still holds

The rootless run proves the documented set is *enough*. The other half — a
production `blastit` with neither root nor those capabilities refuses to boot — is
not something `make e2e` starts (there is no negative container in either suite),
so it was reproduced directly against the image the app leg built:

```console
$ docker run --rm --user 10000:10000 --cap-drop ALL --network none \
    --entrypoint blastit blastit-e2e:latest
{"ts":"…","level":"info","event":"boot","component":"init","stage":"stage0","seccomp":true,"preset":null}
{"ts":"…","level":"error","event":"app.output","component":"init","stream":"stderr","line":"init runs as uid 10000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid.","part":1,"parts":1}
{"ts":"…","level":"error","event":"app.output",…,"line":"  A rootless deployment gives the capabilities to a *dedicated copy* of the interpreter, never to the shared one:"…}
…
{"ts":"…","level":"error","event":"app.output",…,"line":"  To run the same tree locally without root, use `blastit dev`; it is a single-uid simulation, not a sandbox"…}
EXIT=1
```

Exit 1, one event naming each missing capability, the `setcap`/`chmod 0500`
recipe, and no secret or credential in any line. `init` does not silently fall
back to single-UID execution.

### 3. `make check` → `EXIT=0`

Run with the sibling's in-progress `docs/` edits present in the worktree (see
"Not mine" below). Nothing was red, including the docs half:

```console
$ make check
uv run --no-project tools/guard.py
guard: OK (0 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD
critical-guard: HEAD..working tree, 13 changed file(s)
  no change to a protected critical test
cargo fmt --all -- --check
… cargo clippy --workspace --all-targets --locked -- -D warnings
… cargo doc --workspace --no-deps --locked
… test result: ok. 176 passed; 0 failed; 1 ignored; …
…
PASS  docs/reference/cli.md  fence 3  serve-flags           0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                 0.0s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked (The e2e harnesses need Docker and root; the runnable-docs gate is unprivileged and Docker-free, so only the command names and the scripts they name are checked.)

38 fences: 26 executed, 12 accounted, 0 failed · 27.4s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

### 4. `git status --porcelain e2e/` → empty

```console
$ git status --porcelain e2e/
$                                     # no output: no e2e/** change was needed
```

Cleanup verified — the harness removes what it starts:

```console
$ docker ps -a --format … | grep -i blastit   →  no blastit containers
$ docker volume ls … | grep -i blastit        →  no blastit volumes
$ docker network ls … | grep -i blastit       →  no blastit networks
```

The only containers left on this host belong to other projects.

## Why no `e2e/**` edit (and why that is not a missed failure)

Two commits landed on top of the tree MA.2.3.13 made green:

* `7440cd5` (MA.3.7) — watermark reading, `queries = []` listeners, one-pass
  `dev --learn`. Its behaviour changes are in `run_dev` / `dev_view_sockets` /
  `dev_learn` (`crates/blastit/src/stage0.rs`) and in `mem_usage`, which both legs
  exercise only through the `boot` event's `mem_total` / `mem_used` /
  `max_view_processes`. Those three fields keep their names and types
  (`stage1.rs::preload_lifecycle`), and `check_logs.py` asserts exactly that
  (`mem_total is a byte count`, `mem_used is a byte count`, `the view process cap
  was derived from memory`) — which is why the reading changed (`mem_used` is now
  ~50 MB, not page-cache-inflated) without an assertion moving. The one
  production-visible rule — a route that *declares* `queries = []` gets a denying
  listener — changes no socket for the fixtures: neither `e2e/demo/blastit.toml`
  nor `e2e/sqlalchemy/blastit.toml` declares an empty grant (the routes with no
  database are routes with no `[route.db]` at all, and `route_declares_proxy`
  keeps those socket-less). The app leg's `42501` assertions and the SQLAlchemy
  `DELETE` denial therefore keep their meaning and still fire.
* `aaeadf4` (MA.3.1) — the runnable-docs harness. It touches `Makefile` and
  `python/tests/runnable_docs/` only, and is not part of `make e2e`.

Nothing under `e2e/**` was changed: no test was skipped, no assertion widened, no
failure swallowed. The suites at this HEAD are the evidence that the e2e tier
survived both commits; if the reviewer wants the `e2e` fixtures to cover the new
`queries = []` rule and the negative fail-closed boot as *permanent* checks, that
is new coverage, not fallout — see Follow-ups.

## Follow-ups (not in this node)

* **`queries = []` has no e2e coverage.** MA.3.7's documented rule (a route that
  declares the proxy with an empty grant gets a `0600` listener that denies every
  statement with `42501`) is proven by unit/critical tests but not by
  `make e2e`: the demo app has no raw-SQL view on a route that declares an empty
  grant, and `e2e/grants.py` cannot emit `queries = []`. A fixture change would
  add a view + a probe and teach `grants.py` to declare an empty grant; it would
  also have to stay compatible with `e2e/live.py` (App Platform), so it is its own
  node.
* **No e2e-level negative boot.** Neither suite starts production `blastit`
  without root/capabilities and asserts the refusal; that half of the fail-closed
  proof lives in `crates/blastit/tests/critical_dev_and_fail_closed.rs` and in the
  manual `docker run` above. A `docker compose run --rm -u 10001` service (or a
  `docker run` against the built image) with `--cap-drop ALL` asserting exit ≠ 0
  and the named capabilities would make it permanent.
* **Cosmetic label in `e2e/check_logs.py`.** In the root leg it prints `ok the
  capability set is listed exactly in rootless mode (root: [])`: the assertion is
  right (`bool(capabilities) == (privilege == "capabilities")`), the sentence
  reads oddly when the mode is `root`. Left alone here to keep this node's diff
  empty; a one-word fix for whoever next edits the file.
* **Not mine, in the worktree.** `ARCHITECTURE.md` and eight `docs/**` pages are
  modified by the concurrent docs-only sibling; `plan/JOURNAL.md` and the
  `MA.3.2` / `MA.3.4` node files are the lead's bookkeeping. None of them was
  touched here, and the `make check` result above covers them as they stood.
