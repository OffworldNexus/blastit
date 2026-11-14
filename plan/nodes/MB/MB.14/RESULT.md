# Result MB.14

## Summary

Dogfooding target: `make docs-serve` builds the Zensical site and serves it
through a **freshly built** `target/debug/blastit dev`, on the real
`[route.static]` path (resolver, ETag, `Cache-Control`, refusal matrix). Both
targets are self-contained bash in the `Makefile`; no new scripts, no Python, no
`sudo`, no network beyond the cached `uvx --offline zensical`.

* `make docs-serve` (`PORT ?= 8080`) — `zensical build` (non-strict; `docs-check`
  owns strictness), stage `target/docs-serve/{site/,blastit.toml}`,
  `cargo build -p blastit --locked`, then the server in the foreground with a URL
  line and `Stop: Ctrl-C`. An invalid or out-of-range `PORT` is a parse-time error
  for this goal alone, before zensical or cargo have run.
* `make docs-serve-check` — same stage + build, background server on the first
  free port from 8765 (probed with bash `/dev/tcp`, never assuming 8080 is free),
  ≤15 s readiness wait with the log dumped on timeout, four assertions, trap-based
  teardown (SIGTERM → 5 s → SIGKILL only as a last resort), then two negative
  post-conditions: the port must refuse connections and no process of this run may
  survive. A response that stalls after its headers is a failure (10 s read
  timeout), never an `ok`. State and log live in `target/docs-serve/`.
* Both targets are in `make help` and `.PHONY`; one short paragraph in
  `README.md` where `blastit dev` is introduced. `docs-serve-check` is **not** in
  `make check` (see below).

### Deviations from `NODE.md` (deliberate, verified)

1. **`blastit dev` rejects `--manifest`.** Acceptance point 1 asks for
   `blastit dev --manifest target/docs-serve/blastit.toml`. The built binary's own
   help (`blastit dev --help`) lists only `--port`, `--host`, `--learn`, and the
   CLI refuses the flag:

   ```console
   $ ./target/debug/blastit dev --manifest target/docs-serve/blastit.toml --port 8799
   blastit dev: `--manifest` is not accepted: put `blastit.toml` in the working directory, or set BLASTIT_MANIFEST
   $ echo $?
   1
   ```

   So both targets use the documented route — `BLASTIT_MANIFEST=<abs path>` — and
   keep `root` relative to the manifest's own directory (`root = "site"`); that is
   what `docs/reference/settings.md` (`BLASTIT_MANIFEST`),
   `docs/reference/cli.md` ("`BLASTIT_MANIFEST` names another file, which is the
   only way to move it") and `docs/frameworks/static-and-spa.md` document. The
   behaviour is identical; only the spelling differs, and the docs were already
   right: `--manifest` belongs to `blastit-manifest fold`, not to `blastit dev`.
   `NODE.md`'s context/acceptance text is what drifted. Filed as a follow-up.

2. **No `immutable` glob.** Decision: everything the docs stage serves is
   `no-cache`. `make docs-serve` rebuilds the tree in place, and a dev-time doc
   server must never hand out a year-long cache. Zensical's fingerprinted names are
   only under `assets/*/*` (`assets/javascripts/bundle.<hash>.min.js`,
   `assets/stylesheets/modern/main.<hash>.min.css`), where a bare `/assets/*` would
   not match them anyway (`*` stops at `/` per `docs/frameworks/static-and-spa.md`),
   while `assets/images/*`, `brand/*` and `404.html` are not fingerprinted. The
   check therefore asserts the documented `Cache-Control: no-cache` on a
   **globbed** hashed asset (`assets/javascripts/bundle.<hash>.min.js`), never a
   hardcoded hash.

### Why `docs-serve-check` is not in `make check` (acceptance 5)

It binds a TCP port and builds the standalone `blastit` binary (`cargo build -p
blastit`), while `make check` is the fast, hermetic, unprivileged gate that must
not depend on a free port or a second cargo build target. It also rebuilds
`site/` non-strict, which `docs-check` owns. It stays a developer/CI-job target:
`make docs-serve-check` on its own.

## Evidence

All commands run from `/home/remy/offworld-nexus/blastit`, base
`5c4d8c63a6b7c0af22edb4d07c6d1c77fd9be6d3`. Another node committed `0bbb877` into
this same checkout mid-session, so the final gate and the canonical
`docs-serve-check` capture below are at `0bbb877`. Captures taken before the review
fixes are kept where they still prove their point; `## Review` re-verifies the two
changed behaviours on the final code.

### Acceptance 1 — `make docs-serve` builds the site and serves it through blastit

```
$ make docs-serve PORT=8796
zensical build   -> site/
Build started
No issues found
Build finished in 0.20s
cargo build      -> /home/remy/offworld-nexus/blastit/target/debug/blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.09s

  Docs served by blastit itself (single-uid dev, 127.0.0.1:8796).
  URL:  http://127.0.0.1:8796/
  Stop: Ctrl-C.

BLASTIT_MANIFEST="/home/remy/offworld-nexus/blastit/target/docs-serve/blastit.toml" \
  "/home/remy/offworld-nexus/blastit/target/debug/blastit" dev --port "8796"
  blastit dev · single-uid simulation · 127.0.0.1:8796
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
13:02:56.036 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
13:02:56.055 INFO  ready       serving  routes=1  workers=16
```

Two real `curl -i` runs against that server (body of the 38 KB HTML page elided
and marked; headers verbatim):

```
$ curl -i http://127.0.0.1:8796/
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 38776
ETag: "9778-18e7eb034afc026b"
Last-Modified: Sat, 14 Nov 2026 13:02:55 GMT
X-Content-Type-Options: nosniff
Cache-Control: no-cache

                                   [body: 38 KB of HTML, elided here]
```

```
$ curl -i http://127.0.0.1:8796/mb14-missing
HTTP/1.1 404 Not Found
Content-Type: text/plain; charset=utf-8
Content-Length: 10
Cache-Control: no-store

not found
```

The missing path is `404 text/plain`, **not** the SPA index — the docs site is
served with no `spa`, so `docs/frameworks/static-and-spa.md`'s last row applies.

### Acceptance 2 — `make docs-serve-check`

Canonical run, full output:

```
$ make docs-serve-check
zensical build   -> site/
Build started
No issues found
Build finished in 0.20s
cargo build      -> /home/remy/offworld-nexus/blastit/target/debug/blastit
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
docs-serve-check: serving /home/remy/offworld-nexus/blastit/target/docs-serve/site on 127.0.0.1:8765
docs-serve-check: pid 2174574 (/home/remy/offworld-nexus/blastit/target/debug/blastit dev --port 8765)
ok   / (index)                      200  text/html; charset=utf-8  no-cache
ok   /reference/cli/                200  text/html; charset=utf-8  no-cache
ok   missing path (not the index)   404  text/plain; charset=utf-8  no-store
ok   asset /assets/javascripts/bundle.3cb411a8.min.js 200  text/javascript  no-cache
docs-serve-check: 4/4 checks passed (server log: /home/remy/offworld-nexus/blastit/target/docs-serve/check.log)
$ echo $?
0
$ (exec 3<>/dev/tcp/127.0.0.1/8765) 2>/dev/null && echo ANSWERS || echo closed
closed
$ pgrep -af -- '--dev-root=/run/user/1000/'
no leftovers
```

Note the `pid` line: `$!` is the dev **init** pid. Depending on when it is sampled
it shows the command the recipe started (`…/blastit dev --port 8765`) or, a
millisecond later, the same pid after it execve'd into
`--stage=1 --dev-root=…-<that pid>-…` (runs print either; both name the same pid).
That is why teardown signals `$pid` and the leftover scan is anchored on it.

Port is never assumed free (8765 occupied by a second real `blastit dev`):

```
$ make docs-serve-check          # 8765 already answering
docs-serve-check: serving /home/remy/offworld-nexus/blastit/target/docs-serve/site on 127.0.0.1:8766
ok   / (index)                      200  text/html; charset=utf-8  no-cache
ok   /reference/cli/                200  text/html; charset=utf-8  no-cache
ok   missing path (not the index)   404  text/plain; charset=utf-8  no-store
ok   asset /assets/javascripts/bundle.3cb411a8.min.js 200  text/javascript  no-cache
docs-serve-check: 4/4 checks passed (server log: …/target/docs-serve/check.log)
$ echo $?
0
```

Teardown is clean and the concurrent run was **not** blamed (its pid is not this
run's): `8765: closed`, `8766: closed`, `no leftovers`.

**Negative control** — the real recipe text with one expectation mutated
(`make -n docs-serve-check | sed 's|check "/ (index)" GET / 200 text/html no-cache|check "/ (index)" GET / 500 text/html|'`),
so the FAIL path is proven, not assumed:

```
FAIL / (index)                      wanted 500 text/html
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 38776
ETag: "9778-18e7eae411e9ad3e"
Last-Modified: Sat, 14 Nov 2026 13:00:41 GMT
X-Content-Type-Options: nosniff
Cache-Control: no-cache
Connection: close

<!doctype html>
…
ok   /reference/cli/                200  text/html; charset=utf-8  no-cache
ok   missing path (not the index)   404  text/plain; charset=utf-8  no-store
ok   asset /assets/javascripts/bundle.3cb411a8.min.js 200  text/javascript  no-cache

--- /home/remy/offworld-nexus/blastit/target/docs-serve/check.log (tail) ---
13:00:42.215 INFO  request     GET / → docs  200  0.7ms
13:00:42.226 INFO  request     GET /reference/cli/ → docs  200  0.6ms
13:00:42.236 INFO  request     GET /mb14-does-not-exist → docs  404 unmatched  0.1ms
13:00:42.243 INFO  request     HEAD /assets/javascripts/bundle.3cb411a8.min.js → docs  200  0.1ms
negative-run exit=1
```

Teardown evidence from the server's own log (`check.log`), same shape after
`SIGTERM` and after the Ctrl-C-equivalent `SIGINT` of `make docs-serve`:

```
13:02:56.290 INFO  request     GET /mb14-missing → docs  404 unmatched  0.2ms
13:02:56.327 INFO  shutdown    drain
13:02:56.328 INFO  process.exited -  pid=1995621 uid=0  signal=15
13:02:56.328 INFO  shutdown    stopped
$ for p in 8796; do (exec 3<>/dev/tcp/127.0.0.1/$p) … ; done
8796: closed
$ pgrep -af -- '--dev-root=/run/user/1000/'
no leftovers
```

`docs-serve`'s `Stop: Ctrl-C` is accurate: the server's init shares make's
process group (`make pid=1990696 pgid=1990696`, `init pid=1990820 pgid=1990696`),
so a terminal `SIGINT` reaches it, and `SIGINT` closed the port in ~1 s.

### Acceptance 3 — help + README

```
$ make help | grep docs-serve
docs-serve-check   Bounded smoke test of the docs serve stack (binds a port; not part of `check`)
docs-serve         Build the docs site and serve it through `blastit dev` (PORT=8080)
```

`docs-serve-stage` is deliberately absent from `help` (no `##` doc string) and is
in `.PHONY`. `README.md` gains three lines next to `blastit dev`:

> Our own documentation site is served the same way: `make docs-serve` builds it
> with Zensical and hands it to a freshly built `blastit dev` (`PORT=8080`), and
> `make docs-serve-check` smoke-tests that stack.

### Acceptance 4 — dependency-light, repo writes confined to `site/`, `target/` and `.cache/`

No new script, no Python, no `sudo`, no network: the only tools are `make`, `bash`
(`/dev/tcp`, `printf`), coreutils (`sleep`, `timeout`, `tr`, `cut`, `head`,
`tail`, `ps`), `awk`, `pgrep`, the cached `uvx --offline zensical`, and cargo. The
staged tree is self-contained:

```
target/docs-serve/blastit.toml   # version = 1, one [[route]] path = [".*"], [route.static] root = "site"
target/docs-serve/site/          # copy of zensical's site/
target/docs-serve/check.log
```

Tracked-tree writes: `zensical build` refreshes the repo root **`.cache/`** as well
(it was already there from `docs-check`, and it is self-ignored — its own
`.cache/.gitignore` is a single `*`, verified with `git check-ignore -v .cache`, and
`git status` never sees it). Nothing else outside `site/` and `target/` is written:

```
$ git status --short -- Makefile README.md RELEASING.md
 M Makefile
 M README.md
$ git status --short --untracked-files=all | grep -E '^\?\? (target|site|\.cache)/'
none (target/ and site/ are gitignored; .cache/ ignores itself)
```

### Acceptance 5 — `make check` stays green

```
$ make check
…
56 fences: 29 executed, 27 accounted, 0 failed · 33.1s
uv run --no-project tools/plan.py validate
plan OK
$ echo $?
0
```

`docs-serve-check` is **not** part of it, by design (see Summary).

**One transient, non-mine failure, and why it is not this diff.** A third run
failed one fence:

```
FAIL  docs/frameworks/static-and-spa.md  fence 2  install
  blastit-0.2.0.dist-info/sboms/blastit-py.cyclonedx.json in the installed
  environment differs from blastit-0.2.0-cp314-cp314-manylinux_2_39_x86_64.whl
  (sha256 858a92b3d654): a wheel cached by an earlier run was installed, not the
  one this run built
```

Cause: `dist/` is a shared, generated directory, and the runnable-docs harness
hashes the wheel when the session starts (`harness.py:562`) but re-reads the file
when it verifies the install (`harness.py:742`). Another node committed
`0bbb877 [MB.3.5.1+MB.3.9.1]` into this same checkout mid-session (HEAD moved
`5c4d8c6 → 0bbb877`), so `scripts/build-wheel.sh` rewrote
`dist/blastit-0.2.0-*.whl` with different compiled bytes (the SBOM hashes them)
while this run had already installed the previous wheel. `git diff --stat -- crates
python scripts` is empty for this working tree — the diff here is `Makefile` and
`README.md` only. Re-running the same target on the identical tree:

```
$ make docs-test
…
56 fences: 29 executed, 27 accounted, 0 failed · 32.3s
$ echo $?
0
```

and the full gate above passed afterwards at `0bbb877`.


## Review

Goldfish review: **PASS with three MINOR findings**; all three are fixed in this
tree.

1. **`PORT` was validated only inside the `docs-serve` recipe** — after the stage
   had already spent zensical and cargo — and only as "digits". It is now validated
   at parse time, scoped to that goal (`ifeq ($(filter docs-serve,$(MAKECMDGOALS)),docs-serve)`)
   and before any prerequisite runs, with a real range check:

   ```
   $ make docs-serve PORT=70000
   Makefile:80: *** PORT must be within 1..65535: e.g. `make docs-serve PORT=8080` (got PORT='70000').  Stop.
   $ echo $?
   2                                                  # ~10-26 ms, no zensical, no cargo
   $ make docs-serve PORT=abc
   Makefile:80: *** PORT is not a port number: e.g. `make docs-serve PORT=8080` (got PORT='abc').  Stop.
   ```

   `PORT=0` and `PORT=65536` give the range message, `PORT=''` and `PORT=abc` the
   "not a port number" one; none of them run `zensical build` or `cargo build`
   (a warm run exits in ~10 ms). The validation is scoped to the goal, so a stray
   invalid `PORT` in the environment cannot break the smoke test:
   `PORT=abc make docs-serve-check` → `4/4 checks passed`.
   *Implementation note:* the first attempt used a shell `case … )` inside
   `$(shell …)`; the bare `)` closes the make function call at parse time, so make
   handed the shell a truncated program. The `grep` + `[ … -lt/-gt … ]` form has no
   bare `)` and produces the same messages.

2. **`timeout 10 cat <&3 || true` swallowed a stalled read as `ok`.** `request()`
   now returns `timeout`'s status and `check()` treats any non-zero as a failure:
   rc 124 prints `no complete response within 10 s`, other rc prints
   `request failed (rc=N)`, both print the partial response and fall into the
   existing log-tail dump; a normal close/EOF still returns 0 (the four passing
   checks are that path). Verified with a socket that sends headers and then stalls,
   against the `request`/`status_of`/`header_of`/`check` bodies extracted verbatim
   from `make -n docs-serve-check`:

   ```
   $ bash /tmp/mb14-stall-test.sh      # stalling listener on 127.0.0.1:8795
   FAIL stalls after headers           no complete response within 10 s
   HTTP/1.1 200 OK
   Content-Type: text/html
   Cache-Control: no-cache
   exit would be 1 with fail=1
   wall=10s
   ```

3. **The "nothing is written into the repo outside `site/` and `target/`" claim was
   overstated.** `zensical build` also refreshes the repo-root `.cache/` (6 files
   newer than this session's first build; it is self-ignored — `.cache/.gitignore`
   is a single `*`, and `git check-ignore -v .cache` resolves through it). Acceptance
   4 is reworded to say so.

Post-fix verification: `make docs-serve-check` → 4/4, port refused, no leftovers;
`make docs-serve PORT=8001` serves `200 text/html; charset=utf-8` + `no-cache` and
SIGINT stops it. **`PORT=8000` is not free on this host** — an unrelated Django dev
server (`pid 1707950`) answers `302 → /fr/` there — and blastit fails cleanly and
fast instead of serving the wrong thing:

```
13:12:45.507 ERROR app.output  stderr  binding 127.0.0.1:8000: Address already in use (os error 98)
make: *** [Makefile:222: docs-serve] Error 1
```

(no process was left behind).

## Files

* `Makefile` — `PORT`, `DOCS_SERVE_STAGE`, `DOCS_SERVE_MANIFEST`,
  `DOCS_SERVE_BIN`, `DOCS_SERVE_CHECK_PORT_BASE`; `docs-serve-stage`,
  `docs-serve`, `docs-serve-check`; `.PHONY` and the header target list.
* `README.md` — one paragraph next to `blastit dev`.
* `RELEASING.md` — **not** touched: the node accepts "README **or**
  RELEASING.md", and the dev command belongs where `blastit dev` is introduced.

## Follow-ups

* **`NODE.md` text, not the product, was wrong about `--manifest`.** `blastit dev`
  refuses the flag and names the fix; `docs/reference/cli.md` already says
  `BLASTIT_MANIFEST` "is the only way to move it", and `--manifest` belongs to
  `blastit-manifest fold`. Acceptance point 1 should be restated as
  `BLASTIT_MANIFEST=… blastit dev --port N` so the next node does not re-derive it.
* **`blastit dev` execve's into `--stage=1`** (tooling lesson, not a product bug):
  after boot the process's `cmdline` no longer says `dev`, so
  `pgrep -f "blastit dev --port N"` never matches. Signal the pid you started;
  identify a leftover run by its `--dev-root=... blastit-dev-<uid>-<pid>-<nonce>`.
  Worth one line in `docs/knowledge/LESSONS.md` (out of this node's `paths`).
* Not verified here, out of scope: `blastit check` against the generated
  all-static manifest (the docs site is a preview, not a CI artefact).
* `.github/workflows/docs.yml` could run `make docs-serve-check` as its own step
  (it is bounded and hermetic); `paths` for this node exclude `.github/`, so the
  wiring is left to a CI node.
* **Shared `dist/` + two reads of the wheel** (host concurrency, `python/` is out of
  `paths`): the runnable-docs harness hashes `dist/blastit-<version>-*.whl` at
  session start (`harness.py:562`) and re-reads the file to verify the install
  (`harness.py:742`), so a concurrent `scripts/build-wheel.sh` in the same checkout
  — e.g. another node committing and running `make check` — fails an `install`
  fence with "a wheel cached by an earlier run was installed". Worth a
  `docs/knowledge/LESSONS.md` line, or a lock/one-build-per-run for `dist/`.
