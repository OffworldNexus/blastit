# MB.62 — Bounded spaced apt retries for sury's published-index race

## Goal

Replace `MB.60`'s single gz fallback in `e2e/php-sapi/Dockerfile` with a bounded, spaced
escalation (plain → `.gz` → plain after 15 s) that fails the layer non-zero on a third
failure, and document the second, wider incident in the layer comment.

## Change

One site: the post-`EXTRA_REPO` `apt-get update` in `e2e/php-sapi/Dockerfile`.

- Three-attempt ladder per the review round: fail 1 plain, fail 2
  `-o Acquire::CompressionTypes::Order::=gz`, fail 3 plain after `sleep 5` + `sleep 15` (20 s in
  total), then an explicit `exit 1` after a diagnosis line, so a third failure is unmistakable in
  the CI log tail instead of reading as "rerun".
- Layer comment keeps `MB.60`'s measurement (`InRelease` 10:18Z, `.xz` 291892 vs served 290956,
  "the `.gz` index was current then") and adds the 13:22Z case (`.xz` 292780 != 292160 *and*
  `.gz` 520904 != 520306, against an `InRelease` already an hour old), the measured >75 min
  non-rotation and the later inversion, so a repeat reads as upstream rather than transient.
- No global `CompressionTypes::Order`, Debian-only first update untouched, Alpine branch
  untouched, `e2e/php_sapi_probe.py` untouched.

## Acceptance

### 1. Shell-level ordering proof (`debian:bookworm-slim`, stub `apt-get` first on `PATH`)

Re-run against the **three-attempt ladder** now in the Dockerfile. The block is extracted
**verbatim** (8-space outer `fi;` terminates the extract), wrapped in `set -eu`, and run against a
stub `apt-get` that logs every call and fails on the first `MB62_FAIL_CALLS` calls.

```
=== extracted block ===
set -eu
if ! apt-get update; then
    echo "apt-get update failed: the mirror is probably publishing indexes; retrying with the gz variant" >&2;
    sleep 5;
    if ! apt-get -o Acquire::CompressionTypes::Order::=gz update; then
        echo "apt-get update failed again; retrying plainly after a 15s pause" >&2;
        sleep 15;
        if ! apt-get update; then
            echo "apt-get update failed 3 times in 20s: the sury mirror is serving indexes its signed InRelease does not match, which is upstream (not a transient publish) — investigate, do not merely rerun" >&2;
            exit 1;
        fi;
    fi;
fi;
=== case A: fails on calls 1-2, succeeds on call 3 ===
E: stub apt-get failure on call 1
apt-get update failed: the mirror is probably publishing indexes; retrying with the gz variant
E: stub apt-get failure on call 2
apt-get update failed again; retrying plainly after a 15s pause
--- case A: fail_first=2 status=0 elapsed=20s ---
CALL 1: apt-get update
CALL 2: apt-get -o Acquire::CompressionTypes::Order::=gz update
CALL 3: apt-get update
=== case B: every call fails ===
E: stub apt-get failure on call 3
apt-get update failed 3 times in 20s: the sury mirror is serving indexes its signed InRelease
does not match, which is upstream (not a transient publish) — investigate, do not merely rerun
--- case B: fail_first=999 status=1 elapsed=20s ---
CALL 1: apt-get update
CALL 2: apt-get -o Acquire::CompressionTypes::Order::=gz update
CALL 3: apt-get update
```

Attempt order is exactly plain → gz → plain, the two sleeps add up to the measured 20 s, a
third success returns 0, and a third failure exits **1** (the explicit `exit 1`) with the
diagnosis line above it in stderr, so the CI tail carries the conclusion, not just the nausea.

Syntax: `sh -n` and `bash -n` both OK on the extracted block, and on the whole RUN layer body
joined the way Docker joins continuations (comment lines dropped, `\`+newline folded:
1 logical line, 2654 bytes) — `sh -n layer body: OK`, `bash -n layer body: OK`.

### 2. Container-local arm64-index reproduction of today's upstream state

`debian:bookworm-slim`, `dpkg --add-architecture arm64`, sury keyring + `[arch=arm64]`
`https://packages.sury.org/php/ bookworm main`, then the unmodified `apt-get update`.

```
=== declared sizes in the signed InRelease (arm64) ===
 805f000f714ead49ec0af4cd140b6b53 520306 main/binary-arm64/Packages.gz
 7ce67385f42100031712783f887f89f8 292160 main/binary-arm64/Packages.xz
=== apt-get update (unmodified, as the Dockerfile runs it) ===
E: Failed to fetch .../main/binary-arm64/Packages.xz  File has unexpected size
   (292780 != 292160). Mirror sync in progress? [IP: 151.101.135.52 443]
   Release file created at: Sat, 21 Nov 2026 12:16:47 +0000
E: Some index files failed to download. They have been ignored, or old ones used instead.
apt-get update exit=100
```

So today's mirror is **not** healthy: it reproduces the incident byte-for-byte (served `.xz`
292780 against an `InRelease` that declares 292160). The escalation itself, run verbatim against
that live state:

```
+ apt-get update
  File has unexpected size (292780 != 292160). ...
+ echo apt-get update failed: the mirror is probably publishing indexes; retrying with the gz variant
+ sleep 5
+ apt-get -o Acquire::CompressionTypes::Order::=gz update
  File has unexpected size (520904 != 520306). ...
+ echo apt-get update failed again: still publishing; retrying plainly after a longer pause
+ sleep 15
+ apt-get update
  File has unexpected size (292780 != 292160). ...
escalation block exit=100
```

It fails closed in 20 s instead of hanging, which is the specified behaviour — but it does **not**
recover, because the upstream state is worse than "mid-publish". Six plain probes, 30 s apart:

```
T+0sz   probe 1: exit=100 | Date: Sat, 21 Nov 2026 12:16:47 UTC | 292780 != 292160
T+30sz  probe 2: exit=100 | Date: Sat, 21 Nov 2026 12:16:47 UTC | 292780 != 292160
T+60sz  probe 3: exit=100 | Date: Sat, 21 Nov 2026 12:16:47 UTC | 292780 != 292160
T+90sz  probe 4: exit=100 | Date: Sat, 21 Nov 2026 12:16:47 UTC | 292780 != 292160
T+120sz probe 5: exit=100 | Date: Sat, 21 Nov 2026 13:32:01 UTC | 292780 != 294336
T+150sz probe 6: exit=100 | Date: Sat, 21 Nov 2026 13:32:01 UTC | 292780 != 294336
Sat Nov 21 13:33:46 UTC 2026
```

The signature finally rotated at 13:32:01Z, yet the CDN kept serving `.xz` 292780 where the new
`InRelease` declares 294336. The mismatch therefore ran from 12:16Z to at least 13:33Z (**> 75
minutes**), and in the later window the served object is *older* than the signature — the inverse
of the 13:22Z incident. That is a durable upstream/CDN inconsistency, not a seconds-to-minutes
publish window, so no bounded retry of this shape can be expected to save the `aarch64-gnu` legs
while it lasts.

### 3. `ruff` on `e2e/`

```
$ uvx --offline ruff format --check e2e/
17 files already formatted
$ uvx --offline ruff check e2e/          # ruff 0.17.6
Found 4 errors.  (all pre-existing, none in this node's paths:
  e2e/docs_deploy.py FURB167 re.M/re.S ×3, e2e/universal_binary.py PYI034 __enter__ -> Self)
```

`e2e/` is not in the repo's ruff gate (`make py-lint` = `python tools`), and this node adds no
Python at all, so the findings are unchanged-latent rather than introduced. `make py-lint` passes
(inside `fast-check`). Logged as a follow-up rather than fixed, since `e2e/php-sapi/Dockerfile` is
this node's only path.

### 4. `make fast-check`

```
$ make fast-check
...
assertion `left == right` failed: blastit check:
/tmp/opencode/blastit-e2e-dev-1526934-learn-write/tmp/blastit-check-1000-1527101-528567033-0/pg/.s.PGSQL.5432:
path must be shorter than SUN_LEN
test result: FAILED. 23 passed; 1 failed
error: test failed, to rerun pass `-p blastit --test e2e_check_fmt`
```

One failure, and it is a host-path-length artefact, not this change: `scratch_root()` in
`crates/blastit/tests/common/project.rs` prefers the hard-coded `/tmp/opencode`, which makes the
fixture's PostgreSQL socket path 110 bytes against `sun_path[108]`. Scoped it precisely:

```
$ cargo test --workspace --no-fail-fast
... only failing test name: learn_writes_one_grant_without_widening_a_view
   (every other target ok; 25 empty targets, 10/10, 13, 17, 18, 19, 1×7 … all ok)

$ unshare -Urm sh -c 'mount -t tmpfs none /tmp; touch /tmp/opencode; cargo test -p blastit --test e2e_check_fmt'
test learn_writes_one_grant_without_widening_a_view ... ok
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Same commit, same test: red only where the scratch root is the long `/tmp/opencode`, green (24/24)
where the fixture falls back to `/tmp`. Nothing Rust changed in this node.

### 5. `make php-sapi-e2e` (extra: the healthy path is untouched)

```
$ make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1
make-exit=0        # zero "retrying with the gz" / "still publishing" lines
  ok  php 8.6 abi 1: status 200, cpu_ns 12351346 <= wall_ns 13153438, exit()+fatal contained, next request served
```

The amd64/gnu leg never enters the fallback (sury's amd64 indexes are consistent today), so the
escalation is invisible on the normal path and costs nothing when `apt-get update` succeeds.

## Follow-ups

1. **`scratch_root()` must not hard-code `/tmp/opencode`** (`crates/blastit/tests/common/project.rs`).
   It makes `e2e_check_fmt::learn_writes_one_grant_without_widening_a_view` fail on any host with
   that directory (socket path 110 > `SUN_LEN` 108) and is the only red in `make fast-check` here.
   Outside this node's paths — needs its own node (prefer `temp_dir()` plus a short per-case dir).
2. **Today's sury/Fastly mismatch is durable (> 75 min, and later inverted)**, so the 20 s bound
   cannot rescue the `aarch64-gnu` legs while it lasts. Worth a human/upstream note and a decision
   on whether a red leg during such an outage should be marked as upstream, rather than widening
   these retries — the layer already fails closed with the real `unexpected size` numbers visible.
3. **`e2e/` is outside `make py-lint`.** Four latent findings (`FURB167` ×3, `PYI034`) would
   surface if `e2e/` were ever added to the ruff scope; not touched here.
