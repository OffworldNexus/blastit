# RESULT — MB.3.59: the minor is resolved statically, from the library and the path

**Status: done** — `make check` exit 0 (re-run after the last edit), `make docs-check`
exit 0, `plan validate` OK, `critical_guard --base 0204266 --node MB.3.59` OK,
`make critical-skip-check` OK. Alpine 8.4 and 8.3 now serve through **their own package
name** with `BLASTIT_LIBPHP` unset, and 8.2's refusal names the missing `php_version`
symbol. Stage 0 is still zero-`dlopen`.

## What changed (static, in the node's order — no step loads the library)

| step | code | proof |
|---|---|---|
| 1. name fast path | `version_of` (unchanged): the file name, then the descriptor's `readlink` target; `PhpVersion::parse` now also reads the packed `libphp83.so` spelling | `the_version_comes_from_the_file_name`, `the_packed_name_and_the_alpine_directory_state_the_minor` |
| 2. path spellings | `path_versions` reads the file name **and** the immediate per-minor parent directory (`/usr/lib/php84`, `/usr/local/lib/php83`); `LIBPHP_CANDIDATES` gains `libphp83.so`/`libphp84.so`/`libphp82.so`; `library_dirs()` gains `/usr/lib/php{82,83,84}` and `/usr/local/lib/php{82,83,84}` in the ADR §3 order | `the_alpine_embed_directories_are_searched`, `the_packed_name_and_the_alpine_directory_state_the_minor`, Docker boot below |
| 3. object bytes | the existing bounded `.dynsym` walk now records whether `php_version` is exported **as a function**, decodes the compile-time string its body returns (`lea` on x86_64, `adrp`/`add` on aarch64, `endbr64`/`bti c` skipped), and maps the address through **mapped, non-writable** section headers only — all inside `MAX_SECTION_BYTES`, no `mmap`, no load | `the_object_bytes_name_the_minor_when_the_path_does_not`, `the_version_body_is_decoded_on_both_architectures`, real Alpine library at `/tmp/libphp.so` below |
| disagreement | `choose_version`: two different minors (path vs directory vs object) are `PhpRefusal::VersionConflict`; none is `NoVersion`; both name the path, the candidates/`.dynsym` fact and `BLASTIT_LIBPHP` | `two_disagreeing_minors_are_refused_naming_both`, real-bytes run below |
| 8.2 | `PhpRefusal::NoPhpVersion` when the symbol is absent — whatever the file name says | `a_libphp_without_php_version_is_refused_naming_the_symbol` |

`ZendAbi` (`Nts`/`Zts`/`NotLibphp`) is still the first verdict; a ZTS or non-library file is
refused before any version question, exactly as before.

## Measured before / after, per leg (each run in the leg's own image)

| leg | before (MB.3.58) | after (this node) |
|---|---|---|
| Alpine 8.4, own name, `BLASTIT_LIBPHP` unset | refused: *"… whose file name names no PHP major.minor, so no shim can be chosen …"* | **serves**: `request GET / → app 200 10.4ms` |
| Alpine 8.4, object bytes only (`/tmp/libphp.so`, no version in the path) | refused (same sentence) | **serves**: `request GET / → app 200 9.6ms` |
| Alpine 8.4 protected suites | suites were `false` (no libclang) | `critical_php_backend` 5/5, `critical_php_embed` 21/21, unprivileged maps witness `2 role(s) denied (EACCES)` |
| Alpine 8.3, own name (`/usr/lib/libphp83.so`), unset | refused (same sentence) | **serves**: `request GET / → app 200 9.1ms` |
| Alpine 8.3 protected suites | suites were `false` | `critical_php_backend` 5/5, `critical_php_embed` 21/21, unprivileged maps witness `2 role(s) denied (EACCES)` |
| Alpine 8.2 (`/usr/lib/libphp82.so`), its own image | stage-1 refusal `Symbol not found: php_version` | **stage-0 refusal naming the absent `php_version`** — see below |
| Debian bookworm 8.2 (`/usr/lib/libphp8.2.so`), its own image | stage-1 refusal `Symbol not found: php_version` | **the same stage-0 sentence** — see below |

### Alpine 8.4, own package name, `BLASTIT_LIBPHP` unset

```
$ docker run --rm blastit-php-sapi-e2e:mb359c-php84 sh -c '… env -u BLASTIT_LIBPHP \
    BLASTIT_PHP_SHIM_DIR=/src/target/php-sapi /src/target/debug/blastit dev --port 18092 …'
--- body:
alpine-own-name-ok
--- boot log:
18:17:02.642 WARN  manifest.warning the [route.php] runtime is libphp /usr/lib/php84/libphp.so and shim /src/target/php-sapi/libblastit_php84-x86_64-musl.so (PHP 8.4, x86_64-musl)
18:17:02.662 INFO  ready       serving  routes=1  workers=16
18:17:02.854 INFO  request     GET / → app  200  10.4ms
```

`mb359c` is the image rebuilt from the exact tree these numbers are reported for (the
earlier builds served the same route in `8.9–37.8ms`; the timing is the unprivileged dev
boot on a loaded host, not a claim). The shim it chose is
`libblastit_php84-x86_64-musl.so`, and the object's own `php_version` string (`8.4.27`)
agrees with the `php84` directory — no copy, no `BLASTIT_LIBPHP`.

### The object's own bytes (step 3) with a path that states nothing

```
$ docker run --rm blastit-php-sapi-e2e:mb359c-php84 sh -c 'cp /usr/lib/php84/libphp.so /tmp/libphp.so; …'
--- body:
object-bytes-ok
--- boot log:
18:17:09.557 WARN  manifest.warning the [route.php] runtime is libphp /tmp/libphp.so and shim /src/target/php-sapi/libblastit_php84-x86_64-musl.so (PHP 8.4, x86_64-musl)
18:17:09.767 INFO  request     GET / → app  200  9.6ms
```

### The disagreement sentence, on real bytes

`cp /usr/lib/php84/libphp.so /tmp/libphp8.3.so` (name says 8.3, object says 8.4):

```
blastit dev: route "app": [route.php] entry "public/index.php": /tmp/libphp8.3.so states
more than one PHP minor — `libphp8.3.so` states PHP 8.3, the `php_version` string the
object carries states PHP 8.4 — so no single shim can be chosen for it; give it one
versioned name (libphp8.4.so, libphp-8.4.so, …), keep the distro's directory and file name
consistent, or set BLASTIT_LIBPHP=/path/to/libphp.so; install the PHP embed package — …
```

### Alpine 8.3, its own packed package name, `BLASTIT_LIBPHP` unset

```
$ docker run --rm blastit-php-sapi-e2e:mb359-php83 sh -c '… env -u BLASTIT_LIBPHP \
    BLASTIT_PHP_SHIM_DIR=/src/target/php-sapi /src/target/debug/blastit dev --port 18088 …'
--- body:
alpine83-own-name-ok
--- boot log:
17:43:30.256 WARN  manifest.warning the [route.php] runtime is libphp /usr/lib/libphp83.so and shim /src/target/php-sapi/libblastit_php83-x86_64-musl.so (PHP 8.3, x86_64-musl)
17:43:30.280 INFO  ready       serving  routes=1  workers=16
17:43:30.459 INFO  request     GET / → app  200  9.1ms
```

`make rs-test-php-image` in that image: `critical_php_backend` 5/5 (`served: HTTP/1.1 200
OK … (libphp /usr/lib/libphp83.so)`), `critical_php_embed` 21/21, and the unprivileged
witness `2 role(s) denied (EACCES)`.

### 8.2: the sentence names the missing symbol, not the name

Measured in the `alpine:3.20` leg's own image, with `blastit` built there
(`apk add clang llvm-dev clang18-libclang`, `RUSTFLAGS=-C target-feature=-crt-static`):

```
blastit dev: route "app": [route.php] entry "public/index.php": /usr/lib/libphp82.so is an
NTS libphp whose `.dynsym` exports no `php_version` — the symbol the embedded worker
resolves before `php_module_startup` — so the worker cannot determine its minor at load
time and no shim can be chosen for it (its file name states PHP 8.2, but a file name
cannot replace the symbol); PHP 8.2 and earlier ship it that way; install the PHP embed
package — Debian/Ubuntu: apt install libphp-embed, Alpine: apk add php84-embed,
Fedora/RHEL: dnf install php-embedded, or set BLASTIT_LIBPHP=/path/to/libphp.so
```

The same sentence was produced by the `debian:bookworm` 8.2 leg's own image
(`cargo build -p blastit` there; `BLASTIT_LIBPHP=/usr/lib/libphp8.2.so`), and the unit test
`a_libphp_without_php_version_is_refused_naming_the_symbol` holds it on the synthetic
object. This is the sentence the two `refused` matrix rows record; the shim's real-load
probe still serves in both 8.2 images (`make php-sapi-e2e` all three egress cases above).

## Support statement (`scripts/platform_matrix.py`)

```
$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and exercised:
4/8 wheels, 2/4 archives, 6/12 shims; reaches: served 4 of 12 shims, 0 probed,
2 refused, 6 unbuilt; 8 served once the aarch64 legs run; 0 leg(s) dropped
```

`SHIM_REACHES` now says `served` for 8.4/8.3 on both `gnu` and `musl`, `refused` for 8.2 on
both. The Alpine legs moved from `suites: false` to `true`: `e2e/php-sapi/Dockerfile` now
installs `clang`/`llvm-dev`/`clang19-libclang` on the musl branch when the leg builds the
suites, exports `LIBCLANG_PATH=/usr/lib/llvm19/lib`, and compiles the two protected suites
with `-C target-feature=-crt-static` (a static musl binary cannot `dlopen`).

## Verification commands

```
$ cargo test -p blastit --locked --test critical_php_embed
test result: ok. 21 passed; 0 failed
$ cargo test -p blastit --locked --test critical_php_backend   # on a host with no PHP
test result: ok. 5 passed; 0 failed
$ cargo test -p blastit --locked --features python             # exit 0
$ make rs-test-php-image PHP_SAPI_IMAGE=blastit-php-sapi-e2e:mb359c-php84 \
      PHP_SAPI_IMAGE_MINOR=8.4 PHP_SAPI_IMAGE_LIBPHP=/usr/lib/php84/libphp.so
test result: ok. 5 passed  (critical_php_backend)
test result: ok. 21 passed (critical_php_embed)
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
$ make rs-test-php-image PHP_SAPI_IMAGE=blastit-php-sapi-e2e:mb359-php83 \
      PHP_SAPI_IMAGE_MINOR=8.3 PHP_SAPI_IMAGE_LIBPHP=/usr/lib/libphp83.so
test result: ok. 5 passed / 21 passed; PR_SET_DUMPABLE(0) witness — 2 role(s) denied
$ make php-sapi-e2e PHP_MINOR=8.2 PHP_SAPI_LIBC=musl   # the probe still serves in the 8.2 leg
  ok  php 8.2 abi 1: status 200 … (all three egress cases)
$ make docs-check                                             # No issues found
$ uv run --no-project tools/critical_guard.py --base 0204266 --node MB.3.59
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.3.59
$ make critical-skip-check                                    # holds no skip construct
$ uv run --no-project tools/plan.py validate                  # plan OK
$ make check                                                  # exit 0
$ make e2e                                                    # exit 0 (see the flake note)
```

`make e2e`'s first run failed 3 of 142 promises on the *demo* path: `P6 E10` answered
`502` where `200` was expected, and the harness's own retry `P6 E10[1]` passed. That is
`MB.3.18`'s documented `max_requests = 1` recycle-window flake (scheduler returns `502` in
the sub-millisecond window after a worker sends `DONE`), unrelated to this node's
discovery; the second `make e2e` run was exit 0.

## Docs

`docs/frameworks/php.md` §1 was rewritten with the reviewer's corrections: 8.2 is "not
supported yet" and its package "ships no `php_version`" (a host-package fact, not a
Blastit bug); Alpine's names are no longer a refusal; the copy is enough and
`BLASTIT_LIBPHP` is only for a library elsewhere; "the one privileged step" is true again
because no Alpine copy is needed. In the node's `NODE.md`, the stale "not in `paths`" /
"only the first three" notes are struck; in `MB.3.58/RESULT.md`, "the gate and the release
body" became "the gate and `--summary`" and the "only places it is printed" contradiction
is fixed. (Those two are prose-only edits to plan files named in the lead's LOG entry, made
because the lead's list asked for them here; no status, attempt or frontmatter field was
touched.)

## What is still unproven

The aarch64 legs (both libcs) are unbuilt, exactly as before: the sentence still says
"8 served once the aarch64 legs run", and `SHIM_REACHES` has no aarch64 row. The aarch64
`php_version` decoder is pinned by bytes (`the_version_body_is_decoded_on_both_architectures`)
but has not run on an arm host.

## Follow-ups (outside this node's `paths`)

- `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` §1.3/§4.1 still lists only
  the dotted names and the five glibc directories; the ADR's wording should gain the
  packed/per-minor spellings and the `php_version`-absent refusal (the node's notes asked
  for it, but `docs/decisions/**` is not in this node's paths).
- `.github/workflows/php-sapi.yml:127-140` still says the Alpine legs "build no `blastit`
  (no libclang)"; the matrix now builds the suites there. Workflows are out of scope.
- `docs/knowledge/LESSONS.md`: PHP's `php_version` string is often the suffix of the
  `X-Powered-By: PHP/8.4.27` literal (Alpine), so a bare version-string scan misses it —
  the function's `lea`/`adrp` points into the middle of a bigger string.
