# RESULT — MB.3.65: PHP 8.2 out, 8.5 in; no apology language

STATUS: done

## Summary

The shim set is now **8.3 / 8.4 / 8.5 × {x86_64,aarch64} × {gnu,musl} = 12**, with every
count re-derived from `scripts/platform_matrix.py --summary` (12 declared, 6 proved on
x86_64, 6 aarch64 unbuilt → CI only). 8.2 is gone from the matrix, the probe list, the
removal directories, the toolchain enum, the shims, the e2e images and every sentence a
user reads; an 8.2-only host now reads exactly

```
route "app": [route.php] entry "public/index.php": PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4 and 8.5
```

**The 8.5 ABI needed no redesign:** the shim is compiled against 8.5's own headers and the
one source-level conditional (`ini_entries`' const-ness for 8.2) is deleted, because every
supported minor now has the `const char *` field. Measured, not assumed: the 8.5 shim
compiled, `dlopen`ed beside `libphp8.5.so`, served six real requests (body, head, status,
`cpu_ns`, contained `exit()`/fatal, next request served) under both the INI-only and the
real `SeccompRole::View` filter, on **glibc and musl**. One build-host fact surfaced: PHP
8.5's `zend_operators.h` has an unused parameter in a `static inline`, so `-Wall -Wextra
-Werror` rejects the *header*; the Makefile now passes `php-config --includes` as
`-isystem` (their warnings suppressed, `-Werror` kept for `c/blastit_php_sapi.c`).

The tone guard is `scripts/check_doc_tone.py` (narrow literal list, 9 phrases) wired into
`make check` as `doc-tone`, with pytest negatives in `scripts/test_doc_tone.py` (also in
`docs-unit`): each phrase fails on a dirty fixture and names phrase + line, a clean
fixture passes, the list is asserted non-empty and every entry still detected, and the
covered set is proved to be `README.md` + `docs/**` minus `design/`, `decisions/`,
`research/`, `knowledge/`, `testing/` and `VISION.md`.

## Evidence

### A1 — `ubuntu:26.04` + `libphp8.5-embed`, `BLASTIT_LIBPHP` **unset**

`make php-sapi-e2e PHP_MINOR=8.5` (image `blastit-php-sapi-e2e:mb365-php85-gnu-keep`), then
a real `blastit dev` boot inside it (`-e BLASTIT_LIBPHP=`):

```console
$ docker run --rm -e BLASTIT_LIBPHP= -e PORT=18085 blastit-php-sapi-e2e:mb365-php85-gnu-keep \
    sh -c 'cd /tmp/app && BLASTIT_LIBPHP= /src/target/debug/blastit dev & … curl localhost:18085/'
GET / -> 200  body: php 8.5.4
22:49:52.767 WARN  manifest.warning the [route.php] runtime is libphp /usr/lib/libphp8.5.so and shim /src/target/php-sapi/libblastit_php85-x86_64-gnu.so (PHP 8.5, x86_64-gnu)
BLASTIT_LIBPHP in env: [unset]
```

Probe (six requests, real filter, denied + INI control removed):

```console
  ok  php 8.5 abi 1: status 200, cpu_ns 21136725 <= wall_ns 25341128, exit()+fatal contained, next request served
  ok  php 8.5 abi 1: status 200, cpu_ns 14677844 <= wall_ns 16856536, exit()+fatal contained, next request served
  ok  php 8.5 abi 1: status 200, cpu_ns 11732307 <= wall_ns 12571136, exit()+fatal contained, next request served
php-sapi-e2e: ok
```

Protected suites on the same real pair (`make rs-test-php-image`):

```console
served: HTTP/1.1 200 OK  cpu_ns=18349191  wall_ns=35669654  (libphp /usr/lib/libphp8.5.so)
test result: ok. 9 passed; 0 failed   (critical_php_backend)
test result: ok. 26 passed; 0 failed  (critical_php_embed)
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
```

### A2 — `alpine:3.23` + `php85-embed`, `BLASTIT_LIBPHP` **unset**

```console
$ docker run --rm -e BLASTIT_LIBPHP= -e PORT=18086 blastit-php-sapi-e2e:mb365-php85-musl-keep \
    sh -c 'cd /tmp/app && BLASTIT_LIBPHP= /src/target/debug/blastit dev & … curl localhost:18086/'
GET / -> 200  body: php 8.5.12
23:09:46.714 WARN  manifest.warning the [route.php] runtime is libphp /usr/lib/php85/libphp.so and shim /src/target/php-sapi/libblastit_php85-x86_64-musl.so (PHP 8.5, x86_64-musl)
BLASTIT_LIBPHP in env: [unset]
```

Protected suites on the same musl pair:

```console
served: HTTP/1.1 200 OK  cpu_ns=18947173  wall_ns=30566813  (libphp /usr/lib/php85/libphp.so)
test result: ok. 9 passed; 0 failed
test result: ok. 26 passed; 0 failed
test result: ok. 1 passed; 0 failed          (the unprivileged dumpable witness)
```

### A3 — `scripts/platform_matrix.py --summary` (8.5 out of `unbuilt`; counts from the run)

```console
| family | declared | built & exercised | dropped (reason) |
|---|---|---|---|
| wheels | 8 | 4 of 8 | — |
| standalone archives | 4 | 2 of 4 | — |
| PHP shims | 12 | 6 of 12 | — |
…
| `libblastit_php85-x86_64-gnu.so` | ubuntu:26.04 (ubuntu-24.04) | proved by MB.3.65 |
| `libblastit_php85-x86_64-musl.so` | alpine:3.23 (ubuntu-24.04) | proved by MB.3.65 |
…
| PHP minor | glibc (Debian/Ubuntu) | musl (Alpine) |
|---|---|---|
| 8.5 | served | served |
| 8.4 | served | served |
| 8.3 | served | served |
```

`--check`: `platform-matrix: ok — 8 wheels, 4 archives, 12 shims declared; built and
exercised: 4/8 wheels, 2/4 archives, 6/12 shims; reaches: served 6 of 12 shims, 0 probed,
0 refused, 6 unbuilt; 12 served once the aarch64 legs run`.

### A4 — 8.3 / 8.4 unchanged (no regression)

Six x86_64 legs probed green in this session, each in its own distro image:

| leg | image | result |
|---|---|---|
| php85 x86_64-gnu | `ubuntu:26.04` | ok (above) |
| php85 x86_64-musl | `alpine:3.23` | ok (above) |
| php84 x86_64-gnu | `debian:trixie-slim` | `ok  php 8.4 abi 1: status 200 …` |
| php84 x86_64-musl | `alpine:3.21` | `ok  php 8.4 abi 1: status 200 …` |
| php83 x86_64-gnu | `ubuntu:24.04` | `ok  php 8.3 abi 1: status 200 …` |
| php83 x86_64-musl | `alpine:3.21` | `ok  php 8.3 abi 1: status 200 …` |

8.3/8.4 keep their exact 4 probe spellings and their own `library_dirs()` entries; the only
new paths are `php85`/`8.5`, tried **before** them, so a host with no 8.5 resolves exactly
what it did before (proved by the four 8.3/8.4 legs above).

### A5 — Full 12-shim matrix: what only CI can prove

Green here: **6 of 6 x86_64 shims** (8.3/8.4/8.5 × gnu/musl), built and probed natively.
**Not proved here, CI-only:** `libblastit_php{83,84,85}-aarch64-{gnu,musl}.so` — they need
`ubuntu-24.04-arm` runners (`runs_on` in the matrix); no emulator proves a shim's
`dlopen`/`seccomp`/`fork`, and this node claims nothing about them. `PROVEN` and
`SHIM_REACHES` list them as `declared, unbuilt` / `unbuilt`.

### A6 — 8.2-only bookworm: exactly the unsupported-version sentence

New, reproducible witness: `e2e/php-sapi/Dockerfile.unsupported-minor` (Debian bookworm,
`libphp8.2-embed`) + `run-unsupported-minor.sh`:

```console
$ docker run --rm blastit-php-82:mb365
unsupported-minor: the host's own PHP
unsupported-minor: /usr/lib/libphp8.2.so exports NO php_version
unsupported-minor: ok — PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4 and 8.5
blastit dev: route "app": [route.php] entry "public/index.php": PHP 8.2 is not supported — Blastit supports PHP 8.3, 8.4 and 8.5
```

The refusal is by **minor** (`PhpRefusal::UnsupportedVersion`, fed by the object's
`php_version` body or, when absent, the file name / per-minor directory), not by the
missing symbol; the sentence is one constant (`SUPPORTED_PHP`).

### A7 — `grep -ri 'php82\|8\.2' docs/ crates/ scripts/ .github/ e2e/ ARCHITECTURE.md`

Returned hits, all accounted for:

- **The required refusal and its tests**: `crates/blastit/src/stage0.rs` (the
  `installed_unsupported` doc examples `libphp8.2.so` / `/usr/lib/php82`, the
  `UnsupportedVersion` unit test, and the `SUPPORTED_PHP` sentence's test), plus
  `crates/blastit/tests/critical_php_embed.rs`'s protected 8.2 case — these *are* the
  "8.2 is not supported" statement the node requires.
- **`§8.2` section references** in `ARCHITECTURE.md`, `crates/blastit/src/{config,cli,stage1}.rs`,
  `crates/blastit-http/*`, `blastit-scheduler`, `blastit-frame`, `blastit-dbproxy`,
  `blastit-manifest`, `e2e/venv-smoke` — the configuration-reference section number, not PHP.
- **Unrelated numbers**: `docs/spikes/2026-10-do-secret-barrier.report.json` (`elapsed_ms:
  28.2`), `e2e/rootless/out/rootless-app.log` (generated), `scripts/__pycache__/*.pyc`.
- **Out of this node's `paths` (left, named here)**: `docs/design/backend-runtimes.md`
  (`libblastit_php{82,83,84}` in the `R11` row, `8.2 vs 8.4` in a rejected-option row) and
  `docs/decisions/ADR-0009-…` ("up to `3` proven `libblastit_php{82,83,84}-…so` shims") —
  internal design/decision records, historical snapshots, not user pages; ADR-0010 is the
  table of record and is updated. `docs-site-plan.md` (§8.2 refs) and `CHANGELOG.md`
  (release history) likewise.
- `scripts/platform_matrix.py` quotes ADR-0009's literal in a comment explaining the count
  regex (`{82,83,84}`), and `crates/blastit-php-sapi/tests/critical_shim_contract.rs`
  asserts `Minor::parse("8.2.29") == None` — the refusal, not support.

### A8 — Protected 8.2 case asserts the sentence and goes red when it regresses

The protected test is
`crates/blastit/tests/critical_php_embed.rs::an_unsupported_php_minor_refuses_the_boot_naming_the_supported_set`
(it builds an 8.2 stub ELF, boots `blastit dev`, asserts the sentence verbatim, that the
`.dynsym` story is absent, and that nothing serves).

Mutation run (the source backed up to `/tmp`, mutated in place, the test run, then restored —
`sha256sum -c` proves the restore is byte-identical):

```console
$ sha256sum crates/blastit/src/stage0.rs
5c26e2dd94e055449b6ded3189263043868d3f9584528530f9817792dde472bb
$ cp crates/blastit/src/stage0.rs /tmp/stage0.rs.orig
$ sed -i 's/…"PHP 8.3, 8.4 and 8.5";/…"PHP 8.3 and 8.4";/' crates/blastit/src/stage0.rs
$ cargo test -p blastit --test critical_php_embed --locked an_unsupported_php_minor
---- an_unsupported_php_minor_refuses_the_boot_naming_the_supported_set stdout ----
thread panicked at crates/blastit/tests/critical_php_embed.rs:1812:5:
the refusal is exactly the support sentence: blastit dev: route "app": … PHP 8.2 is not supported — Blastit supports PHP 8.3 and 8.4
test result: FAILED. 0 passed; 1 failed
$ cp /tmp/stage0.rs.orig crates/blastit/src/stage0.rs && sha256sum -c /tmp/stage0.sha
crates/blastit/src/stage0.rs: OK
```

No assertion was weakened, no `#[ignore]`, no skip.

### A9 — `critical_guard` + `make check`

```console
$ uv run --no-project tools/critical_guard.py --base 2419971 --node MB.3.65
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.3.65
```

```console
$ make check BASE=2419971 NODE=MB.3.65
critical-guard: OK: all 3 protected change(s) are authorized by node(s) MB.3.65
… rs-fmt / rs-lint / rs-doc / rs-test / test-critical / py-lint / docs-unit / docs-check /
  docs-test / rs-no-python / seam-check / platform-matrix-check / workflow-check /
  packaging-gates-test / doc-tone / plan-validate …
doc-tone: ok — 26 user-facing page(s), no banned phrase
platform-matrix: ok — … 12 shims declared; built and exercised: … 6/12 shims …
workflow-check: ok — 365 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 43 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=7, test-seam=6)
plan OK
exit=0
```

### A10 — The tone guard's negatives (`scripts/test_doc_tone.py`)

```console
$ uv run --no-project --with pytest==8.4.2 python -m pytest scripts/test_doc_tone.py -q
.............                                                            [100%]
13 passed in 0.49s
```

Covers: each of the 9 phrases in a fixture → exit 1 and `docs/page.md:4: banned phrase "…"`;
a clean fixture (including the legitimate words *limitation*, *worked around*, *for now*)
→ exit 0; the list non-empty and every phrase still detected; the covered set exact
(`README.md` + `docs/frameworks/php.md`, internal records ignored even when dirty); the
repository itself clean.

## Files

- `scripts/platform_matrix.py` — 8.2 rows/`PHP_MINORS` entry removed; `("8.5", gnu)`
  (`ubuntu:26.04`, `libphp8.5-embed`, `/usr/lib/libphp8.5.so`, `php8.5-dev`) and
  `("8.5", musl)` (`alpine:3.23`, `php85-embed`, `/usr/lib/php85/libphp.so`, `php85-dev`);
  `PROVEN` and `SHIM_REACHES` for both 8.5 x86_64 legs (proved by this node).
- `crates/blastit/src/stage0.rs` — `LIBPHP_CANDIDATES`/`LIBPHP_PROBES` (8.5 first, 8.2 gone,
  13 entries), `library_dirs()` (`php85` replaces `php82`), `PhpRefusal::UnsupportedVersion`
  + `SUPPORTED_PHP` sentence, `installed_unsupported()` refusal scan, `NoPhpVersion` no
  longer tells the 8.2 story, tests rewritten (ordering 8.3→8.5, unsupported-minor cases).
- `crates/blastit/src/config.rs` — `PhpVersion::supported()` (8.3–8.5).
- `crates/blastit-php-sapi/src/toolchain.rs` — `Minor::{Php83,Php84,Php85}`,
  `CONFIG_NAMES` newest-first, error sentences name 8.3/8.4/8.5.
- `crates/blastit-php-sapi/{build.rs,src/lib.rs,src/loader.rs,src/probe/main.rs}` — 8.2 → 8.5.
- `crates/blastit-php-sapi/c/blastit_php_sapi.c` — the 8.2-only `ini_entries` conditional
  deleted (every supported minor declares `const char *`).
- `crates/blastit-php-sapi/tests/critical_shim_contract.rs` — parse test 8.5/8.2, toolchain
  message/packaging names 8.5, unsupported minor is 8.6.
- `crates/blastit/tests/common/libphp_probes.rs` — the drift oracle's 13 entries + self-test
  indices moved to 8.5.
- `crates/blastit/tests/critical_php_{embed,backend}.rs` — `PROBES`, directory lists,
  `host_php()` comment; new protected unsupported-minor case.
- `Makefile` — `PHP_MINORS ?= 8.5 8.4 8.3`, `-isystem` for PHP headers, new `doc-tone`
  target in `check`, `scripts/test_doc_tone.py` in `docs-unit`.
- `scripts/check_doc_tone.py`, `scripts/test_doc_tone.py` (new).
- `scripts/test_packaging_gates.py` — the `suites_because` negative mutates
  `suites: True → False` instead of quoting the deleted 8.2 leg.
- `e2e/php-sapi/Dockerfile`, `Dockerfile.two-package` (8.3+8.4), `run-probe.sh`,
  `run-two-package.sh` (rewritten witness: both objects usable, newest must win),
  `Dockerfile.unsupported-minor` + `run-unsupported-minor.sh` (new), `e2e/php_sapi_probe.py`
  (its own `{"803","804","805"}` allowlist).
- `.github/workflows/php-sapi.yml`, `publish-pypi.yml` — prose only.
- `docs/frameworks/php.md` — the 8.2 paragraph replaced by the single sentence *"We support
  supported PHP versions: 8.3, 8.4 and 8.5."* (line 44's "not a degraded mode" untouched).
- `docs/decisions/ADR-0010-…` — probe order 8.5→8.4→8.3, `php{85,84,83}` dirs, `12` shims /
  `3` per tuple unchanged, no 8.2 refusal story, `php85-embed` in the package lists.
- `ARCHITECTURE.md` — the boot-refusal package list names `php85-embed`.
- `docs/knowledge/LESSONS.md` — one entry: PHP 8.5's header warning + the e2e driver's own
  minor allowlist.

## Follow-ups

- **aarch64 is CI-only.** `libblastit_php{83,84,85}-aarch64-{gnu,musl}.so` need
  `ubuntu-24.04-arm`; nothing here claims them. The release PR's PHP SAPI workflow is the
  proof.
- **`docs/design/backend-runtimes.md` and `ADR-0009` still carry `{82,83,84}` literals.**
  They are internal records outside this node's `paths`; ADR-0010 (the table of record) is
  updated. A later doc sweep can move them if the owner wants one story everywhere.
- **`docs/design/backend-runtimes.md` §4.1/§4.7 counts** pass `--check` (`12`/`3` are still
  true), so no count edit was needed there.
- **The 8.2-only witness is run by hand** (like `Dockerfile.two-package`): it proves a
  refusal, not an artifact, so it is deliberately not a matrix leg.
- **`e2e/php-sapi/Dockerfile.two-package` was not rebuilt here** (a ~10-minute Alpine build
  with `pg_query` bindgen). Its packages moved 8.2/8.4 → 8.3/8.4 and its witness was
  rewritten to assert "both objects usable, newest wins" (the 8.2 object no longer exists
  to be the unusable one); the ordering itself is proved by `stage0`'s unit test and by the
  drift-checked probe tables.
