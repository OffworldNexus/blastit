# RESULT: MB.3.39 — CI witness for the real-pair PHP legs

STATUS: done

The `shim` job of `.github/workflows/php-sapi.yml` now runs the two protected PHP
suites against a real NTS `libphp8.4.so` + shim, and a guard step fails the job if
they take their no-PHP refusal branch. `ci.yml` is untouched, so `make check` stays
PHP-free.

## What changed (only the two `paths` files)

`.github/workflows/php-sapi.yml` (still one job, `timeout-minutes: 30`):

1. the `make php-sapi-e2e` step passes `PHP_SAPI_KEEP=1`, so the image it builds
   survives for the next step;
2. new step `make rs-test-php-image` — the one command that builds both test
   binaries on the runner and executes them against the image's pair, and writes
   its own wall time into `$GITHUB_STEP_SUMMARY`;
3. new step `guard: the pair legs must serve, not refuse` — fails the job when
   the suites print their own no-pair refusal line, and also when the served
   evidence is missing (so a rewording cannot silently pass); the markers are
   anchored on the suites' printout, never on a bare stage-0 boot sentence;
4. drive-by: the pre-existing step name
   `- name: make php-sapi-e2e (Docker: debian:trixie-slim + …)` was **not valid
   YAML** (a plain scalar may not contain `: `); it is quoted now. `git show
   HEAD:.github/workflows/php-sapi.yml` fails `yaml.safe_load` too — the whole
   workflow was unparseable before this node (evidence in §1).

`Makefile`:

- `rs-test-php` — builds the shim this host can (`php-sapi`) and runs
  `cargo test -p blastit --locked --test critical_php_backend --test critical_php_embed -- --nocapture`
  with `BLASTIT_PHP_SHIM_DIR=$(PHP_SAPI_OUT)`. Works on a PHP-free host too (the
  suites then prove the refusal branch, exactly as `make check` does).
- `rs-test-php-image` — what CI calls: `cargo test … --no-run` here, then the two
  binaries run inside the `php-sapi-e2e` image with
  `BLASTIT_LIBPHP=/usr/lib/libphp8.4.so` and
  `BLASTIT_PHP_SHIM_DIR=<repo>/target/php-sapi-image` (copied out of the image).
- `PHP_SAPI_KEEP`, `PHP_SAPI_IMAGE*`, `PHP_SUITE_ARGS` variables, `help` one-liners,
  `.PHONY`, and the header comment block. `check`'s prerequisite list is
  unchanged — the pair legs are deliberately **not** in it.
- `php-sapi-e2e` now forwards `--image "$(PHP_SAPI_IMAGE)"`, so that variable is
  the truth for **both** targets (`php_sapi_probe.py` otherwise only reads the
  environment or its own default and would build `…-e2e:local` while
  `rs-test-php-image` then looked for the tag it was told).

Why the binaries are built on the runner and only executed in the image: that
image is deliberately minimal (Debian slim + PHP + a Rust toolchain) and cannot
compile `blastit` — `pg_query`'s build script needs bindgen/libclang, which it
does not install (`error: failed to run custom build command for pg_query v6.2.1`
→ `bindgen-0.66.1/lib.rs`, reproduced in this session). The binaries need only
glibc (`ldd` below), and the image's Debian is newer than the runner's Ubuntu.

## Evidence

### 0. The pair the image really has (checked, not guessed)

`debian:trixie-slim` + `libphp8.4-embed`, asked directly:

```console
$ docker run --rm debian:trixie-slim sh -c 'apt-get update -qq && apt-get install -y -qq libphp8.4-embed && ls -l /usr/lib/libphp8.4.so /usr/lib/x86_64-linux-gnu/libphp8.4.so'
ls: cannot access '/usr/lib/x86_64-linux-gnu/libphp8.4.so': No such file or directory
-rw-r--r-- 1 root root 6049496 Oct 21 07:53 /usr/lib/libphp8.4.so
```

So the Debian multi-arch directory is **not** used and the node's suggested
`/usr/lib/x86_64-linux-gnu/libphp8.4.so` would have been wrong: the pair is
`BLASTIT_LIBPHP=/usr/lib/libphp8.4.so`. `e2e/php-sapi/run-probe.sh` agrees
(`libphp="/usr/lib/libphp${minor}.so"`). The shim the image's own `make php-sapi`
leaves is `/src/target/php-sapi/libblastit_php84-x86_64-gnu.so`:

```console
$ docker run --rm blastit-php-sapi-e2e:local sh -c 'ls -l /usr/lib/libphp8.4.so; ls -l target/php-sapi/'
-rw-r--r-- 1 root root 6049496 Oct 21 07:53 /usr/lib/libphp8.4.so
-rwxr-xr-x 1 root root 17384 libblastit_php84-x86_64-gnu.so
```

The executed binaries need nothing but glibc, so host-build → guest-run is safe:

```console
$ ldd target/debug/build/blastit/0b784173b4fc9736/out/critical_php_embed-… | sed -n 2,4p
	libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1
	libc.so.6 => /lib/x86_64-linux-gnu/libc.so.6
	/lib64/ld-linux-x86-64.so.2
```

### 1. The workflow YAML parses

The node's suggested command has no `yaml` module in a plain `uv run --no-project`
(`ModuleNotFoundError: No module named 'yaml'`), so `--with pyyaml`:

```console
$ uv run --no-project --with pyyaml python -c 'import yaml;d=yaml.safe_load(open(".github/workflows/php-sapi.yml"));j=d["jobs"]["shim"];print("timeout",j["timeout-minutes"]);print([s.get("name") for s in j["steps"]][-3:])'
timeout 30
['make php-sapi-e2e (Docker: debian:trixie-slim + php8.4-dev + libphp8.4-embed)', 'make rs-test-php-image (critical_php_backend + critical_php_embed on the real pair)', 'guard: the pair legs must serve, not refuse']
```

Before the quote fix, the same loader on `HEAD`'s file:

```console
$ uv run --no-project --with pyyaml python -c 'import yaml;yaml.safe_load(open("/tmp/opencode/mb339/head.yml"))'
yaml.scanner.ScannerError: mapping values are not allowed here
  in "/tmp/opencode/mb339/head.yml", line 62, column 40
```

No other workflow had that shape (`grep -rE '^\s*- name: [^'\"'].*: ' .github/workflows/`
→ nothing), and all six parse:

```console
$ for f in .github/workflows/*.yml; do uv run --no-project --with pyyaml python -c "import yaml;yaml.safe_load(open('$f'))" && echo "ok $f"; done
ok .github/workflows/ci.yml
ok .github/workflows/docs.yml
ok .github/workflows/live.yml
ok .github/workflows/php-sapi.yml
ok .github/workflows/publish-pypi.yml
ok .github/workflows/wheels.yml
```

### 2. `make rs-test-php` — this host's real pair (Debian 13, `libphp8.4-embed` 8.4.27)

```console
$ make rs-test-php 2>&1 | tail
php-sapi: cc -> …/target/php-sapi/libblastit_php84-x86_64-gnu.so (/usr/bin/php-config8.4 reports 8.4.27)
php-sapi: 1 shim(s) for x86_64-gnu in …/target/php-sapi; skipped: 8.3 8.2
BLASTIT_PHP_SHIM_DIR=…/target/php-sapi cargo test -p blastit --test critical_php_backend --test critical_php_embed -- --nocapture
served: HTTP/1.1 200 OK  cpu_ns=14309088  wall_ns=32757965  (libphp /usr/lib/libphp8.4.so)
symlinked /tmp/blastit-php-backend-3107778-symlink/libphp8.so -> /usr/lib/libphp8.4.so  served: HTTP/1.1 200 OK
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
exit(): status 500  outcome=view_died  cpu_ns=3598035  wall_ns=19800770  cost_ns=19800770  retired pid 3101571  respawned pid 3101585 (ppid 3101532) uid 1000  live=[3101585]
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.82s
real	0m2.132s
```

All four `# Acceptance` bullets are in that tail: `200` through `view.chan` with
`cpu_ns`, the containment + respawn, the maps leg, and the another-minor refusal.
Note the embed suite reports **9** passed, not the 8 in the node's sample: the
file at `HEAD` already has 9 `#[test]`s (`git show HEAD:crates/blastit/tests/critical_php_embed.rs | grep -c '^#\[test\]'` → 9).

### 3. `make rs-test-php-image` — exactly what CI runs

```console
$ make rs-test-php-image 2>&1 | tail
cargo test -p blastit --test critical_php_backend --test critical_php_embed --no-run   # build here, run in the image
docker run blastit-php-sapi-e2e:local target/debug/build/blastit/3b9e…/out/critical_php_backend-3b9e… --nocapture
served: HTTP/1.1 200 OK  cpu_ns=37412657  wall_ns=58911115  (libphp /usr/lib/libphp8.4.so)
symlinked /tmp/blastit-php-backend-1-symlink/libphp8.so -> /usr/lib/libphp8.4.so  served: HTTP/1.1 200 OK
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
docker run blastit-php-sapi-e2e:local target/debug/build/blastit/0b78…/out/critical_php_embed-0b78… --nocapture
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.08s
real	0m8.847s
```

8.8 s here with a warm host `target/`; in CI it also pays the first
`cargo test -p blastit --no-run` (the workspace's first build in that job) and
then the image runs, which is what the step's summary records
(`wall time of this step: N s (job bound: 30 min)`), so the number comes from the
job itself rather than a claim.

### 3b. `PHP_SAPI_IMAGE` is honoured by both targets

`php-sapi-e2e` now forwards `--image "$(PHP_SAPI_IMAGE)"` (`php_sapi_probe.py`
otherwise reads only the environment or its own default), so one variable names
the tag on both sides:

```console
$ make php-sapi-e2e PHP_SAPI_IMAGE=blastit-php-sapi-override:local PHP_SAPI_KEEP=1
php-sapi-e2e: docker build --build-arg PHP_MINOR=8.4 -t blastit-php-sapi-override:local
  ok  DT_NEEDED: libc.so.6 (no libphp)
php-sapi-e2e: probe — no filter, egress denied (INI layer alone)

$ docker images --format '{{.Repository}}:{{.Tag}}' | grep php-sapi
blastit-php-sapi-override:local

$ make rs-test-php-image PHP_SAPI_IMAGE=blastit-php-sapi-override:local; echo "exit=$?"
docker run blastit-php-sapi-override:local target/debug/build/blastit/3b9e…/out/critical_php_backend-3b9e… --nocapture
served: HTTP/1.1 200 OK  cpu_ns=18473076  wall_ns=37526169  (libphp /usr/lib/libphp8.4.so)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.91s
exit=0
```

A tag that does not exist is refused by name rather than silently falling back to
the default image:

```console
$ make rs-test-php-image PHP_SAPI_IMAGE=does-not-exist:local; echo "exit=$?"
rs-test-php-image: no image 'does-not-exist:local'; build it first:
  make php-sapi-e2e PHP_SAPI_KEEP=1
make: *** [Makefile:552: rs-test-php-image] Error 1
make exit=2
```

The override image was removed afterwards (`docker image rm
blastit-php-sapi-override:local`); `…-e2e:local` is the tag both targets default to
and the one the workflow uses.

### 4. The guard: a PHP-free log fails the job, a served log passes

The guard step's script is extracted **from the YAML** and run three times, so
this is the shipped text, not a paraphrase:

```console
$ uv run --no-project --with pyyaml python -c '...open("/tmp/opencode/mb339/guard.sh","w").write(guard["run"])'
$ RUNNER_TEMP=/tmp/opencode/mb339/refusal bash guard.sh; echo "guard exit=$?"
::error::the PHP pair legs took the refusal branch: this runner has no usable libphp + shim, so nothing was proved
18:refused: blastit dev: route "app": [route.php] entry "public/index.php": no usable NTS libphp was found (searched BLASTIT_LIBPHP, then libphp8.4.so, …)
30:refused (no usable libphp + shim on this host): blastit dev: route "app": … no usable NTS libphp was found …
guard exit=1
```

```console
$ RUNNER_TEMP=/tmp/opencode/mb339/served bash guard.sh; echo "guard exit=$?"
the pair legs served: 2 200(s), 3 respawn(s)
guard exit=0
```

```console
$ RUNNER_TEMP=/tmp/opencode/mb339/empty bash guard.sh; echo "guard exit=$?"   # empty log
::error::the real-pair evidence is missing from the suites' output: served: HTTP/1\.1 200 OK
::error::the real-pair evidence is missing from the suites' output: retired pid .* respawned pid
guard exit=1
```

The refusal transcript is a **real** PHP-free run, not a fixture: this host has
the pair, so its `/usr/lib/libphp8.4.so` is masked inside a mount namespace
(`unshare -m -r; mount --bind /dev/null /usr/lib/libphp8.4.so`) and the same
target runs green (5 + 9 passed) while printing the refusal markers — exactly the
silently-PHP-free runner this guard exists for:

```console
$ unshare -m -r bash -c 'mount --bind /dev/null /usr/lib/libphp8.4.so; make rs-test-php' | grep -E 'no usable libphp \+ shim on this host|test result'
refused (no usable libphp + shim on this host): … no usable NTS libphp was found …
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.58s
```

Why the guard anchors on `^refused( …` / `^refused: …` and not on a boot
sentence or on bare `refused`: five legs refuse **on purpose on a healthy host**
(ZTS, not-a-library, another-minor, inode-pin, probe-timeout) and print
`refused (stage 0, …)`, `refused at boot …`, `refused (stage 1 …)` — and the
stage-0 sentences `no usable NTS libphp was found`, `no shim libblastit_php…`
and `install the PHP embed package` are text a *healthy* leg's captured output
can contain. On the served transcript `grep -c refused` is 6 and
`grep -c 'install the PHP embed package'` is 5, all of them healthy; the shipped
pattern matches 0 of them and both lines of the refusal transcript (18 and 30),
which is what the two runs above show. Fail-closed is the second half: the
served evidence (`served: HTTP/1.1 200 OK`, `retired pid … respawned pid`) is
only printed by legs that really served, so an empty or truncated log fails.

### 5. `make check` on this host (PHP-free for the pair, as CI is)

```console
$ make check
clippy      -p blastit{,-view} (standalone)
env -u PYO3_PYTHON -u PYTHONHOME cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.14s
cargo clippy -p blastit-view --no-default-features --lib --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.04s
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK

real	1m37.942s
make check exit=0
```

`ci.yml` is untouched (`git diff --stat .github/workflows/ci.yml` → empty): the
PHP-free path and its root-phase `--test-threads=1` invocation are unchanged, and
both suites still take the refusal branch there. `check`'s prerequisites are
unchanged too, so the pair legs never become a `make check` requirement.

I cannot run GitHub Actions from here, so the step ordering, the 30-minute bound
and `$GITHUB_STEP_SUMMARY` are unexercised; everything the job runs is exercised
above (`make php-sapi-e2e PHP_SAPI_KEEP=1`, `make rs-test-php-image`, the guard
script taken out of the YAML).

Re-checked under CI's own env (`env: CARGO_TERM_COLOR: always`), because the
executable path is parsed out of cargo's output:

```console
$ CARGO_TERM_COLOR=always make rs-test-php-image; echo "exit=$?"
served: HTTP/1.1 200 OK  cpu_ns=20471595  wall_ns=39809839  (libphp /usr/lib/libphp8.4.so)
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.24s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.89s
exit=0
```

**Watch on run 1 (budget).** The new step's `timeout-minutes: 15` and the job's
30 minutes now also cover the first *cold* `cargo test -p blastit --no-run` on the
runner — the job had never built `blastit` before this node — beside the 1.65 GB
`debian:trixie-slim` + PHP + rustup image, on a ~14 GB runner. No local run can
prove that first-cold number: here the workspace was already warm, and the whole
`make rs-test-php-image` was 8.8 s. If run 1 comes close to either bound, the fix
is `Swatinem/rust-cache` warming (already in the job) plus moving the suite step
above the Docker steps, not a bigger timeout.

Final state re-verified after the review fix (same commands, same tags):
`make check` exit 0, `make rs-test-php` exit 0 (5 + 9 passed, `served: HTTP/1.1
200 OK  cpu_ns=14381683`), `make rs-test-php-image` exit 0 (5 + 9 passed in
`blastit-php-sapi-e2e:local`), the guard 0/1/1 as in §4, and the workflow parses.

Evidence was collected on `3c2afab` (`[MB.3.38] …`) plus this node's two files
uncommitted.

## Notes

- Both targets use `-- --nocapture`: the evidence lines are `println!`s of
  **passing** tests, which cargo otherwise captures and drops, and the guard needs
  them. `CARGO_TERM_COLOR=never` is set for the `--no-run` call because the
  workflow exports `CARGO_TERM_COLOR=always` and the executable path is parsed out
  of cargo's `Executable … (<path>)` line.
- `docker run` uses `--user $(id -u):$(id -g)` so the guest runs the legs at the
  same unprivileged uid the local run uses (the maps leg's `EACCES` half).
- Hazard found the hard way: while `make check` was running I ran
  `cargo test -p blastit --no-run` (standalone) in the same `target/`, which
  overwrote `target/debug/blastit` mid-`rs-test` and made `critical_static_cli`
  fail with "built without feature \"python\"" — a target-dir concurrency effect,
  not a code change. `cargo` does rebuild the bin on a feature change (verified:
  `ldd target/debug/blastit | grep -c python` → 0 after a standalone build, 1 after
  `--features python`), so a clean `make check` after `make rs-test-php` is green.

## Follow-ups

1. `crates/blastit/tests/critical_php_embed.rs`'s module doc ("CI coverage
   (`MB.3.13.4` review, follow-up for the CI owner)", lines 47-57) still says
   *"until then the containment leg has no CI witness"* — false as of this node.
   That file is protected and not in this node's `paths`, so its owner (a
   `critical_php_embed` node) should delete the paragraph and point at
   `make rs-test-php` / the `shim` job.
2. `docs/knowledge/LESSONS.md` (not in `paths`) deserves one line: an ambient
   `BLASTIT_LIBPHP=/nonexistent` cannot produce the refusal branch on a host that
   has the pair — the suite's probe honours the variable while the boot it spawns
   is `env_clear`ed and discovers the library itself, so the suite panics by
   design ("the probe found no usable libphp + shim, yet the boot served"). Use a
   mount namespace (or a container without the package) instead.
3. Second gotcha for the same file: `cargo test --no-run` for the default
   (standalone) configuration writes `target/debug/blastit`, and running it
   **while** `make check` is in progress makes `rs-test`'s
   `critical_static_cli` fail with "built without feature \"python\"" — the
   feature change *does* rebuild the bin (verified), but two cargo invocations in
   one target dir must not overlap.
4. The runner's Ubuntu archive has no `libphp8.4-embed`/`php8.4-dev`; putting the
   pair natively on `ubuntu-latest` would need a third-party PPA decision
   (recorded in the workflow comment, deliberately not taken here).
5. `crates/blastit-php-sapi`'s e2e image installs neither libclang nor `clang`, so
   `cargo test -p blastit` can never run inside it (`pg_query` → bindgen). If a
   later node wants the whole build in the image, `e2e/php-sapi/Dockerfile` needs
   `libclang-dev`; that file is not in this node's `paths`.
