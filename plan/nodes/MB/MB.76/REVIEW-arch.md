# MB.76 — architecture / security / test review

Scope reviewed: `git diff 8cbf0ef -- e2e/php-sapi/Dockerfile e2e/php_sapi_probe.py`
(+ the workspace callers: `Makefile:782`, `.github/workflows/php-sapi.yml`,
`nightly.yml`, `packages.yml`, `publish-pypi.yml`, `e2e/php-sapi/Dockerfile.dockerignore`,
`.gitignore`, `scripts/platform_matrix.py`). No Rust changed, so no `make coverage`
run applies; the Python driver is judged by its story E2E (`make php-sapi-e2e`).

## Answers to the five questions (evidence)

**1. Any unbounded network path left?**
- apt/apk/curl: all bounded (`Dockerfile:115-116,136-142,172,176,181,195-207,221`). ✔
- `apk search` (`Dockerfile:201`) is a local index read — measured: `apk search` in
  `--network none` returns instantly, rc 0.
- `make php-sapi` is a C compile; `COPY` (`Dockerfile:235-236`) is the local context. ✔
- **cargo** (`Dockerfile:248,249,293`) is only *implicitly* bounded: no
  `CARGO_HTTP_TIMEOUT`/`CARGO_NET_RETRY` is set, so it relies on cargo's documented
  defaults (30 s/request, `net.retry` 3, fail-fast on the first index error). That
  is a bound, but it is inherited from a third party, not stated here. `:249`
  additionally has **no `--locked`** (standalone workspace, no `Cargo.lock`
  committed), so it re-resolves against the live index on every build.
- **rustup toolchain payload** (`Dockerfile:224-226`) has no deadline at all —
  see MAJOR 2.
- Base-image pull (`Dockerfile:45`) is unbounded per request; only `BUILD_TIMEOUT`
  caps it (follow-up already records this). `Dockerfile:39-43` says "every network
  fetch below is bounded", which is not true of the `FROM`.

**2. Can a new retry mask a real failure?**
- `curl -f --retry 3` on a genuinely 404 URL: measured `curl: (22) … 404`, one
  request, no retry — a missing payload still fails immediately, and
  `sha256sum -c` (`Dockerfile:175`) still owns content correctness.
- `Acquire::Retries=3` cannot turn a *persistent* index/InRelease mismatch into a
  pass; it only widens the window in which a publishing mirror can recover — which
  is exactly what the MB.62 ladder already did deliberately. No masking.
- `BUILD_TIMEOUT` clamp: `min(PHP_SAPI_BUILD_TIMEOUT, PHP_SAPI_TIMEOUT)`
  (`php_sapi_probe.py:72`) is the right direction and *live* in CI (1200 < 1800;
  no workflow sets either). `timed_out` drives the failure, not `returncode`
  (`:348-357`), so a killed build is never reported as success. ✔
- A timeout *is* still reported as success on one path: `apk update` — MINOR 4.
- Inverted-direction risk (a bound that turns a slow-but-fine pass into a failure):
  MINOR 5 and MINOR 8.

**3. `Acquire::ForceIPv4` / `apk --timeout 60`.**
- `ForceIPv4` lives only in `APT_OPTS` (`Dockerfile:72`) and `APT_OPTS` is never
  expanded on the apk branch (`:188-208`, which uses `APK_OPTS` only). The Alpine
  path is untouched. ✔
- `apk add --timeout 60` / `apk update --timeout 60` parse on **both** generations:
  measured on `alpine:3.21` (apk-tools 2.14.13) and `alpine:3.23` (3.0.9) — no
  "unrecognized option", while a deliberately bogus option *is* rejected on both
  (so the flag is genuinely recognised, not silently swallowed).
- Fail-closed: total outage (`--network none`) → `apk update` rc **4**, and
  `apk add --no-cache` rc **1**. Partial outage → `apk update` rc **0** with
  "N unavailable" (see MINOR 4).

**4. MB.62 ladder behaviour.** Unchanged and still fail-closed: same three rungs,
same `sleep 5` / `sleep 15` spacing, same `exit 1`
(`Dockerfile:136-147`, diff shows only `${APT_OPTS}` added per rung, and rung 2
keeps its own `CompressionTypes::Order::=gz`). Nothing sets a global
`CompressionTypes::Order`. ✔

**5. Observability.** `BUILD_LOG`'s parent is created (`php_sapi_probe.py:341`),
`target/` is ignored (`.gitignore:1` `/target`) and `.dockerignore` keeps it out of
the build context, so no recursive copy; the file is closed by the `with` block on
every path (success included), and no secret is ever passed as a build arg. Two
gaps: the file is unreadable in exactly the failure mode that started this node
(MAJOR 1), and the `docker()` timeout evidence is wrong (MINOR 7).

---

## [MAJOR] 1 — the streamed log is lost in the failure mode the node was written for

`php_sapi_probe.py:343-346` sends the whole build stream to `BUILD_LOG` and nothing
to the process's own stdout. The tail is printed only when *our* clock fires at
`BUILD_TIMEOUT` (`:348-357`). But the two incidents this node exists for were
**killed** runs, and those still leave nothing readable:

- `.github/workflows/php-sapi.yml:52-54` sets `cancel-in-progress` for PRs, so a
  superseded push kills the job mid-build; the log file dies with the runner.
- the job bound is 40 min (`php-sapi.yml:114-119`) while the four post-build
  `docker run`s each still get the full `PHP_SAPI_TIMEOUT=1800 s`
  (`php_sapi_probe.py:206`); a hang there (or in the `rs-test-php-image` step)
  hits the job cap before anything prints.
- the only artifact upload is `target/proven-shims/*.so` (`php-sapi.yml:212-216`),
  so `target/php-sapi-build.log` is never retrievable either.

Result: live CI log silent for up to 20 min, and on a cancel/kill the evidence is
`BUILD_LOG` on a deleted runner — the same "zero output" symptom as before, moved
from the log to a file nobody can read.

**Fix (in-path, ~6 lines):** mirror the stream. `Popen(argv, stdout=PIPE,
stderr=STDOUT, text=True)` with a reader loop that writes each line to `log` *and*
`sys.stdout` (flushed), keeping the `wait(timeout=BUILD_TIMEOUT)` clock; on the
timeout path `process.kill()` + `communicate()` before the tail. Optionally add an
`if: always()` upload of `target/php-sapi-build.log` in each shim job (workflow
edit, out of this node's `paths` — file it as a follow-up if not done here).

## [MAJOR] 2 — the rustup toolchain download still has no deadline, only retries

`Dockerfile:224-226` sets `RUSTUP_MAX_RETRIES` and nothing else. That bounds
*retries after an error*, not the duration of one transfer. Measured against a
blackholed dist server, the connect path is fine — rustup fails in 37 s:

```
error: could not download file from 'http://10.255.255.1/dist/channel-rust-1.89.0.toml.sha256' …
  client error (Connect): tcp connect error: Operation timed out (os error 110)   (37 s total)
```

but a connection that is **established and then goes silent** mid-body (a CDN edge
that accepts and stops, a proxy that dribbles, a stalled TLS record) has no bound
in rustup. That stall is caught only by `BUILD_TIMEOUT=1200 s`
(`php_sapi_probe.py:72`) — 20 min per leg, six legs, which is the same
"one bare transfer eats the job" class MB.76 set out to kill, and the class the
node's own change item 4 requires to "fail with a message naming the download
rather than hanging".

**Fix:** wrap the init in a wall clock — `timeout 300 sh /tmp/rustup-init.sh -y
--profile minimal --default-toolchain "${RUST_TOOLCHAIN}"`, with the existing
`||` message extended to say it hit the 300 s deadline (a timeout exit 124 is a
non-zero, so the current message still fires). `timeout` is present on both libcs
(coreutils / busybox). Leave `RUSTUP_MAX_RETRIES=3` as the retry policy.

## [MINOR] 3 — the bound defaults now live in two places, and the duplicate wins

`Dockerfile:72-75` declares the shipped values as ARG defaults; `php_sapi_probe.py:83-93`
repeats them verbatim and `build()` always passes them (`:309-316`). Every real run
(`make php-sapi-e2e`, CI, `publish-pypi.yml`) overrides the Dockerfile, so the
Dockerfile copy is what a *hand* build uses and the Python copy is what is proven —
tighten one and CI silently keeps the other. Plumbing itself is sound (verified: a
space- and `=`-bearing value survives `--build-arg KEY=VALUE` intact, and an empty
value clears it).

**Fix:** let the Dockerfile own the defaults — `APT_OPTS =
os.environ.get("PHP_SAPI_APT_OPTS")` (no default) and append the `--build-arg`
only when it is not `None`. Same for the other three. A test still shrinks them by
setting the env var, which is all the node asked for.

## [MINOR] 4 — `apk --timeout 60` buys nothing, and a partial index outage is reported as success

Measured, same image, blackholed repository:

| | 2.14.13 (`alpine:3.21`) | 3.0.9 (`alpine:3.23`) |
|---|---|---|
| `--timeout 5` | aborts at **6 s** | aborts at **7 s** |
| no flag | aborts at **64 s** | aborts at **69 s** |
| all repos down (`--network none`) | `apk update` rc **4** | — |
| one repo down | `apk update` rc **0**, "2 unavailable" | rc **0**, "1 unavailable" |

So 60 s *is* apk-tools' own default: `Dockerfile:73,195-207` is documentation, not
a new bound (harmless — the musl legs were never the hang — but `Dockerfile:39-43`
and the RESULT overstate it). The behavioural gap is the rc **0**: when only some
repositories fail (`alpine-edge-testing` on the 8.6 musl leg is the realistic
case), `apk update` reports success and the leg fails one step later at
`apk add` with `unable to select packages: php86-dev (no such package)` — a
timeout/404 presented as "the package does not exist", the same misattribution
`MB.76` fixed for rustup.

**Fix:** keep `--timeout 60` (explicit beats default) but make the index step
honest — `apk update ${APK_OPTS} 2>&1 | tee /dev/stderr` and fail when the summary
line reports `N unavailable`, or assert `apk search -q "${PHP_DEV}"` is non-empty
before the first `apk add` (already the pattern used for `libclang` at `:201-206`,
so it costs one line and reads locally).

## [MINOR] 5 — `--max-time 120` caps total transfer, so it can fail a slow-but-working mirror

`Dockerfile:74` / `:172` / `:221`: `--max-time` is a wall clock over the whole
transfer, not a stall detector. A sury payload (~5 MB) therefore needs ≥ ~45 KB/s
or it is aborted — and because `--retry 3` restarts from zero on each attempt, a
uniformly slow mirror turns into a hard failure where it previously completed.
Worst case is also larger than the build budget: 7 payloads × 4 attempts × 120 s
≈ 56 min, i.e. the sury leg can only ever resolve as a `BUILD_TIMEOUT` (so the
tail, not the ladder, is what a human reads). Two smaller notes: `--retry` alone
does **not** retry `Connection refused` (needs `--retry-connrefused`), and
`--connect-timeout 15` already covers the blackholed-host case the node measured.

**Fix:** bound *stalls*, not throughput — `--connect-timeout 15 --speed-limit 1024
--speed-time 30 --retry 3 --retry-delay 2 --retry-connrefused --retry-max-time 300`
(drop or raise `--max-time`; `--retry-max-time` makes the retry window one number
instead of four).

## [MINOR] 6 — the now-reachable ladder sentence misdiagnoses a network failure

`Dockerfile:143` is unchanged, but MB.76 makes it reachable for a *timeout*: with
`Acquire::Retries=3` each bounded rung can take ~60 s, so after three rungs a
blackholed or 5xx mirror prints "failed 3 times **in 20s**: the distro mirror is
serving indexes its signed InRelease does not match — investigate, do not merely
rerun". Both clauses are now wrong for that cause (the window is minutes; a
timeout is not a signature mismatch), and it points the next investigator at the
wrong layer — the exact failure mode MB.76 fixed in the rustup message.
Related: `Dockerfile:68-71` asserts a v6 blackhole "makes `apt-get update` hang
with no deadline at all" and "used the entire 1800 s", which contradicts the
node's own measurement (apt's built-in ~30 s default ended at 28–37 s). The real
culprit was the unbounded `curl`/rustup transfer.

**Fix (wording only — rungs, spacing and `exit 1` stay exactly as MB.62 wrote
them):** "apt-get update failed on all three spaced attempts (<elapsed>s); the
mirror was unreachable within the per-attempt deadline, or is serving indexes its
signed InRelease does not match — investigate", and correct the `:68-71` comment.

## [MINOR] 7 — the `docker()`-timeout evidence is a vacuous oracle

`php_sapi_probe.py:197-217` turns `TimeoutExpired` into rc 124 with the partial
output; `as_text` (`:166-172`) is right that the exception carries **bytes**. But
the RESULT's evidence (b) shows `(no output)` and concludes "CPython's POSIX
`subprocess.run()` does not re-read the pipe after it kills a timed-out child".
That is not true — measured on this host, both a plain and a `docker run` child
that prints then hangs give `TimeoutExpired.stdout == b'before-hang\n'` (bytes,
even with `text=True`). So the code path that the acceptance claims to prove was
never exercised, and the follow-up "switching the short `docker run` calls to the
same writer is cosmetic" rests on a mis-quoted probe (`sh -c 'echo …; sleep 60'`
was evidently run as something else). Keep the code; fix the evidence so nobody
later deletes `as_text`'s bytes branch or `omission_tail` as dead.

## [MINOR] 8 — `BUILD_TIMEOUT=1200` has ~25 % headroom over the workflow's own worst case

`php_sapi_probe.py:72` defaults to 1200 s; the shim job's own comment
(`php-sapi.yml:114-119`) budgets ~8 min cold for packages + rustup and another
~6–8 min for the in-image `blastit`/suite compile, i.e. up to ~16 min on a 4-core
runner, and no workflow sets `PHP_SAPI_BUILD_TIMEOUT`, so CI gets 1200 s. On a
2-core (private-repo) or slower `ubuntu-24.04-arm` runner that margin disappears
and a healthy build is reported as `docker build did not finish within
BUILD_TIMEOUT=1200s` — a new red for the six legs this node is trying to un-red.

**Fix:** ship the ceiling as data where the job is declared, e.g.
`PHP_SAPI_BUILD_TIMEOUT: 2100` in the shim jobs (still under the 40-min job and
still under the 1800 s `PHP_SAPI_TIMEOUT`, so the build keeps reporting itself
with its tail), or record one measured CI cold build before keeping 1200.

---

Not findings (checked, deliberately fine): log path created and gitignored, `with`
close flushes on success, no secret reaches a `--build-arg` or the log, `curl -f`
no longer executes an empty rustup script, the apt flags never touch the apk
branch, `--retry` does not retry a 404, `sha256sum -c` still fails the sury leg
closed, and `--locked`/`--frozen` semantics on the two main cargo calls are
unchanged.

VERDICT: fix-required
