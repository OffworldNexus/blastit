# MB.64 — Pin sury PHP 8.6 payloads per arch (sury index is durably inconsistent)

**Status:** done. Only `e2e/php-sapi/Dockerfile` changed: the `EXTRA_REPO=sury`
branch of the single package `RUN`, one `PHP_MINOR` guard, and three comment
edits (provenance, the `ARG EXTRA_REPO` doc block, the MB.62 clause).

## Goal

Stop consulting sury's (durably inconsistent) index at build time. The `.deb`
payloads are immutable and hash-verifiable, so install seven pinned payloads per
architecture and let `sha256sum -c` be the gate. `gpgv`, the keyring, the
`InRelease` fetch and the variant trial are gone; sury is not an apt source at
all, and exactly one sury path remains.

## Final sury block (verbatim, `e2e/php-sapi/Dockerfile`)

```
        if [ "${EXTRA_REPO}" = "sury" ]; then \
            case "$(dpkg --print-architecture)" in \
                amd64) set -- \
                    4017e18448b109f640161eacfbd9e2c14f5ccd43fee0711f4f752b1faa39a6b9 pool/main/p/php8.6/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    6e729a360ac400a3be48e084389cc66c9e04af7cec30d9b1a7a69336ab029383 pool/main/p/php-defaults/php-common_102+debsuryorg12.1+3_all.deb \
                    6bbe1c4d2af3a29fb818d38989d866a5805a987511b5f6da857c0fe6a141b1d6 pool/main/p/php8.6/php8.6-cli_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    c1deb64ad9f064b5f06b1ebc8eefd9cb7c4b93407f35709b620b2b770059d941 pool/main/p/php8.6/php8.6-common_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    ff98afc81bcbbf4395ccfec15d1990ac52e9a4dcbddc4fa0315440840d8d4843 pool/main/p/php8.6/php8.6-curl_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    dc84a0d58ada1a6eed84e6c02a711e17766f774d6425aa1db691198b1ad3675c pool/main/p/php8.6/php8.6-dev_8.6.0-3+debsuryorg12.1+1_amd64.deb \
                    076bd388dc546c64b44fa47da7e75c79855b4377dea42af02563ebca0e570ce9 pool/main/p/php8.6/php8.6-readline_8.6.0-3+debsuryorg12.1+1_amd64.deb ;; \
                arm64) set -- \
                    05c7df15aee65495629856b6a357bddc241cc858df20fe0fd1ad6b0a8e901150 pool/main/p/php8.6/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    6e729a360ac400a3be48e084389cc66c9e04af7cec30d9b1a7a69336ab029383 pool/main/p/php-defaults/php-common_102+debsuryorg12.1+3_all.deb \
                    114b0a361209e8cc2649fa096580e3ff645f8500dff373117b6d9cb0d99984af pool/main/p/php8.6/php8.6-cli_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    ab4d4939ec7609ade3d653f5cb2a79fb0876e9b5f927c91f551ee09b4d10e141 pool/main/p/php8.6/php8.6-common_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    5e65f362112e9a17c1413df38fa0b871f07c77b911aeb4bcb04cd74f9ba76330 pool/main/p/php8.6/php8.6-curl_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    5b26ce094929289b721533075ccd46a4035d425aef5f82213bd54f214210f917 pool/main/p/php8.6/php8.6-dev_8.6.0-3+debsuryorg12.1+1_arm64.deb \
                    a1a18d293f22f03a7c8f76417cb272c05f2b0e7250ac5f64877230f59b5318db pool/main/p/php8.6/php8.6-readline_8.6.0-3+debsuryorg12.1+1_arm64.deb ;; \
                *) echo "no pinned PHP 8.6 payloads for this architecture" >&2; exit 1 ;; \
            esac; \
            [ "${PHP_MINOR}" = "8.6" ] || { echo "the pinned sury payloads are PHP 8.6 only: bump the pins when PHP ${PHP_MINOR} is published" >&2; exit 1; }; \
            while [ "$#" -gt 0 ]; do \
                sha=$1; path=$2; shift 2; \
                file=$(basename "$path"); \
                curl -fsSL --retry 3 "https://packages.sury.org/php/$path" -o "/tmp/sury-debs/$file"; \
                printf '%s  /tmp/sury-debs/%s\n' "$sha" "$file" >> /tmp/sury-SUMS; \
            done; \
            sha256sum -c /tmp/sury-SUMS; \
            apt-get install -y --no-install-recommends /tmp/sury-debs/*.deb \
                gcc make pkg-config libc6-dev binutils ca-certificates curl bash tar xz-utils \
                clang libclang-dev; \
            rm -rf /tmp/sury-*; \
        else \
```

Pins are byte-identical to NODE.md (checked mechanically: `diff` of the 14
`sha`/`pool/…` pairs from `NODE.md` against the Dockerfile is empty), sorted by
package name, 7 per arch; `php-common` (`all`) is shared by both arches.
`mkdir -p /tmp/sury-debs` is the only remaining setup line; `gpgv`/`xz-utils`
are no longer installed here (`xz-utils` stays in the install list below).
Alpine branch, both `apt-get update` + MB.62 retry blocks, the non-sury install
line, `rm -rf /var/lib/apt/lists/*` and everything after this `RUN` are untouched.
The pins are PHP 8.6-only, so the guard right after `esac` fails a mismatched
`PHP_MINOR` closed instead of installing the wrong payloads.

## Evidence

### a. Green leg — `make php-sapi-e2e PHP_MINOR=8.6 PHP_SAPI_LIBC=gnu PHP_SAPI_KEEP=1`

`EXIT=0`, 4 probe legs `ok`. The package `RUN` re-executed under `--no-cache`
(same Dockerfile head, same build args) so its `sha256sum -c` output is captured:

```
/tmp/sury-debs/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php-common_102+debsuryorg12.1+3_all.deb: OK
/tmp/sury-debs/php8.6-cli_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-common_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-curl_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-dev_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
/tmp/sury-debs/php8.6-readline_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
```

Tail of the green leg (final probe line):

```
php-sapi-e2e: probe — real filter, INI control removed (allow_url_fopen=1)
  ok  php 8.6 abi 1: status 200, cpu_ns 10724517 <= wall_ns 11754497, exit()+fatal contained, next request served
      allow_url_fopen=1  socket(AF_INET) errno before/after=1/1  fopen_http=other  stream_socket=other (stream_socket_client(): Unable to connect to tcp://127.0.0.1:9 (Unknown error))  curl=network (Failed to connect to 127.0.0.1 port 9 after 0 ms: Couldn't connect to server)
      json: {"allow_url_fopen": "1", … "ffi_enable": "0", "phar_require_hash": "1", "ffi_loaded": true, "phar_loaded": true, "attempts": {…}}
```

### b. Fail-closed proof

`4017e184…` → `5017e184…` (one hex char) in the amd64 `libphp8.6-embed` pin, same
make target:

```
make: *** [Makefile:762: php-sapi-e2e] Error 1     (make exit 2, docker exit 1)
  6.666 + sha256sum -c /tmp/sury-SUMS
  6.679 /tmp/sury-debs/libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_amd64.deb: FAILED
  6.696 sha256sum: WARNING: 1 computed checksum did NOT match
   157 | >>>             sha256sum -c /tmp/sury-SUMS; \
```

Pin restored; `diff` of the 14 pairs against `NODE.md` is empty again and
`git diff` for this file shows only the intended change. No `gpgv`, no
`/etc/apt/keyrings`, no `apt.gpg`, no `InRelease` fetch, no variant trial, no
`--allow-unauthenticated`, no `|| true`.

### c. Stratum checks

```
$ uvx --offline ruff format --check e2e/
17 files already formatted          (exit 0)
```

### d. Review pass: comments + the 8.6-only guard

The one line added to the sury branch by review (not by the plan sketch): the
pins are `php8.6`-only, so a leg that asks for another minor now fails closed
instead of downloading 8.6 payloads and dying at the missing `libphpX.Y.so`:

```
            esac; \
            [ "${PHP_MINOR}" = "8.6" ] || { echo "the pinned sury payloads are PHP 8.6 only: bump the pins when PHP ${PHP_MINOR} is published" >&2; exit 1; }; \
```

Comment edits only, no shell change:

`ARG EXTRA_REPO` doc block — the apt-suite line a future agent would otherwise
follow:

```
#   sury                  pinned PHP 8.6 payload source (Debian bookworm): its
#                         mirror's index objects are unreliable, so it is
#                         deliberately never added as an apt source
#   alpine-edge-testing   `edge/testing` appended to `/etc/apk/repositories`
```

MB.62 retry block, first clause only (rest of the block byte-identical):

```
        # This re-reads the distro's indexes only (sury's payloads are installed
        # from pins verified against a signature-checked index, never from an apt source), and a distro mirror
```

### e. Lead re-verification of both arch arms (the arm64 half is what kept failing)

The pins were re-checked mechanically against `NODE.md` (14/14 pairs, no extra,
no missing) and **both arms were fetched live and hash-verified on this host** —
arm64 payloads are ordinary files to `curl`, so its pins are proven too, without
waiting for an arm64 runner:

```
$ # amd64 arm, 7 payloads, sha256sum -c SUMS
libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
php-common_102+debsuryorg12.1+3_all.deb: OK
php8.6-cli_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
php8.6-common_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
php8.6-curl_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
php8.6-dev_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK
php8.6-readline_8.6.0-3+debsuryorg12.1+1_amd64.deb: OK          (EXIT=0)
$ # arm64 arm, same procedure
libphp8.6-embed_8.6.0-3+debsuryorg12.1+1_arm64.deb: OK
php-common_102+debsuryorg12.1+3_all.deb: OK
php8.6-cli_8.6.0-3+debsuryorg12.1+1_arm64.deb: OK
php8.6-common_8.6.0-3+debsuryorg12.1+1_arm64.deb: OK
php8.6-curl_8.6.0-3+debsuryorg12.1+1_arm64.deb: OK
php8.6-dev_8.6.0-3+debsuryorg12.1+1_arm64.deb: OK
php8.6-readline_8.6.0-3+debsuryorg12.1+1_arm64.deb: OK          (EXIT=0)
```

Reviewers: `review-arch` PASS (fail-closed gate fatal, `set --`/`shift 2` parity,
zero MB.63 dead code, diffs confined to the sury branch), `review-ux` approve
(provenance precise, no claim implies a keyring or `InRelease` fetch). Their two
`MINOR`s are recorded under Follow-ups.

## Follow-ups

- Bump the 14 pins (and the provenance date) when sury publishes the next PHP 8.6
  build; the values are per-arch and per-version, so a bump touches both arms.
- `php-common` is `all` and therefore listed twice (one per arch arm) — harmless,
  but a single shared preamble would remove the duplicate if a third arch lands.
- (review-ux, MINOR) the provenance comment says amd64 "disagreed in the other
  direction" without numbers; add served-vs-signed sizes there on the next bump.
- (review-ux, MINOR) the bump guidance does not say the `php-common` `_all` pin is
  shared by both arms; one clause closes it.
