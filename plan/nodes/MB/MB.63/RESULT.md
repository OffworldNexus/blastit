# MB.63 — install sury PHP 8.6 from signature-verified, hash-pinned payloads

`STATUS: done`

Only `e2e/php-sapi/Dockerfile` changed: the `EXTRA_REPO=sury` branch of the single
package-install `RUN` layer. The Alpine branch, the plain Debian/Ubuntu
`apt-get install ca-certificates curl`, the `MB.62` three-attempt retry logic, the
tool-list install line and the `unknown EXTRA_REPO` guard keep their behaviour.

## What changed

- **Deleted** the `sources.list.d/sury.list` write. That index is exactly what
  breaks: the mirror serves `main/binary-<arch>/Packages*` objects that do not
  hash to the values its own signed `InRelease` lists, and `by-hash/` 404s, so apt
  has no content-addressed escape. `sury` is now **not** an apt source at all.
  Its measured numbers live once, in the block comment above the layer
  (2026-11-21, against sury's own `InRelease`).
- **Kept** the keyring path `/etc/apt/keyrings/sury.gpg`, and added
  `apt-get install -y --no-install-recommends gpgv xz-utils` from the distro.
- **New verification chain**, `set -eux` fatal throughout (no `|| true`, no
  `--allow-unauthenticated`):
  1. `curl` `dists/bookworm/InRelease` on **each** attempt, then
     `gpgv --keyring /etc/apt/keyrings/sury.gpg`. A bad signature is immediately
     fatal — the retry loop is never entered for it.
  2. For each of `Packages`, `Packages.gz`, `Packages.xz` on
     `main/binary-$(dpkg --print-architecture)/`, read the SHA256 the **signed**
     `InRelease` lists for that exact filename (one `awk` over its `SHA256:`
     section — Release format is `checksum size filename`, so the checksum is
     field 1) and accept the **first** variant whose raw bytes hash to it
     (`gunzip -c` / `xz -dc` for the index used afterwards). A variant the signed
     document stops listing, or that 404s, is skipped with a message so the
     variant that *would* match still gets its turn; the matching variant and its
     signed hash are logged.
  3. Bounded retry of exactly **3** attempts, `sleep 10` between (none after the
     third), for the index only; after three misses it exits 1 naming the measured
     diagnosis (sury serves indexes its signed `InRelease` does not match —
     upstream, not transient).
  4. One `awk` pass over blank-line-separated stanzas extracts `SHA256` +
     `Filename` for exactly `php$PHP_MINOR-{dev,curl,cli,common,readline}`,
     `libphp$PHP_MINOR-embed` and `php-common` (derived from the leg's
     `PHP_MINOR`, not hardcoded); anything missing fails with the package name on
     stderr.
  5. `curl` each payload into `/tmp/sury-debs/`, then `sha256sum -c` every one
     against the verified index value — a mismatch is fatal, no retry.
  6. `apt-get install -y --no-install-recommends /tmp/sury-debs/*.deb` plus the
     unchanged tool list (`gcc make pkg-config libc6-dev binutils ca-certificates
     curl bash tar xz-utils clang libclang-dev`); apt resolves the distro-side
     dependencies from bookworm. Fatal, no retry.
- No payload version or hash is hardcoded: the signed index is the pin. No new
  file.

### Review follow-up (second pass)

The verification chain was not redesigned. Fixed: (1) the `MB.62` retry comment
and its third-failure echo no longer blame `sury` or say "Debian" — it re-reads
**the distro's** indexes only (Debian and Ubuntu legs alike), and the sury
numbers appear once, in the sury block; the now-false "a missing package still
fails the `apt-get install` below" sentence is dropped (the index scan names a
missing package first) while the "nothing sets a global `CompressionTypes::Order`"
sentence stays; (2) the mapping `awk` derives its seven names from
`PHP_MINOR`, and the header says "no payload version or hash"; (3) an unlisted or
unfetchable index variant now falls through to the next variant instead of killing
attempt 1; (4) no `sleep 10` after the third attempt; (5) the header's
extra-repository clause covers both the Alpine repository and the sury
keyring/`InRelease` fetch. The comment edits are free, the echo edit changes the
`RUN` bytes (the layer re-runs).

## Evidence

### `make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1` — green

The `RUN` edit invalidated the layer, so the build really re-ran:

```
$ make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1
php-sapi-e2e: docker build --build-arg BASE=debian:bookworm-slim --build-arg PHP_MINOR=8.6 -t blastit-php-sapi-e2e:local
php-sapi-e2e: readelf -d libblastit_php86-x86_64-gnu.so
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)
  ok  php 8.6 abi 1: status 200, cpu_ns 11094253 <= wall_ns 11857988, exit()+fatal contained, next request served
php-sapi-e2e: probe — real View{egress:false} filter, egress denied (both layers)
  ok  php 8.6 abi 1: status 200, cpu_ns 11739279 <= wall_ns 15212858, exit()+fatal contained, next request served
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.6 abi 1: status 200, cpu_ns 9196885 <= wall_ns 11960689, exit()+fatal contained, next request served
EXIT=0
```

The probe driver captures `docker build` output and only replays it on failure, so
the layer's own lines were captured by running the **extracted RUN body verbatim**
(the `RUN`/continuation text, comments dropped the way the Dockerfile parser drops
them) in a scratch `debian:bookworm-slim` container with `EXTRA_REPO=sury`:

```
gpgv: Good signature from "DEB.SURY.ORG Automatic Signing Key <deb@sury.org>"
verified index variant '.xz': main/binary-amd64/Packages.xz sha256 6b5a7eae92b0ba2da4337f13861b069f77e68116a8a1389e05a97cc24c3dd85a (signed by sury's InRelease)
/tmp/sury-debs/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php-common_102+debsuryorg12.1+3_all.deb: OK
/tmp/sury-debs/php8.6-cli_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-common_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-curl_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-dev_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-readline_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
```

The matched variant moves with the publisher (`.gz` in the first pass, `.xz` here),
which is exactly why the choice is made from the signed hashes and not from a
fixed compression preference.

### The built image (kept by `PHP_SAPI_KEEP=1`)

```
$ docker run --rm blastit-php-sapi-e2e:local sh -c 'ls -l /usr/lib/libphp8.6.so; grep -rl sury /etc/apt/sources.list /etc/apt/sources.list.d || echo "no sury apt source"; dpkg-query -W -f="${Package} ${Version}\n" php8.6-cli libphp8.6-embed php-common'
-rw-r--r-- 1 root root 12603632 /usr/lib/libphp8.6.so
no sury apt source
libphp8.6-embed 8.6.0-3+debsuryorg12.1+1
php-common 2:102+debsuryorg12.1+3
php8.6-cli 8.6.0-3+debsuryorg12.1+1
```

### Fail-closed proofs (scratch container, same host)

```
=== baseline: signature verifies and the payload hash is the signed one ===
gpgv: Good signature from "DEB.SURY.ORG Automatic Signing Key <deb@sury.org>"
index main/binary-amd64/Packages: served 28649f87…0618, signed c02269a4…5014
index main/binary-amd64/Packages.gz: served 64e1a847…f7af, signed f36c7242…83ef
index main/binary-amd64/Packages.xz: served 6b5a7eae…d85a, signed 6b5a7eae…d85a
verified index variant '.xz'
/tmp/p/one.deb: OK
=== PROOF (a): a corrupted expected hash aborts (set -e fatal, exit 1) ===
/tmp/p/one.deb: FAILED
PROOF_A_EXIT=1
=== PROOF (b1): a tampered InRelease makes gpgv abort ===
gpgv: BAD signature from "DEB.SURY.ORG Automatic Signing Key <deb@sury.org>"
PROOF_B1_EXIT=1
=== PROOF (b2): an absent signature (empty InRelease) makes gpgv abort ===
gpgv: no signature found
PROOF_B2_EXIT=2
ALL_PROOFS_FAIL_CLOSED
```

The same run also exercises the new fall-through: `Packages` and `Packages.gz`
both mismatch their signed hashes, their messages are printed, and `.xz` is the
variant that is accepted. Proof (a) rotated one hex digit
(`tr '0123456789abcdef' '123456789abcdef0'`) of the expected SHA256 taken from the
verified index and re-ran `sha256sum -c` under `sh -ec`: the build step aborts.
Proofs (b1)/(b2) edited the cleartext of a real `InRelease` and emptied one:
`gpgv` refuses both, so no index or payload is ever consumed without a good
signature.

## Gates

```
$ uvx --offline ruff format --check e2e/
17 files already formatted
RUFF_EXIT=0
$ uv run --no-project tools/guard.py
guard: OK (3 staged/untracked file(s) checked)
GUARD_EXIT=0
$ uv run --no-project tools/plan.py validate
plan OK
PLAN_EXIT=0
```

`make fast-check` is not re-run for this follow-up (no Rust or Python change). Its
only red leg on this host is environment-only and unreachable from a Dockerfile
diff: `learn_writes_one_grant_without_widening_a_view` fails because the fixture
root is hardcoded to `/tmp/opencode` and created/preferred there
(`crates/blastit/tests/common/project.rs:43`), nesting `blastit check`'s Unix
socket at 109 bytes against `SUN_LEN`'s 107-byte maximum (100 bytes with the root
at `/tmp`; the same file already documents the hazard at
`e2e_check_fmt.rs:533`). guard, `cargo fmt --all -- --check`, clippy
`-D warnings` and `cargo doc` `-D warnings` are green. **The release gate must run
`make fast-check` with a short `TMPDIR`** (e.g. `TMPDIR=/tmp`) so that test sees a
short fixture root.

## Final RUN block (verbatim)

```dockerfile
RUN set -eux; \
    if command -v apt-get >/dev/null; then \
        apt-get update; \
        apt-get install -y --no-install-recommends ca-certificates curl; \
        if [ "${EXTRA_REPO}" = "sury" ]; then \
            mkdir -p /etc/apt/keyrings /tmp/sury-debs; \
            apt-get install -y --no-install-recommends gpgv xz-utils; \
            curl -fsSL https://packages.sury.org/php/apt.gpg \
                -o /etc/apt/keyrings/sury.gpg; \
        elif [ -n "${EXTRA_REPO}" ]; then \
            echo "unknown EXTRA_REPO '${EXTRA_REPO}' for this image" >&2; \
            exit 1; \
        fi; \
        # This re-reads the distro's indexes only (sury's payloads are installed
        # from signed hashes below, never from an apt source), and a distro mirror
        # can still serve an index object its own signed `InRelease` does not match
        # while it publishes — Debian and Ubuntu alike — and no compression choice
        # wins when the variants are stale. Hence three spaced attempts (20s of
        # sleeps): plain, the gz indexes, then plain after a longer pause, which
        # covers a publisher that rotates within that window. The third failure
        # fails the leg closed and says so in the log, so a repeat reads as
        # upstream rather than "rerun". Nothing sets a global
        # `CompressionTypes::Order` (that would hide real index mismatches).
        if ! apt-get update; then \
            echo "apt-get update failed: the mirror is probably publishing indexes; retrying with the gz variant" >&2; \
            sleep 5; \
            if ! apt-get -o Acquire::CompressionTypes::Order::=gz update; then \
                echo "apt-get update failed again; retrying plainly after a 15s pause" >&2; \
                sleep 15; \
                if ! apt-get update; then \
                    echo "apt-get update failed 3 times in 20s: the distro mirror is serving indexes its signed InRelease does not match (sury is not an apt source here) — investigate, do not merely rerun" >&2; \
                    exit 1; \
                fi; \
            fi; \
        fi; \
        if [ "${EXTRA_REPO}" = "sury" ]; then \
            arch=$(dpkg --print-architecture); \
            prefix="main/binary-${arch}"; \
            variant=""; \
            attempt=0; \
            while [ -z "${variant}" ]; do \
                attempt=$((attempt + 1)); \
                if [ "${attempt}" -gt 3 ]; then \
                    echo "sury serves ${prefix} indexes its signed InRelease does not match (3 attempts, 10s apart): that is upstream, not transient" >&2; \
                    exit 1; \
                fi; \
                curl -fsSL https://packages.sury.org/php/dists/bookworm/InRelease \
                    -o /tmp/sury-InRelease; \
                gpgv --keyring /etc/apt/keyrings/sury.gpg /tmp/sury-InRelease; \
                for compression in "" .gz .xz; do \
                    name="${prefix}/Packages${compression}"; \
                    signed=$(awk -v n="${name}" '/^SHA256:/ { s = 1; next } /^[A-Za-z]/ { s = 0 } s && $3 == n { print $1; exit }' /tmp/sury-InRelease); \
                    if [ -z "${signed}" ]; then \
                        echo "sury's InRelease lists no ${name}: trying the next variant" >&2; \
                        continue; \
                    fi; \
                    if ! curl -fsSL "https://packages.sury.org/php/dists/bookworm/${name}" \
                        -o /tmp/sury-index; then \
                        echo "cannot fetch ${name}: trying the next variant" >&2; \
                        continue; \
                    fi; \
                    served=$(sha256sum /tmp/sury-index | cut -d' ' -f1); \
                    if [ "${served}" != "${signed}" ]; then \
                        echo "sury ${name} hashes ${served}, its signed InRelease says ${signed}: trying the next variant" >&2; \
                        continue; \
                    fi; \
                    case "${compression}" in \
                        "") cp /tmp/sury-index /tmp/sury-Packages ;; \
                        .gz) gunzip -c /tmp/sury-index > /tmp/sury-Packages ;; \
                        .xz) xz -dc /tmp/sury-index > /tmp/sury-Packages ;; \
                    esac; \
                    variant="${compression:-plain}"; \
                    echo "verified index variant '${variant}': ${name} sha256 ${signed} (signed by sury's InRelease)" >&2; \
                    break; \
                done; \
                if [ -z "${variant}" ] && [ "${attempt}" -lt 3 ]; then sleep 10; fi; \
            done; \
            awk -v m="${PHP_MINOR}" 'BEGIN { split("php" m "-dev libphp" m "-embed php" m "-curl php" m "-cli php" m "-common php" m "-readline php-common", want, " "); for (i in want) need[want[i]] = 1 } \
                 /^Package: / { pkg = $2 } \
                 /^SHA256: / { sha = $2 } \
                 /^Filename: / { file = $2 } \
                 /^$/ { if (pkg in need) { parts = split(file, seg, "/"); print sha, seg[parts], file; delete need[pkg] } pkg = ""; sha = ""; file = "" } \
                 END { if (pkg in need) { parts = split(file, seg, "/"); print sha, seg[parts], file; delete need[pkg] } bad = 0; for (p in need) { print "the verified sury index has no " p >> "/dev/stderr"; bad = 1 } exit bad }' \
                /tmp/sury-Packages > /tmp/sury-mapping; \
            while read -r sha basename path; do \
                curl -fsSL "https://packages.sury.org/php/${path}" \
                    -o "/tmp/sury-debs/${basename}"; \
                echo "${sha}  /tmp/sury-debs/${basename}" >> /tmp/sury-SUMS; \
            done < /tmp/sury-mapping; \
            sha256sum -c /tmp/sury-SUMS; \
            apt-get install -y --no-install-recommends /tmp/sury-debs/*.deb \
                gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils \
                clang libclang-dev; \
            rm -rf /tmp/sury-*; \
        else \
            apt-get install -y --no-install-recommends \
                "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
                gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils \
                clang libclang-dev; \
        fi; \
        rm -rf /var/lib/apt/lists/*; \
    else \
        if [ "${EXTRA_REPO}" = "alpine-edge-testing" ]; then \
            echo "https://dl-cdn.alpinelinux.org/alpine/edge/testing" \
                >> /etc/apk/repositories; \
        elif [ -n "${EXTRA_REPO}" ]; then \
            echo "unknown EXTRA_REPO '${EXTRA_REPO}' for this image" >&2; \
            exit 1; \
        fi; \
        apk update; \
        apk add --no-cache \
            "${PHP_DEV}" "${PHP_EMBED}" "${PHP_CURL}" \
            gcc make pkgconf musl-dev binutils ca-certificates curl bash tar xz; \
        if [ "${PHP_SAPI_SUITES:-1}" = "1" ]; then \
            apk add --no-cache clang llvm-dev; \
            libclang=$(apk search -q 'clang*-libclang' \
                | grep -xE 'clang[0-9]+-libclang' | sort -V | tail -n 1); \
            [ -n "$libclang" ] || { \
                echo "no clang<version>-libclang package on this image (bindgen needs it)" >&2; \
                exit 1; \
            }; \
            apk add --no-cache "$libclang"; \
        fi; \
    fi
```

## Follow-ups

- `scripts/platform_matrix.py:329-331` still describes `sury` as "Ondřej Surý's
  signed APT repository for Debian (the key at `packages.sury.org/php/apt.gpg`,
  the `bookworm main` suite)", and the comment above it (`:325-333`) promises a leg
  needs "no second list" of packages. Both are stale: there is no apt suite any
  more, the keyring is fetched, and the package names are derived from
  `PHP_MINOR` inside the Dockerfile rather than listed per leg. Outside this
  node's `paths`.
- `docs/design/universal-distribution.md:121` still says the sury leg installs
  `/etc/apt/keyrings/sury.gpg` **and** `/etc/apt/sources.list.d/sury.list`; the
  second one is gone. Outside this node's `paths`.
- The keyring (`packages.sury.org/php/apt.gpg`) is fetched unauthenticated over
  TLS from the same origin as the payloads: the fingerprint assertion the old
  design doc describes (`docs/design/universal-distribution.md:121`, primary
  `15058500A0235D97F5D10063B188E2B695BD4743`) is not implemented anywhere, so a
  same-origin compromise would defeat both the signature check and the hashes.
  Pre-existing, and it needs a pinned fingerprint to close — worth a node of its
  own.
- `crates/blastit/tests/common/project.rs:43` hardcodes `/tmp/opencode` as the
  fixture root, which nests `blastit check`'s Unix socket past `SUN_LEN` and makes
  `learn_writes_one_grant_without_widening_a_view` fail on any host where that
  directory exists. A `SUN_LEN`-aware (short) fixture root, or a per-test short
  `TMPDIR` like the dry-run test already uses, would make `rs-test` green without
  the `TMPDIR=/tmp` workaround. Outside this node's `paths`.
- `e2e/php_sapi_probe.py` captures `docker build` output and prints it only on
  failure, so a green leg leaves no build log in CI. Showing the package layer's
  `verified index variant` / `: OK` lines on success (or under `--verbose`) would
  make an upstream index rotation diagnosable from the run log alone. Outside this
  node's `paths`.
