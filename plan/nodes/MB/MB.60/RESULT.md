# MB.60 — RESULT

Survive sury's stale arm64 `Packages.xz` in the php-sapi Dockerfile, and stop the
driver from hiding the build error.

## Changes (2 files, both in `paths`)

1. **`e2e/php-sapi/Dockerfile`** — the update that re-reads the indexes after the
   extra repository is added (the Debian-only *first* update is untouched; this
   one runs on every apt leg, with or without an extra repo) is now

   ```sh
   if ! apt-get update; then \
       echo "apt-get update failed; retrying with the .gz indexes (a second failure means the repository is broken or unreachable)" >&2; \
       apt-get -o Acquire::CompressionTypes::Order::=gz update; \
   fi; \
   ```

   with a comment naming the exact upstream defect (fresh `InRelease` at
   `2026-11-21T10:18:11Z` advertising `main/binary-arm64/Packages.xz` = 291892 B /
   SHA256 `9eda492b…d165` while the mirror served 290956 B; that suite's `.gz`
   correct) and saying the retry prefers `.gz` so a mirror sync race cannot fail
   a leg that installs fine — while a `.gz` index that is wrong too, or a missing
   package at the `apt-get install` below, still fails the leg. The message names
   both outcomes because the `if !` guard fires on any failure (DNS, an expired
   `Release`, a GPG error), not only a compression mismatch. No global
   `Acquire::CompressionTypes::Order`, no change to the Debian-only first update,
   `set -eux` semantics intact (the `!` condition is exempt from `-e`).
2. **`e2e/php_sapi_probe.py` `build()`** — `"--progress=plain"` added to the
   `docker("build", …)` argument list, and the non-zero exit now reports a
   120-line tail instead of the default 40, prefixed with
   `  … N earlier line(s) omitted …` when the buffer is longer than that, so a cut
   log cannot be read as the whole build output. The tail length lives in one
   local (`keep`), so the omitted count and the slice cannot disagree, and the
   marker says `1 earlier line` for a single line (this repo's human-output rule
   asks for `1 request`, never `1 requests`). `tail()` itself is unchanged, so
   the other call sites keep the silent default. Evidence:

   ```
   $ grep -n "keep = 120\|omitted\|earlier \|lines=keep\|lines=120" e2e/php_sapi_probe.py
   224:        keep = 120  # one number, so the count and the slice cannot disagree
   225:        omitted = len(log.strip().splitlines()) - keep
   226:        noun = "line" if omitted == 1 else "lines"
   227:        marker = f"  … {omitted} earlier {noun} omitted …\n" if omitted > 0 else ""
   228:        return False, marker + tail(log, lines=keep)
   ```

   ```python
   if result.returncode != 0:
       # BuildKit's own failure summary is ~38 lines, so the default 40-line tail
       # can end exactly before the real `E: Failed to fetch …` apt line (that is
       # how the sury index race took two CI runs to diagnose, MB.60).
       log = result.stdout + result.stderr
       keep = 120  # one number, so the count and the slice cannot disagree
       omitted = len(log.strip().splitlines()) - keep
       noun = "line" if omitted == 1 else "lines"
       marker = f"  … {omitted} earlier {noun} omitted …\n" if omitted > 0 else ""
       return False, marker + tail(log, lines=keep)
   return True, ""
   ```

   `--progress` itself needs BuildKit/buildx (the docker ≥ 23 default), so a
   classic builder (`DOCKER_BUILDKIT=0`) rejects it with `unknown flag`; it pins
   the plain display whether the driver runs under a pipe (CI) or a terminal, and
   the actual repair for the swallowed apt line is the longer failure tail.

## Proof 1 — the normal path still builds and serves on this host

```
$ make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1
php-sapi-e2e: docker build --build-arg BASE=debian:bookworm-slim --build-arg PHP_MINOR=8.6 -t blastit-php-sapi-e2e:local
php-sapi-e2e: readelf -d libblastit_php86-x86_64-gnu.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.6 abi 1: status 200, cpu_ns 12558538 <= wall_ns 28099538, exit()+fatal contained, next request served
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.6 abi 1: status 200, cpu_ns 8015811 <= wall_ns 11882456, exit()+fatal contained, next request served
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.6 abi 1: status 200, cpu_ns 8361300 <= wall_ns 11903830, exit()+fatal contained, next request served
```

Exit 0, zero `FAIL`/`Traceback`/`Hash Sum mismatch` lines in the full log. This is
also what proves the `--progress=plain` argument is accepted by this host's
builder (BuildKit) and that the Dockerfile's comment lines inside the `RUN`
continuation are legal: the Dockerfile parser strips them and the shell runs the
new `if ! apt-get update; then … fi`.

The apt layer itself was then re-executed from a byte-identical staged copy of the
real file (only change: `FROM ${BASE}` → `FROM ${BASE} AS base`, so `--target base`
can stop right after it), `--no-cache --progress=plain`:

```
#6 [2/8] RUN set -eux;     if command -v apt-get >/dev/null; then …  fi;         if ! apt-get update; then             echo "apt-get update failed; retrying with the .gz indexes (a second failure means the repository is broken or unreachable)" >&2;             apt-get -o Acquire::CompressionTypes::Order::=gz update;         fi;         apt-get install -y --no-install-recommends …
#6 10.41 + [ sury = sury ]
#6 10.48 + echo deb [signed-by=/etc/apt/keyrings/sury.gpg] https://packages.sury.org/php/ bookworm main
#6 10.48 + apt-get update
#6 10.54 Get:4 https://packages.sury.org/php bookworm InRelease [6084 B]
#6 10.68 Get:5 https://packages.sury.org/php bookworm/main amd64 Packages [293 kB]
#6 10.75 Reading package lists...
#6 11.20 + apt-get install -y --no-install-recommends php8.6-dev libphp8.6-embed …
…
#13 naming to docker.io/library/mb60-apt-branch:latest done
```

No `retrying with the .gz indexes` line in the trace: on a healthy repository the
fallback is a no-op and `apt-get install` sees exactly the same package set.
(The single textual occurrence of that sentence in the log is the `#6 [2/8] RUN …`
instruction echo, quoted above with this node's final wording — the staged copy
ran before the review reworded the message; the shell path it executed is
identical.)

## Proof 2 — the arm64 defect, container-local

### 2a. The live mirror has since converged (measured, in-container and by curl)

The stale window closed between the PR #64 red checks and this run, so the live
URL no longer fails. Measured now:

```
$ grep -E '^(Date|Valid-Until):' InRelease
Date: Sat, 21 Nov 2026 10:18:11 UTC
$ grep -A1 'main/binary-arm64/Packages\.xz$' InRelease
 9eda492bcdd9dc975b34b8c0414962bc90aa7a4470ac89939cb746bb3d5cd165 291892 main/binary-arm64/Packages.xz
$ curl -fsSL …/main/binary-arm64/Packages.xz -o pkgs-arm64.xz
xz: 291892 bytes  sha256=9eda492bcdd9dc975b34b8c0414962bc90aa7a4470ac89939cb746bb3d5cd165
```

i.e. the bytes *and* the size now match the very `InRelease` that was published at
10:18:11Z — the index apt advertised and the file the mirror served are the same
file again. In `debian:bookworm-slim`, with the sury list written exactly as the
Dockerfile writes it plus `dpkg --add-architecture arm64`:

```
echo "deb [signed-by=/etc/apt/keyrings/sury.gpg arch=arm64] https://packages.sury.org/php/ bookworm main" > /etc/apt/sources.list.d/sury.list
=== (a) plain apt-get update ===
Get:4 https://packages.sury.org/php bookworm InRelease [6084 B]
Get:6 https://packages.sury.org/php bookworm/main arm64 Packages [292 kB]
Fetched 9371 kB in 2s (5999 kB/s)
plain-exit=0
=== (b) apt-get -o Acquire::CompressionTypes::Order::=gz update ===
Hit:4 https://packages.sury.org/php bookworm InRelease
fallback-exit=0
```

A sweep of every `SHA256:`-listed file in that `InRelease` against its served
`Content-Length` found **no** binary-index mismatch left (only `main/source/*` and
`main/Contents-*`, which this repo does not publish, answer 404).

### 2b. Deterministic reproduction of the same defect class (the fallback is measured, not asserted)

Because the live race is over, the exact shape was rebuilt locally in
`debian:bookworm-slim` (apt 2.6.1 — the same apt every leg uses): a real APT
repository whose `Release` advertises `main/binary-amd64/Packages.xz` as 1104
bytes while the served file is 168 bytes, and whose `Packages.gz` is correct,
served over HTTP:

```
--- the sury shape, reproduced: index sizes advertised vs served ---
  Packages.xz served = 168 bytes, Release advertises 1104 bytes
  Packages.gz served = 133 bytes, Release advertises 133 bytes (correct)
=== (a) plain apt-get update (the pre-fix Dockerfile) ===
Get:4 http://127.0.0.1:8080 bookworm/main amd64 Packages [1104 B]
Err:4 http://127.0.0.1:8080 bookworm/main amd64 Packages
  File has unexpected size (168 != 1104). Mirror sync in progress? [IP: 127.0.0.1 8080]
E: Failed to fetch http://127.0.0.1:8080/dists/bookworm/main/binary-amd64/Packages.xz  File has unexpected size (168 != 1104). Mirror sync in progress? [IP: 127.0.0.1 8080]
E: Some index files failed to download. They have been ignored, or old ones used instead.
plain-exit=100
=== (b) apt-get -o Acquire::CompressionTypes::Order::=gz update (the Dockerfile's fallback) ===
Get:4 http://127.0.0.1:8080 bookworm/main amd64 Packages [133 B]
Reading package lists...
fallback-exit=0
fixture known to apt afterwards: from-fixture:   Installed: (none)
```

The failure text is the CI sentence verbatim (`File has unexpected size … Mirror
sync in progress?`, exit 100 on a failing `apt-get update` under `set -e`), and the
Dockerfile's own fallback line turns it into `exit 0` with the index parsed —
apt picks `.xz` by default and `.gz` only when told to, which is why the retry is
needed rather than a global setting.

### 2c. The retry does not weaken the assertion

Same harness, both compressed variants advertised longer than served:

```
=== phase C: both compressed variants bad — the fallback must still fail ===
plain-exit=100
E: Failed to fetch file:/repoC/./Packages.gz  Hash Sum mismatch
E: Some index files failed to download. They have been ignored, or old ones used instead.
fallback-on-broken-repo-exit=100
```

So a genuinely broken repository still fails the leg; the fallback only switches
index compression. The same run also shows the `file:` transport path
(`plain-exit=100` / `fallback-exit=0`, `apt-cache policy from-fixture` resolving
after the retry).

## Proof 3 — gates (lead-run, final tree)

```
$ uvx --offline ruff check e2e/php_sapi_probe.py
All checks passed!
$ uvx --offline ruff format --check e2e/php_sapi_probe.py
1 file already formatted
$ make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1
…
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.6 abi 1: status 200, cpu_ns 20802206 <= wall_ns 24399035, exit()+fatal contained, next request served
E2E_EXIT=0   (3 probe cases ok, 0 FAIL/Traceback)
$ uv run --no-project tools/guard.py
guard: OK (3 staged/untracked file(s) checked)
$ uv run --no-project tools/plan.py validate
plan OK
```

### Marker arithmetic, measured on the edited file rather than asserted

`build()`'s real failure branch was driven by loading `e2e/php_sapi_probe.py`
itself (`importlib`) and replacing only `leg()` (so no `docker`/matrix call) and
`docker()` (a failing build whose `stdout` is the fake log) — the marker lines are
the driver's own:

```
$ uv run --no-project python /tmp/opencode/mb60_marker_probe.py
input log lines = 130 (php_sapi_probe.py)
  ok            = False
  marker        = '  … 10 earlier lines omitted …'
  report lines  = 121 (marker + tail)
  first kept    = '  line 011'
  last kept     = '  line 130'
input log lines = 121 (php_sapi_probe.py)
  ok            = False
  marker        = '  … 1 earlier line omitted …'
  report lines  = 121 (marker + tail)
  first kept    = '  line 002'
  last kept     = '  line 121'
input log lines = 120 (php_sapi_probe.py)
  ok            = False
  marker        = '(no marker)'
  report lines  = 120 (marker + tail)
  first kept    = '  line 001'
  last kept     = '  line 120'
```

130 = 10 omitted + 120 kept, 121 = 1 omitted + 120 kept (`1 earlier line`,
singular), and 120 prints no marker at all. The Dockerfile `echo` keeps its long
wording on purpose: it names both outcomes of the `if ! apt-get update` guard
(DNS/expired `Release`/GPG as well as a compression mismatch), so the shorter
wording suggested in review was rejected rather than applied.

The e2e target was run **twice** on this tree, including once after the review
round rewrote the comment and the fallback `echo`: the reworded `RUN` really does
parse and execute, the image is built through the driver with `--progress=plain`
in its argv, and the retry stays a no-op on a healthy repository (no
`retrying with the .gz indexes` line in the run).

### `make fast-check` on this host is red BEFORE and AFTER this node (out of scope)

`make fast-check` is exit **2** here for a reason that has nothing to do with this
node: `crates/blastit/tests/common/project.rs:42-49` (`scratch_root()`) puts
fixtures under `/tmp/opencode` whenever that directory is creatable, which pushes
one fixture's Unix socket to 109 bytes, one over the 107-byte `sun_path`:

```
$ cargo test -p blastit --test e2e_check_fmt learn_writes_one_grant_without_widening_a_view
thread 'learn_writes_one_grant_without_widening_a_view' panicked at crates/blastit/tests/e2e_check_fmt.rs:642:5:
assertion `left == right` failed: blastit check: /tmp/opencode/blastit-e2e-dev-1367955-learn-write/tmp/blastit-check-1000-1367962-878264288-0/pg/.s.PGSQL.5432: path must be shorter than SUN_LEN
  left: Some(2)
 right: Some(0)
```

That is the whole failure: no fast-check target can be affected by this change,
because `git diff --name-only dffa4b5` lists only `e2e/php-sapi/Dockerfile`,
`e2e/php_sapi_probe.py` and `plan/`. The same assertion was already red at
`ab50fe0` (`MB.58`), so it is pre-existing and belongs to a node owning
`crates/blastit/tests/` — see `## Follow-ups`.

Two non-mutating confirmations that the failure is host path length, not logic:

- In a private mount namespace (`unshare -rm`, `/tmp` a fresh tmpfs, `/tmp/opencode`
  a plain file so `scratch_root()`'s `create_dir_all` fails and the helper falls
  back to `std::env::temp_dir()`), `e2e_check_fmt` is fully green —
  `24 passed; 0 failed`, including `learn_writes_one_grant_without_widening_a_view ... ok`.
  (The same namespace makes `is_root()` true, so the root-only
  `a_binary_owned_by_another_uid_is_refused` then fails on `chown` → `EINVAL`;
  that is the namespace, not the tree.)
- The earlier `targets ok: 59 FAILED: 0 passing tests: 332` figure in this file
  was obtained by renaming the real `/tmp/opencode` aside. **That method is
  retired**: `/tmp/opencode` serves the live documentation preview, and a
  `trap`-protected rename still loses it to a `SIGKILL`. No gate above depends on
  it. `/tmp/opencode` was verified restored afterwards (directory, 429 links,
  `docs-preview/site/index.html` present, `http://127.0.0.1:18980/` → 200).

## Notes for the lead

- Both red PR #64 checks were the `aarch64-gnu` PHP 8.6 leg; the arm64 *shape* was
  reproduced deterministically above on amd64 with the same apt, because the live
  mirror converged before the proof could be taken against it. Re-running the
  `aarch64-gnu` leg on an arm64 runner is the only way to observe that arch's own
  index fetch.
- The fallback cannot mask a real publish problem: 2c proves a bad `.gz` still
  fails, and 2a/2b prove the healthy case is unchanged.
- **No gate in Proof 3 depends on a bare `make fast-check` being green here.** The
  bare run's single red test is the pre-existing `SUN_LEN` fixture-path defect
  described in Proof 3; it is green under any shorter fixture root (measured), and
  every test this node can influence is green.
- `uvx --offline ruff check e2e/` (the whole directory) reports 4 **pre-existing**
  findings in files this node does not touch — `e2e/docs_deploy.py:39` (`re.M`,
  `re.S` → `FURB167`) and `e2e/universal_binary.py:289` (`PYI034`, `__enter__`
  should return `Self`). `make py-lint` covers only `python/` and `tools/`, so
  nothing gates `e2e/*.py`; scoped to the changed file, ruff is clean.
- `--progress=plain` is a BuildKit flag; both this host and CI's docker build with
  BuildKit enabled (proof 1 built through the driver with the flag in the argv),
  and a classic builder (`DOCKER_BUILDKIT=0`) would reject it with
  `unknown flag`.

## Follow-ups

- `crates/blastit/tests/common/project.rs:42-49` prefers `/tmp/opencode`, which
  pushes one `check --learn` fixture socket to 109 bytes (> the 107-byte
  `sun_path`) on any host where `/tmp/opencode` exists, so
  `learn_writes_one_grant_without_widening_a_view` is red for environmental
  reasons on such hosts (and needs a node owning `crates/blastit/tests/` to pick a
  shorter root or a `SUN_LEN`-aware fixture path).
- `make py-lint` covers `python` and `tools` only, so `e2e/*.py` formatting is not
  enforced by any gate; the driver edit keeps the file ruff-clean by hand.
- The same swallowed-build-error class this node fixed in `e2e/php_sapi_probe.py`
  still exists in three other drivers and one script: `e2e/run_apps.py:570`,
  `e2e/run_prod.py:1609` and `:1630` (`docker build -q` + `tail(stderr, 30)`,
  worse because `-q` suppresses the build output almost entirely) and
  `scripts/build-wheel.sh:359`. Each of the four carries its own `docker()`/
  `tail()` copy, so the follow-up node should extract one small shared helper
  (`e2e/_docker.py`: build argv, captured streams, omitting-marker tail) and have
  all four use it, rather than patching four divergent copies.
- If a future leg needs the *uncompressed* index fallback as well, the same
  `if ! …; then` shape with `Order::=` would be needed; no leg does today.
