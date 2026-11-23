# MB.76 — bound every network step in the php-sapi image

Two consecutive CI runs of the six `*-gnu` shim jobs sat inside `docker build`
for the whole 1800 s `PHP_SAPI_TIMEOUT` (`subprocess.TimeoutExpired`) with
**zero build output**, while `*-musl` passed and a healthy build is ~2 min. No
acquire had a deadline of its own, so one stalled fetch ate the job and the
`MB.62` retry ladder was never reached.

## What changed

### `e2e/php-sapi/Dockerfile` (the fetch bounds are `ARG`s, so a test can shrink them)

```dockerfile
ARG APT_OPTS="-o Acquire::http::Timeout=20 -o Acquire::https::Timeout=20 -o Acquire::Retries=3 -o Acquire::ForceIPv4=true"
ARG APK_OPTS="--timeout 60"
ARG CURL_OPTS="--connect-timeout 15 --max-time 300 --retry 3 --retry-delay 2 --retry-connrefused"
ARG RUSTUP_MAX_RETRIES=3
```

- **apt**: every call now carries `${APT_OPTS}` — the bootstrap `update`/`install
  of `ca-certificates curl`, all three rungs of the `MB.62` ladder (the gz rung
  keeps its own `-o Acquire::CompressionTypes::Order::=gz` alongside), the sury
  pinned-payload `install`, and the stock `install`. The ladder itself is
  byte-for-byte the same three spaced attempts.
- **curl**: the seven sury `.deb` payloads and the rustup script all use
  `${CURL_OPTS}` (the sury fetch dropped its now-redundant bare `--retry 3`).
- **rustup**: `curl … -f -o /tmp/rustup-init.sh` then run it, never
  `curl … | bash` — a failed download used to execute an empty script and exit 0
  (the only symptom was `cargo: not found` two `RUN`s later). Both the fetch and
  `rustup-init` fail the `RUN` with a message naming the URL, and
  `RUSTUP_MAX_RETRIES` bounds rustup's own toolchain downloads.
- **apk** (musl legs): `apk update` and every `apk add` carry `${APK_OPTS}`
  (`--timeout 60`). Verified against the real images: apk-tools 2.14.13
  (`alpine:3.21`), 3.0.7 (`alpine:edge`) and 3.0.9 (`alpine:3.23`) all list
  `--timeout TIME` on both `update` and `add`, so `--timeout` is genuinely
  supported and no substitute was needed. No apt flags were added to this branch;
  `apk search` (a local index read) is untouched.

### `e2e/php_sapi_probe.py`

- `BUILD_TIMEOUT` (`PHP_SAPI_BUILD_TIMEOUT`, default **1500 s** — 25 min against a
  ~16 min cold build with the suites) is the build's
  own ceiling, `min()`-clamped to `PHP_SAPI_TIMEOUT` so a smaller outer backstop
  can never make the inner bound meaningless; `PHP_SAPI_TIMEOUT` stays the outer
  backstop only.
- `build()` streams `docker build` output into `BUILD_LOG`
  (`PHP_SAPI_BUILD_LOG`, default `target/php-sapi-build.log`) via `Popen` instead
  of `capture_output=True` + `run()`: the output is on disk *while* the build
  runs, so a killed job still leaves evidence. A stall is reported as a **build**
  timeout with that log's 120-line tail (same shape as the `MB.60` failed-build
  report, factored into `omission_tail`).
- `docker()` no longer lets `subprocess.TimeoutExpired` escape: it becomes a
  return code `124` result whose output is the message naming the command and the
  bound plus the captured tail, so every give-up path (`image_tuple`,
  `check_linkage`, `run_probe`) prints something.
- The bounds are passed explicitly as `--build-arg`s (`APT_OPTS`, `APK_OPTS`,
  `CURL_OPTS`, `RUSTUP_MAX_RETRIES`, env-overridable as `PHP_SAPI_*_OPTS`), with
  the shipped defaults still in the Dockerfile.

**Chosen shape**: streaming via `Popen` (the preferred option). It did not fight
the existing structure — one helper, `run_build()`, and the failure report became
`refusal_note()` reading `BUILD_LOG`.

## Evidence

### (a) Fail-fast proof — blackholed address `10.255.255.1`

`blackhole.test` mapped to a blackholed IPv4 in `/etc/hosts`, debian:trixie-slim,
shipped flags, wall clock by `date +%s`:

```
$ docker run --rm debian:trixie-slim sh -c '…'
=== (2a) curl NO bounds: still hanging at 30s (coreutils kills it) ===
elapsed=30s rc=124 (124 = still hanging when killed)
=== (2b) curl with the shipped CURL_OPTS ===
curl: (28) Connection timed out after 15001 milliseconds
curl: (28) Connection timed out after 15002 milliseconds
elapsed=66s rc=0
=== (2c) apt-get update with the shipped APT_OPTS on the blackholed mirror ===
W: Failed to fetch http://blackhole.test/debian/dists/trixie/InRelease  Unable to connect to blackhole.test:80:
elapsed=28s
```

So: an unbounded `curl` was still hanging when killed at 30 s, while the shipped
`--connect-timeout 15` ends each attempt at exactly 15 s and the whole
`--retry 3 --retry-delay 2` call is a bounded 66 s. `apt-get update` with
`Acquire::http::Timeout=20` gave up at 28 s. Neither is anywhere near the 1800 s
job. The healthy side of the same container (a real `apt-get update` +
`install curl` with `${APT_OPTS}`) took 9 s, so the flags do not slow a good
mirror.

An unbounded `apt-get update` in the same container hit apt's own 30 s default
and finished at 37 s, which is the useful correction to the node's premise: the
1800 s stall was **not** apt (it has a 30 s default in this image), it was the
unbounded `curl`/rustup transfer — which is exactly what (2a) reproduces.

### (b) Deliberately stalled build — reported with its tail, inside `BUILD_TIMEOUT`

A scratch `Dockerfile` (`FROM debian:trixie-slim` + `RUN echo …; sleep 600`) run
through the shipped `run_build()` with `PHP_SAPI_BUILD_TIMEOUT=15`:

```
BUILD_TIMEOUT=15s PHP_SAPI_TIMEOUT=600s log=/tmp/opencode/mb76-stall/build.log
=== (3) a stalled `docker build` (RUN sleep 600), BUILD_TIMEOUT=15s ===
ok=False
  docker build did not finish within BUILD_TIMEOUT=15s (PHP_SAPI_TIMEOUT=600s is only the outer backstop); killed it; full log: /tmp/opencode/mb76-stall/build.log
  #4 CACHED
  #5 [2/2] RUN echo starting a deliberately stalled step; sleep 600
  #5 0.180 starting a deliberately stalled step
```

and a `docker run` that outlives `PHP_SAPI_TIMEOUT`:

```
returncode=124
  docker run --rm alpine:3.21 sh -c echo before-hang; sleep 60 did not finish within PHP_SAPI_TIMEOUT=6s: killed it
  (no output)
```

`(no output)` is honest rather than a regression: CPython's POSIX
`subprocess.run()` does not re-read the pipe after it kills a timed-out child, so
partial output is lost there. The command and its bound are still named; the path
that actually stalled (the build) streams to a file and keeps everything.

### (c) `MB.62` ladder unchanged and still fires

The three attempts (plain, gz, plain after 15 s) are quoted verbatim from the
shipped Dockerfile and were executed unchanged in `debian:trixie-slim` against a
hard-failing index:

```
        if ! apt-get ${APT_OPTS} update; then
            echo "apt-get update failed: the mirror is probably publishing indexes; retrying with the gz variant" >&2;
            sleep 5;
            if ! apt-get ${APT_OPTS} -o Acquire::CompressionTypes::Order::=gz update; then
                echo "apt-get update failed again; retrying plainly after a 15s pause" >&2;
                sleep 15;
                if ! apt-get ${APT_OPTS} update; then
                    echo "apt-get update failed on all three spaced attempts: the mirror was unreachable within the per-attempt deadline (a stall) or is serving indexes its signed InRelease does not match — investigate, do not merely rerun" >&2;
                    exit 1;
                fi;
            fi;
        fi;
```
```
E: Some index files failed to download. They have been ignored, or old ones used instead.
apt-get update failed on all three spaced attempts: the mirror was unreachable within the per-attempt deadline (a stall) or is serving indexes its signed InRelease does not match — investigate, do not merely rerun
ladder rc=1 after 20s
```

Same rungs, same spacing (20 s of `sleep`s), same `exit 1`; only the
per-attempt deadline inside each rung is new, and the final sentence names both
causes because a bounded rung can now fail on a stall as well as on an index
mismatch (review MINOR 6).

### (d) Healthy build

Tuple read from `scripts/platform_matrix.py --leg shims --minor 8.4 --libc gnu`
(`base=debian:trixie-slim dev=php8.4-dev embed=libphp8.4-embed curl=php8.4-curl
libphp=/usr/lib/libphp8.4.so suites=true`), not guessed. `make php-sapi-e2e
PHP_MINOR=8.4` — the full leg, suites compiled in, all four probe runs:

```
php-sapi-e2e: docker build --build-arg BASE=debian:trixie-slim --build-arg PHP_MINOR=8.4 -t blastit-php-sapi-e2e:local (build <= 1200s, log /tmp/opencode/mb76-healthy-build.log)
php-sapi-e2e: readelf -d libblastit_php84-x86_64-gnu.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.4 abi 1: status 200, cpu_ns 10748913 <= wall_ns 13985526, exit()+fatal contained, next request served
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.4 abi 1: status 200, cpu_ns 12147883 <= wall_ns 12273038, exit()+fatal contained, next request served
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.4 abi 1: status 200, cpu_ns 11971500 <= wall_ns 12286782, exit()+fatal contained, next request served
EXIT=0
```

BuildKit's own step timings from the streamed log (131 `Get:` lines, i.e. a real
mirror round trip, no retries needed): `#5 DONE 58.9s` (packages, with the new
`${APT_OPTS}`), `#6 DONE 15.7s` (rustup via `curl -f -o`, `default toolchain set
to stable-x86_64-unknown-linux-gnu`), `#10 DONE 11.4s` (shim + probe),
`#11 DONE 51.0s` (protected suites), `#12 DONE 127.9s` (image export) —
~4.4 min end to end, well inside `BUILD_TIMEOUT=1200` and the job's 1800 s.

### (e) `make fast-check` / `make docs-check`

Both green on this tree, and run twice with the same result; the last run is the
tree of record (63 `test result: ok` lines, zero failures). Wall-clock suffixes
are dropped because they differ by milliseconds per run:

```
$ make fast-check
…
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
FASTCHECK_EXIT=0

$ make docs-check
uv run --no-project scripts/gen_language_blocks.py --check
uvx --offline zensical build --strict
Build started
No issues found
DOCSCHECK_EXIT=0
```

`e2e/` is outside `py-lint` (`ruff check python tools`), so the driver was also
formatted/checked by hand: `uvx --offline ruff format --check e2e/php_sapi_probe.py`
→ `1 file already formatted`, and `python -m py_compile` → ok. (The two
`# noqa: E402` RUF100 notices predate this node and are untouched.)

## Follow-ups

- **`cargo` needs no new bound here**: cargo's `http.timeout` defaults to 30 s per
  request and `net.retry` to 3, so the in-image `cargo build`/`cargo test` are
  already bounded per request (they are also not what hung: the stalled step was
  a bare transfer). If a future run stalls inside a `cargo` fetch, the lever is
  `CARGO_HTTP_TIMEOUT`/`CARGO_NET_RETRY`, not this node.
- **No `uv` exists inside this image**, so bounding `uv` is not applicable here
  (`uv run --no-project e2e/php_sapi_probe.py` is the *host* side; `BUILD_TIMEOUT`
  and `PHP_SAPI_TIMEOUT` are its bounds). Verified, not assumed:
  `docker run --rm debian:trixie-slim sh -c 'command -v uv || echo "uv: absent"'`
  → `uv: absent` (and `python3: absent`; the image is Debian + apt packages +
  rustup).
- **rustup has no download deadline knob**, only `RUSTUP_MAX_RETRIES`; a
  connection that is established and then goes silent still relies on the
  toolchain server timing out. Adding `RUSTUP_DIST_SERVER` retries to a
  proxy/CDN is the next lever if that ever happens.
- **`docker pull` of the base image is unbounded** by this file (`docker build`'s
  own resolve). It was 0.0 s cached in CI and is not what stalled; a
  `--pull`-less build makes it a one-time cost.
- The probe's `docker()` timeout keeps no partial output on POSIX (see (b)); the
  build path already streams, so switching the short `docker run` calls to the
  same writer is a cosmetic improvement if it ever matters.

## Review fixes (`REVIEW-arch.md`)

Applied exactly these five; the apk `--timeout 60`, the duplicated defaults, the
`docker()` 124 shape and everything else are untouched.

1. **[MAJOR] the build stream is teed, not just filed.** `run_build()` pipes
   `docker build` through `mirror_build()`, which writes every line to
   `BUILD_LOG` *and* to the driver's own stdout, flushed — so the job log a
   cancelled CI run actually keeps now names the step it died in. `Popen` runs in
   its own session and the `BUILD_TIMEOUT` kill is an `os.killpg(…, SIGKILL)`, so
   buildx's plugin child cannot hold the pipe open and strand the reader; the
   reader is joined before the `with` closes the file, and the 120-line tail on
   timeout is unchanged.
2. **[MAJOR] rustup has a wall clock.** `timeout 600 sh /tmp/rustup-init.sh …`,
   with the `||` message naming the rustup download *and* the deadline, so a
   silent mid-transfer stall is not reported as a bare `timeout: exit 124`.
   `RUSTUP_MAX_RETRIES=3` stays the retry policy.
3. **[MINOR] curl** — `${CURL_OPTS}` is now `--connect-timeout 15 --max-time 300
   --retry 3 --retry-delay 2 --retry-connrefused`, in the Dockerfile ARG and in
   the probe's copy, so a refused port retries too and a genuinely slow mirror is
   not a false failure (the per-attempt bound and `BUILD_TIMEOUT` still govern).
4. **[MINOR] `BUILD_TIMEOUT` 1200 → 1500 s**, `min()` clamp and comment kept (the
   comment now reads "25 min against a ~16 min cold build with the suites").
5. **[MINOR] the ladder's last sentence** names both causes — a stall inside the
   per-attempt deadline *or* an index/`InRelease` mismatch — with the three rungs,
   their spacing and the `exit 1` exactly as `MB.62` wrote them.

Re-proved after the change:

- **stalled scratch build** (`FROM debian:trixie-slim` + `RUN … sleep 600`,
  `PHP_SAPI_BUILD_TIMEOUT=15`), the shipped `run_build()`. The stream is teed
  live and the tail still comes from `BUILD_LOG`:

  ```
  #5 [2/2] RUN echo starting a deliberately stalled step; sleep 600
  #5 0.257 starting a deliberately stalled step
  ok=False
    docker build did not finish within BUILD_TIMEOUT=15s (PHP_SAPI_TIMEOUT=600s is only the outer backstop); killed it; full log: /tmp/opencode/mb76-stall/build.log
    …
    #5 0.257 starting a deliberately stalled step
  ```

- **the resumed ladder** (shipped bytes, `debian:trixie-slim`, failing index):
  three rungs, corrected final sentence, `ladder rc=1 after 20s`.

- **healthy leg still green**: `make php-sapi-e2e PHP_MINOR=8.4` → `EXIT=0`,
  `readelf` + all four probe runs `ok` (php 8.4 abi 1, status 200, cpu_ns ≤
  wall_ns, `exit()`+fatal contained, next request served, egress denied in all
  three layers), step totals `#5 45.7s #6 17.5s #10 13.1s #11 69.6s #12 159.1s`
  ≈ 5 min. Build cache: only the `FROM` metadata and the unchanged `WORKDIR`/
  `COPY` steps were reused — the two edited `RUN` lines invalidate steps 5–11, so
  packages, rustup (now under `timeout 600`), the shim/probe and the suites all
  re-ran from the network and a cold-ish compile.
- **the tee in a real run**: the driver's own stdout carried **1259** docker step
  lines (1419 lines total) where the pre-review run's log held 21 — and the build
  header line now prints `build <= 1500s`.

`make fast-check` and `make docs-check` were re-run once each on the final tree
(as before both green: 63 `test result: ok`, `No issues found`); the transcripts
in (a)–(d) remain records of the pre-review cut, whose only differences are the
`--max-time` / `BUILD_TIMEOUT` numbers quoted above.

One evidence correction (review MINOR 7): `docker()`'s timeout *does* report the
command and its bound, but on this host the `docker run` leg yields
`TimeoutExpired.stdout is None` — the child's `before-hang` was still in the pipe
buffer when the deadline fired and surfaced on the terminal after the kill, which
is why (b) shows `(no output)`. `as_text`'s bytes branch is still needed and
stays: `subprocess.run` on a plain child does carry it (measured:
`TimeoutExpired.stdout == b'before-hang\n'`).

## Musl leg + docstring (final follow-up)

- The module docstring now says `BUILD_TIMEOUT` (`PHP_SAPI_BUILD_TIMEOUT`,
  **25 min**), matching the 1500 s constant; nothing else in that paragraph moved.
- The apk branch — the one changed line nothing had exercised yet — is proven on
  the leg `scripts/platform_matrix.py --leg shims --minor 8.6 --libc musl --arch
  x86_64` declares (`base=alpine:edge dev=php86-dev embed=php86-embed
  curl=php86-curl libphp=/usr/lib/php86/libphp.so
  extra_repo=alpine-edge-testing suites=true`), not a guessed minor:

  ```
  php-sapi-e2e: docker build --build-arg BASE=alpine:edge --build-arg PHP_MINOR=8.6 -t blastit-php-sapi-e2e:local (build <= 1500s, …)
  php-sapi-e2e: readelf -d libblastit_php86-x86_64-musl.so
    ok  DT_NEEDED: libc.musl-x86_64.so.1 (no libphp)
  php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
    ok  php 8.6 abi 1: status 200, cpu_ns 9818586 <= wall_ns 13279438, exit()+fatal contained, next request served
  … two more runs ok …
  EXIT=0
  ```

  The streamed build log shows apk itself succeeding under the shipped order
  (`#5 1.067 OK: 35807 distinct packages available`, `#5 2.124 (12/60)
  Installing ca-certificates (20261020-r0)`, `edge/testing` appended, step
  totals `#5 DONE 17.9s` packages / `#6 DONE 35.0s` rustup — `default toolchain
  set to stable-x86_64-unknown-linux-musl` under `timeout 600` / `#11 DONE 82.7s`
  suites / `#12 DONE 117.8s` export).
- Flag ordering also proved directly on `alpine:edge` (apk-tools 3.0.7), including
  that the option is really parsed rather than swallowed:

  ```
  $ docker run --rm alpine:edge sh -c 'apk update --timeout 60 | tail -2; apk add --no-cache --timeout 60 ca-certificates; echo "add rc=$?"'
  OK: 28531 distinct packages available
  (1/1) Installing ca-certificates (20261020-r0)
  OK: 8412 KiB in 17 packages
  add rc=0

  $ docker run --rm alpine:edge sh -c 'apk add --no-cache --timeout 60 --no-such-flag ca-certificates; echo "bogus rc=$?"'
  ERROR: command line: unrecognized option 'no-such-flag'
  bogus rc=1
  ```
