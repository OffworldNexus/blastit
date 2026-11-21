# RESULT — MB.3.61

Discovery now prefers the library that can *serve* rather than the first name in
a list, the "exports no `php_version`" sentence is only said about an object that
really has no such symbol, and the two guards that reported the wrong reason
either way (ADR-0010, `check-exec-env.sh`, `check_workflows.py`) say the true one.
Every claim below is a command that ran here, including the two-package host
(`php82-embed` **and** `php84-embed` installed together) in a real image.

Base: `490bb9b` on `release/0.2.0`, worktree
`.worktrees/release-0.2.0`. No commit, no `git add`, no plan-status change.

---

## 1. The ordering defect: two embed packages, and the newest one serves

### 1.1 The cause, in one pair of lines

`first_existing` walked `LIBPHP_CANDIDATES` **name-major** — every directory for
`libphp8.4.so`, then every directory for `libphp-8.4.so`, … — so
`/usr/lib/libphp82.so` was tried before `/usr/lib/php84/libphp.so`, although the
8.2 object exports no `php_version` and the 8.4 one serves. `MB.3.61` replaces
the name-major walk with `LIBPHP_PROBES`, which is **version-major**: every 8.4
spelling (dotted, hyphenated, packed, and `libphp.so` *inside* `/usr/lib/php84`
or `/usr/local/lib/php84`), then every 8.3 spelling, then every 8.2 spelling,
then an unversioned `libphp.so` anywhere as the last resort. `LIBPHP_CANDIDATES`
stays as the list the refusal sentence names.

A single-package host is unaffected by construction: only one probe can match,
so the same file is found as before. The new unit test pins that half too.

### 1.2 Proven red on the base commit, green now (unit level)

The test was written first and run against `490bb9b`'s `stage0.rs` with only the
test module applied:

```console
$ cargo test -p blastit --locked --lib -- \
    stage0::php_boot_gate::two_embed_packages_serve_from_the_newer_library \
    stage0::php_boot_gate::php_version_presence_is_the_name_not_the_body_shape
test stage0::php_boot_gate::php_version_presence_is_the_name_not_the_body_shape ... FAILED
test stage0::php_boot_gate::two_embed_packages_serve_from_the_newer_library ... FAILED
---- two_embed_packages_serve_from_the_newer_library stdout ----
panicked at crates/blastit/src/stage0.rs:5000:13:
two packages, and the 8.4 library serves: Refused("route \"app\": [route.php] entry
\"public/index.php\": /tmp/blastit-php-gate-2696585-two-packages/libphp82.so is an NTS libphp
whose `.dynsym` exports no `php_version` — the symbol the embedded worker resolves before
`php_module_startup` — … (its file name states PHP 8.2, but a file name cannot replace the
symbol); PHP 8.2 and earlier ship it that way; install the PHP embed package — … ")
---- php_version_presence_is_the_name_not_the_body_shape stdout ----
panicked at crates/blastit/src/stage0.rs:5059:43:
STT_NOTYPE: refused as NoPhpVersion { path: "…/odd-symbols-STT_NOTYPE/libphp8.4.so",
named: Some(PhpVersion { major: 8, minor: 4 }) }
test result: FAILED. 0 passed; 2 failed; …
```

With the fix the same two tests pass, and the whole gate's suite with them:

```console
$ cargo test -p blastit --locked --lib -- stage0::php_boot_gate
test result: ok. 26 passed; 0 failed; 0 ignored; 0 measured; 184 filtered out
```

The test itself: a manifest with `[route.php]`, both libraries on disk at their
packages' own paths, `BLASTIT_LIBPHP` unset (`PhpSearch::new(None, …)`, which
*is* "no override"), `lib_dirs` = the two directories `library_dirs()` names:

* both present → `PhpBoot::Proceed` with `version == 8.4` and `libphp` the
  `php84/libphp.so` path;
* the 8.2 object alone → still refused, naming the symbol it really lacks
  (single-package behaviour, unchanged);
* the 8.4 object alone → still serves from its own directory.

### 1.3 Proven on a **real** two-package host (an image, both packages)

New, in this node's paths: `e2e/php-sapi/Dockerfile.two-package` +
`e2e/php-sapi/run-two-package.sh`. `alpine:3.21` installs `php82-embed`
(8.2.35, `/usr/lib/libphp82.so`) **and** `php84-embed`
(`/usr/lib/php84/libphp.so`) side by side; the image builds `blastit`, the 8.4
shim and both protected suites with `BLASTIT_LIBPHP` unset, and the witness
refuses to pass unless the suites took their **served** branch and the resolved
library is the 8.4 one (a skipped suite is a green image that proves nothing).

Base commit `490bb9b`, same image, same command:

```console
$ docker build -f e2e/php-sapi/Dockerfile.two-package -t blastit-php-two:base490 .
$ docker run --rm --cap-add SYS_PTRACE blastit-php-two:base490
two-package: the host's own layout
-rwxr-xr-x 1 root root 4821697 /usr/lib/libphp82.so
-rwxr-xr-x 1 root root 5051928 /usr/lib/php84/libphp.so
two-package: /usr/lib/libphp82.so exports NO php_version
two-package: /usr/lib/php84/libphp.so exports php_version
…
test result: ok. 21 passed; 0 failed          # the suites were green — on their REFUSAL branch
two-package: FAIL — the boot refused on a host whose 8.4 library serves:
refused (no usable libphp + shim on this host): blastit dev: route "app": [route.php] entry
"public/index.php": /usr/lib/libphp82.so is an NTS libphp whose `.dynsym` exports no
`php_version` — … (its file name states PHP 8.2, but a file name cannot replace the symbol)
exit=1
```

This node's code, same image definition:

```console
$ docker build -f e2e/php-sapi/Dockerfile.two-package -t blastit-php-two:local .
$ docker run --rm --cap-add SYS_PTRACE blastit-php-two:local
two-package: the host's own layout
-rwxr-xr-x 1 root root 4821697 /usr/lib/libphp82.so
-rwxr-xr-x 1 root root 5051928 /usr/lib/php84/libphp.so
two-package: /usr/lib/libphp82.so exports NO php_version
two-package: /usr/lib/php84/libphp.so exports php_version
…
symlinked /tmp/blastit-php-backend-17-symlink/libphp8.so -> /usr/lib/php84/libphp.so  served: HTTP/1.1 200 OK
served: HTTP/1.1 200 OK  cpu_ns=30939759  wall_ns=40900524  (libphp /usr/lib/php84/libphp.so)
test result: ok. 5 passed; 0 failed
…
test result: ok. 21 passed; 0 failed
two-package: ok — with php82-embed and php84-embed installed and no BLASTIT_LIBPHP,
  the boot served from /usr/lib/php84/libphp.so (the 8.2 object at /usr/lib/libphp82.so was not preferred)
exit=0
```

`--cap-add SYS_PTRACE` is the same requirement `make rs-test-php-image` has (the
maps leg content-checks `PR_SET_DUMPABLE(0)` roles as root); it is in the
Dockerfile header and the script header so the next reader does not rediscover
it.

### 1.4 The two critical suites' own probe agrees with the boot again

`crates/blastit/tests/critical_php_{embed,backend}.rs` each carry a
`libphp_path()` helper that claims to look where `blastit` looks. It had the old
name-major order, so on a two-package host it would have probed `libphp82.so`,
found no `php_version` and silently skipped the served half — a test that reports
"no usable pair" on a host that serves. Both helpers now use the same
version-major probe list (and the musl triplet, which they had hard-coded as
`gnu`). No assertion was weakened; the diff is the helper and its doc comment.

## 2. `php_version` presence is the name, the body is a separate fact

`scan_symbols` set `php_version = Some((st_value, st_size))` only for an
`STT_FUNC` symbol whose fields it could read, and `ObjectFacts::exports_php_version`
was `php_version.is_some()` — so an object exporting `php_version` as
`STT_NOTYPE`, or with `st_size == 0`, was refused with "exports no
`php_version`", a sentence about a symbol it has. Now:

* `DynSymbols::exports_php_version` is set when the **name** is seen in the
  `.dynsym`, nothing else;
* `DynSymbols::php_version_body` carries `(st_value, st_size)` and is only set
  for an `STT_FUNC` whose fields were read;
* `ObjectFacts::version` still comes from the body decode, so a versioned path
  (`libphp8.4.so`) serves and an unversioned one with an unreadable body is
  `NoVersion`, whose sentence names the unreadable `return PHP_VERSION;` body
  (and says an `STT_NOTYPE`/zero-sized symbol is exactly that shape);
* `NoPhpVersion` (and its sentence) is now only about a **missing name**.

Pinned by `php_version_presence_is_the_name_not_the_body_shape` for both shapes
(`STT_NOTYPE`, `st_size == 0`), with three assertions each: the versioned name
**serves** as 8.4; the unversioned twin is refused with the body sentence and
**not** with "exports no `php_version`"; and an object with no such name at all
still gets that absence sentence — the two refusals stay distinguishable. The
fixture grew one `PhpVersionSymbol` enum (`Function`/`NotType`/`ZeroSize`) and
`shared_object_in`; every existing fixture keeps its exact bytes.

## 3. ADR-0010 no longer says either false thing

`docs/decisions/ADR-0010-…md`, two lines of the same table/paragraph the node
names, plus the twin on the wheels row so the table cannot contradict itself:

* **§1.3 (`:16`)** — the parenthetical that listed seven dotted names and five
  directories now lists the directories (`/usr/lib/<triplet>`, `/usr/lib64`,
  `/usr/lib`, `/usr/lib/php{84,83,82}`, `/usr/local/lib/<triplet>`,
  `/usr/local/lib`, `/usr/local/lib/php{84,83,82}`), the packed names, the
  per-minor `libphp.so`, the **version-major** order and the reason ("a host
  carrying two embed packages serves from the newest library rather than
  refusing the first name it finds"), and names **both** refusals: no
  `php_version` **name** (`NoPhpVersion`), and name present but the returned
  version unreadable with no minor in the path (`NoVersion`).
* **§4 (`:41`)** — `Host runtime required` for the standalone archives was
  "**Nothing** for static". It now reads: `-gnu` glibc **2.28** (built in
  `quay.io/pypa/manylinux_2_28_<arch>`, `--max-glibc 2_28` refuses a binary that
  outgrew it, CI executes it on bookworm/trixie/the builder); `-musl` no glibc
  but `apk add libgcc` (dynamic musl, so it loads `libgcc_s.so.1`); no static
  archive ships; `libphp` 8.3–8.4 for `[route.php]` with an 8.2 host refused.
* **§4 (`:40`)** — the wheels row said `8.2`–`8.4` for the `libphp` package; it
  now says `8.3`–`8.4` with the 8.2 refusal named, so the two rows agree.

```console
$ git diff --stat docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md
 docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md | 6 +++---
 1 file changed, 3 insertions(+), 3 deletions(-)
```

(three lines changed: the two the node names, plus the wheels row that would
otherwise contradict the archives row about 8.2.)

## 4. Folded guards: a guard that reports the wrong reason, twice

### 4.1 `scripts/check-exec-env.sh` — "cannot parse" is exit 2, not a silent exit 1

Both `readelf` substitutions were bare pipelines with `2>/dev/null`; under
`set -euo pipefail` a non-ELF input failed the assignment and killed the script
with no message at all — the caller saw exit 1, i.e. "too old". One
`read_version_info()` now performs the read once, checks its status and prints
the tool's own reason before exiting 2; `required` and `abi_nodes` parse that
captured text.

```console
$ git show 490bb9b:scripts/check-exec-env.sh > /tmp/old.sh; /tmp/old.sh \
    --image debian:trixie-slim --binary README.md
$ echo $?
1                                  # nothing printed: a verdict of the wrong kind, with no reason

$ scripts/check-exec-env.sh --image debian:trixie-slim --binary README.md
check-exec-env: FAIL — cannot read symbol versions from 'README.md', so its glibc requirement is unknown:
  readelf: Error: Not an ELF file - it has the wrong magic bytes at the start
  a file readelf cannot parse has no version requirement to compare, and
  reporting one would be a guess. Pass the ELF binary this check is about.
$ echo $?
2

# the happy path is untouched
$ scripts/check-exec-env.sh --image debian:trixie-slim --binary target/debug/blastit
check-exec-env: ok — blastit needs glibc 2.39, debian:trixie-slim provides 2.41
$ echo $?
0
```

### 4.2 `scripts/check_workflows.py` — `make standalone` is a host build

Rule 15 only matched the literal `scripts/build-standalone.sh`, so a shipping
step running `make standalone` (whose recipe *is* that script) skipped the rule
entirely — neither the host-build check nor the per-leg `--max-glibc` /
`builder_image` checks ran. Now `STANDALONE_MAKE` (with `(?![-\\w])`, so
`make standalone-glibc` is not this shape) feeds `runs_host_standalone_build()`,
which is used both to enter the rule and to refuse.

Self-test: `self_test_rule15()` runs at the top of **every** audit (`make
workflow-check`, inside `make check`) and answers two things — the recognizer
sees five host spellings and rejects three shipped ones, and a synthetic
`publish-pypi.yml` job actually produces a problem for each host spelling. It is
in `check_workflows.py` itself because that is the file this node's `paths`
cover; a broken recognizer exits 2 rather than passing workflows it can no longer
see.

```console
# the defect, restored in a fixture copy of publish-pypi.yml
$ sed -i 's|scripts/build-standalone-in-image.sh|make standalone|' \
    /tmp/mb361-fixture/repo/.github/workflows/publish-pypi.yml
$ uv run --no-project --locked scripts/check_workflows.py --repo /tmp/mb361-fixture/repo
FAIL publish-pypi.yml: job binaries_gnu step 6: runs scripts/build-standalone.sh on the runner
  (or `make standalone`, the same host build), which compiles against the runner's C library …
workflow-check: FAIL — 365 checks over 6 workflows and 1 actions, 1 problem(s)
$ echo $?
1

# the shipped spelling is not refused
$ sed -i 's|make standalone |make standalone-glibc |' …/publish-pypi.yml
$ uv run --no-project --locked scripts/check_workflows.py --repo /tmp/mb361-fixture/repo --quiet
workflow-check: ok — 360 checks over 6 workflows and 1 actions, 0 problem(s)
$ echo $?
0

# the self-test catches a vacuous recognizer (STANDALONE_MAKE replaced by `(?!)`)
$ uv run --no-project --locked /tmp/mb361-broken/check_workflows.py
workflow-check: FAIL — rule 15's own self-test is broken (8 problem(s)); no workflow verdict
  below it is worth anything until it is fixed
FAIL rule 15 no longer sees 'make standalone' as a host build
FAIL rule 15 passed a shipping step running 'make standalone'
…
$ echo $?
2
```

## 5. Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.15s

$ cargo test -p blastit --locked --test critical_php_backend
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.28s

$ make rs-test-php-image            # image rebuilt from this tree first: make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_KEEP=1
docker run blastit-php-sapi-e2e:local  # the suites this image built (root)
test result: ok. 5 passed; 0 failed
exit(): status 500  outcome=bailed  cpu_ns=2342781  wall_ns=28441695  cost_ns=28441695  retired pid 489  respawned pid 490 (ppid 463) uid 0  live=[490]
test result: ok. 21 passed; 0 failed
docker run --user 1000:1000 blastit-php-sapi-e2e:local  # the PR_SET_DUMPABLE(0) witness
init pid 9: no libphp/shim; above the view 0 map(s) content-checked, 2 denied (EACCES), 0 gone, of 3 children; euid 1000
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
exit=0

$ make check
…
workflow-check: rule 15 self-test ok — `make standalone` is a host build
workflow-check: ok — 364 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 29 cases passed (workflow=14, matrix=11, wheel-tag=4)
uv run --no-project tools/plan.py validate
plan OK
exit=0

$ make docs-check
uvx --offline zensical build --strict
No issues found
exit=0

$ uv run --no-project tools/critical_guard.py --base 490bb9b --node MB.3.61
critical-guard: 490bb9b..working tree, 8 changed file(s)
  M  crates/blastit/tests/critical_php_backend.rs  [authorized by crates/blastit/tests/**]
  M  crates/blastit/tests/critical_php_embed.rs  [authorized by crates/blastit/tests/**]
critical-guard: OK: all 2 protected change(s) are authorized by node(s) MB.3.61
exit=0

$ make critical-skip-check
protected critical tests hold no skip construct
exit=0

$ uv run --no-project tools/plan.py validate
plan OK
```

## 6. Files changed

| file | change |
|---|---|
| `crates/blastit/src/stage0.rs` | `LIBPHP_PROBES` (version-major search, per-minor `libphp.so`), `LibphpProbe`, `first_existing`; `DynSymbols::exports_php_version` (name only) + `php_version_body`; `NoPhpVersion`/`NoVersion` docs and sentences; `PhpVersionSymbol` fixture + `shared_object_in`; two new tests (`two_embed_packages_serve_from_the_newer_library`, `php_version_presence_is_the_name_not_the_body_shape`) |
| `crates/blastit/tests/critical_php_embed.rs` | `libphp_path()` probe: same version-major order as the boot, musl triplet |
| `crates/blastit/tests/critical_php_backend.rs` | the same helper fix |
| `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` | §1.3 candidate/directory/order/refusals; §4 archive runtime (glibc 2.28 / `apk add libgcc`) and `libphp` 8.3–8.4 |
| `e2e/php-sapi/Dockerfile.two-package` (new) | alpine 3.21 with `php82-embed` **and** `php84-embed`, no `BLASTIT_LIBPHP`, both suites compiled in the image |
| `e2e/php-sapi/Dockerfile.two-package.dockerignore` (new) | the same context trim as the single-package image |
| `e2e/php-sapi/run-two-package.sh` (new) | the witness: layout, per-library `php_version` presence, suites' own status kept, served-branch + resolved-path assertions |
| `scripts/check-exec-env.sh` | `read_version_info()`: a readelf failure is exit 2 with its message, never a silent exit 1 |
| `scripts/check_workflows.py` | `STANDALONE_MAKE`, `runs_host_standalone_build()`, rule 15 docstring, and `self_test_rule15()` run by every audit |
| `docs/knowledge/LESSONS.md` (outside `paths`, see §8) | two entries: co-installed Alpine embed packages + name-vs-body symbol fact; `var=$(readelf …)` under `set -euo pipefail` |

## 7. What only CI can prove

* **aarch64**: the two-package image is x86_64 only; nothing here says the
  per-minor directory probe behaves differently on arm, but no arm64 host ran it.
* **The Alpine CI legs** (`php82-embed`, `php83-embed`, `php84-embed`) still use
  `BLASTIT_LIBPHP` from `scripts/platform_matrix.py`, so CI does not exercise the
  unset-override walk. Moving one leg to "no override" would make the ordering a
  matrix-proved fact; that file is MB.3.18's, not this node's (`paths`).
* **A shipping workflow that actually runs `make standalone`**: the rule and its
  self-test prove the audit refuses it; no workflow does it, so the refused shape
  exists only in the fixture.

## 8. Follow-ups (outside this node's `paths`)

* `scripts/test_packaging_gates.py` should gain the fixture-level case for this
  rule ("replace `scripts/build-standalone-in-image.sh` with `make standalone` in
  `publish-pypi.yml`, expect exit 1") beside MB.3.60's two. It is outside this
  node's `paths`, so the self-test lives in `check_workflows.py` instead and runs
  in `make check` — same assertions, one level lower (a synthetic job rather than
  a mutated YAML).
* `make standalone-glibc` in a shipping workflow is *not* a host build, so it does
  not enter rule 15's loop: a matrix step building through it would skip the
  literal `--max-glibc`/`builder_image` checks (the target itself still passes
  `--max-glibc 2_28` and builds in `manylinux_2_28`). Teaching the rule to accept
  the Makefile target *and* require the floor flags would close that.
* `scripts/platform_matrix.py`: one Alpine leg could carry "no `BLASTIT_LIBPHP`"
  so the two-package-free discovery path is exercised in CI, and
  `e2e/php-sapi/Dockerfile.two-package` could become a declared leg rather than a
  by-hand image.
* `docs/frameworks/php.md` (user page) explains the three spellings but not the
  two-package ordering outcome; one sentence there would match this node's ADR
  text. Out of `paths`.
* `docs/knowledge/LESSONS.md` was edited outside `paths`: the lead's LOG records
  the MB.3.58/.60 precedent and asked to be told what was added — §6 names both
  entries, and nothing existing was changed (verified with `git diff`: 2
  insertions, 0 deletions).
