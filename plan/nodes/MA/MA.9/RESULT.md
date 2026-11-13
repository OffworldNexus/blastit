# Result MA.9

## Summary

`.github/workflows/ci.yml` only (+158/−13, two hunks in the `smoke (root)` job: the job header and its
steps). The job can no longer hang, and the phase that blocked is named in the log while it blocks:

1. `timeout-minutes: 20` on the job, plus `timeout-minutes` on **every** phase that can wait (chmod 1,
   preflight 2, build 10, privileged run 10, doc tests 3) and a per-binary `timeout 90s` — 90 s × the
   crate's six binaries is 9 minutes, inside the privileged step's 10-minute bound, so a slow binary
   still fails with *its own* message. A re-run of the 35-minute idle is now a red check in
   ≤ 20 minutes.
2. A preflight that uses `sudo --non-interactive` for every privileged call (the only bare `sudo` is
   `sudo --version`, which needs no privileges) and echoes what it resolved
   (`sudo --version`, root's uid, both `cargo --version`s, `PYO3_PYTHON`, the homes, and
   `BLASTIT_TEST_PG_URL=set` — never the URL, never the environment). A sudo that would have
   prompted fails in milliseconds with an `::error::` that names the cause.
3. **Build as the runner user, execute as root**: `cargo test -p blastit --locked --no-run
   --message-format=json` lists every test binary, then each one is run under
   `sudo --non-interactive env … <binary> --test-threads=1 --nocapture`, streamed through `tee`.
   Root never **builds** the test binaries (it only executes them), so root cannot start a crate
   rebuild, and an in-flight log always exists.
4. Coverage is unchanged: the same six test binaries `cargo test -p blastit` builds (lib unit, bin
   unit, the three `tests/critical_*.rs`, `tests/smoke.rs`) still run as uid 0 with
   `--test-threads=1`, and the crate's doc tests still run under root in their own step. That doc-test
   step is the one place root still runs `cargo` — `cargo test --doc` (rustdoc; it compiles snippets,
   not the crate's test targets, which are already built and handed over as paths), bounded at 3
   minutes and reporting `running 0 tests` for `blastit` today.

Root cause, in two lines: **candidate 1 (a password prompt) is refuted** — the same job's earlier
`sudo chmod …` succeeds on that runner, and `--non-interactive` now proves it in milliseconds — and
**candidate 2 (root compiling) is refuted by measurement** — a root `cargo test -p blastit` compiles
**zero** units (`Finished in 0.10s`) with the job's exact env handoff. That leaves
**candidate 3, a root-phase block**, and it is narrowed to the only unbounded waits in `smoke.rs`
(`connect_as`'s `waitpid(pid, …, 0)` and `Server::drop`'s `child.wait()`); it cannot be settled on a
developer machine, so the job now names the blocked test in its own log. Details and the exact output
to expect are below.

## Root cause: refuted, refuted, and what remains

### Candidate 1 — `sudo-rs` blocking on a password prompt: **refuted**

* The same job's step at line 117 (`sudo chmod go-w /opt && sudo chmod -R go-w /opt/hostedtoolcache`)
  ran minutes earlier on that runner and succeeded, so the NOPASSWD rule matches plain
  `sudo <command>` invocations there; sudo-rs decides the policy per invocation, and both
  invocations are the same shape.
* MA.8's own record of run 43407063422: the step "got past the sudo-rs lookup and then hung" — a
  prompt would have to come *before* the lookup, and would have produced a red step in seconds.
* Nothing in the MA.7 diff makes a policy decision different: the added words are `NAME=value`
  arguments to `env`, all space-free, so sudo-rs has nothing to re-split.
* I cannot see the failed step's log to close this by observation (see "silence was unavailable"
  below), so the job now both *prevents* it (`--non-interactive` everywhere) and *proves* it in the
  first second of the run.

### Candidate 2 — root rebuilding the crate graph: **refuted as the cause, and eliminated by construction**

Measured on this machine, with the job's exact env handoff, after the runner-user build:

```console
$ PY=…/python3.14
$ sudo -n env "PATH=$HOME/.cargo/bin:$PATH" "RUSTUP_HOME=$HOME/.rustup" "CARGO_HOME=$HOME/.cargo" \
    "PYO3_PYTHON=$PY" cargo test -p blastit --locked --no-run
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s
EXIT:0
```

Zero units compiled, four times in a row, alternating root and runner (`root again` → 0.10 s,
`as runner user` → 0.09 s, `back to root` → 0.09 s). When a unit *does* flip, it is `blastit` alone
and it says so in the first second:

```console
    Compiling blastit v0.1.1 (/…/crates/blastit)
    Finished `test` profile [unoptimized + debuginfo] target(s) in 3.47s
```

so the worst case root could add on CI is one crate — ~68 s, the duration of the whole
`cargo build -p blastit --tests` step in the failed run (10:10:54–10:12:02) — never 35 minutes, and
never silently.

`LD_LIBRARY_PATH` (the one variable sudo strips that the build has) is **not** part of cargo's
fingerprint; building with it, rebuilding without it, then with it again, all report `Finished` and
compile nothing:

```console
== A: runner env WITH LD_LIBRARY_PATH ==     Finished … in 0.10s
== B: same minus LD_LIBRARY_PATH (sudo) ==   Finished … in 0.09s
== C: back to A ==                           Finished … in 0.09s
```

The *ownership* half of the candidate is real, and I reproduced it: a root `cargo` run left **5,572
root-owned paths** under `target/` (fingerprints, `.o` files, and — interestingly — the test binaries
themselves, which `cargo` 1.100 places in `target/debug/build/blastit/<hash>/out/<name>-<hash>`). My
first `chown` ran as the runner user and was refused for every one of them (`Operation not permitted`);
only `sudo chown` cleared them, and the runner user could not build again until it did. The new job
never builds the test binaries as root, so that cannot accumulate on CI — the one remaining root
`cargo` call is `--doc`, which rustdoc keeps to snippet compilation and which has its own 3-minute
bound (see item 4 above).

### Candidate 3 — a test blocking as root: the surviving candidate, narrowed

It is not "the suite got slow": `tests/smoke.rs` at `2c05139` — the develop commit whose `smoke (root)`
step took **5 s** — already contained **31** `#[test]`s, the same number as today:

```console
== 2c05139 (2026-10-27 Release 0.1.0)
crates/blastit/tests/smoke.rs                      31 tests
lib/bin unit tests at 2c05139: 132
== HEAD (599f912)
crates/blastit/tests/critical_check_engine.rs      16 tests
crates/blastit/tests/critical_config_and_presets.rs 11 tests
crates/blastit/tests/critical_dev_and_fail_closed.rs 19 tests
crates/blastit/tests/smoke.rs                      31 tests
lib/bin unit tests at HEAD: 179
```

So a 5 s → 35+ min delta is a *block*, not growth. `tests/smoke.rs` is the only file whose behaviour
changes as uid 0 (`needs_root`), and every wait in it is deadline-bounded
(`READY_TIMEOUT`/`EXIT_TIMEOUT` = 30 s) **except two**, both of which outlive every deadline if the
process they wait for never exits:

* `crates/blastit/tests/smoke.rs:2109` — `connect_as`'s `unsafe { libc::waitpid(pid.as_raw(), &mut status, 0) }`;
* `crates/blastit/tests/smoke.rs:408` — `Server::drop`'s `let _ = self.child.wait();`.

I could not settle *which* one, and not even which binary, locally: **the privileged suite cannot run
from a `0700` checkout at all.** My root run of the full command fails in 4 s, and the refusal is
blastit's own visibility probe:

```console
$ sudo -n env "PATH=…" "RUSTUP_HOME=…" "CARGO_HOME=…" "PYO3_PYTHON=$PY" "BLASTIT_TEST_PG_URL=" \
    cargo test -p blastit --locked -- --test-threads=1
…
---- unprivileged_production_has_no_single_uid_fallback stdout ----
panicked at crates/blastit/tests/critical_dev_and_fail_closed.rs:1283:5:
/home/remy (mode 0700) is not traversable by uid 21000 — …
test result: FAILED. 18 passed; 1 failed; … finished in 2.75s
EXIT:101 WALL:4s
```

(The CI runner's `/home/runner` and workspace are world-traversable, which is why this is specific to
a developer's `0700` home; the same class of check is what the job's `chmod go-w /opt` step fixes.)

**"No log output" is not evidence, and I did not treat it as such:** an in-flight job has no
downloadable log at all (MA.8 verified `gh api …/jobs/131823878765/logs` → 404 `BlobNotFound`), so
silence was *unavailable*, not absent. The next run produces it.

## What changed

Two hunks, `.github/workflows/ci.yml`, `smoke (root)` (the job header, and its steps). Full text of
what was added or changed:

```yaml
    timeout-minutes: 20
```

```yaml
      # This step runs before the preflight, so it must not be able to wait on
      # anything either: `--non-interactive` turns "sudo wants a password" into an
      # instant readable error, and the step is bounded so a wedged `chmod -R` is a
      # red step rather than a job that idles to its timeout (MA.9).
      - name: Make /opt and the tool cache non-world-writable (as root)
        timeout-minutes: 1
        run: |
          set -euo pipefail
          sudo --non-interactive chmod go-w /opt
          sudo --non-interactive chmod -R go-w /opt/hostedtoolcache
```

```yaml
      # Preflight: prove, in seconds, that root is reachable *without a prompt*
      # and that cargo resolves for root with the homes this job hands it, and
      # echo what was resolved. `--non-interactive` is the point: a sudo that
      # would have asked for a password now exits non-zero immediately instead of
      # blocking on /dev/tty with nothing in the log (MA.9 candidate 1). Nothing
      # here prints the whole environment: only the variables the test binaries
      # need are echoed, and the PostgreSQL URL is reduced to "set".
      - name: Preflight (sudo is non-interactive, cargo runs as root)
        timeout-minutes: 2
        run: |
          set -euo pipefail
          echo "sudo: $(sudo --version | head -1)"
          if ! root_uid="$(sudo --non-interactive id -u)"; then
            echo "::error::sudo --non-interactive id -u failed: this runner's sudo wants a password (or denies the command), so the privileged step could only block on a prompt. That is MA.9 candidate 1; the NOPASSWD rule no longer matches what this job runs."
            exit 1
          fi
          echo "sudo: root uid $root_uid, resolved non-interactively"
          echo "runner: uid=$(id -u) user=$(id -un) cwd=$PWD"
          echo "runner cargo: $(command -v cargo) ($(cargo --version))"
          if [ -z "${PYO3_PYTHON:-}" ]; then
            echo "::error::PYO3_PYTHON is empty: the setup-python step did not reach GITHUB_ENV, and every test binary would lose libpython (tests/smoke.rs derives LD_LIBRARY_PATH from it)."
            exit 1
          fi
          if [ -z "${BLASTIT_TEST_PG_URL:-}" ]; then
            echo "::error::BLASTIT_TEST_PG_URL is empty: the service's URL did not reach the environment, and the smoke tests would silently skip every PostgreSQL check."
            exit 1
          fi
          echo "runner python: PYO3_PYTHON=$PYO3_PYTHON"
          echo "homes: RUSTUP_HOME=${RUSTUP_HOME:-<unset>} CARGO_HOME=${CARGO_HOME:-<unset>}"
          echo "postgres: BLASTIT_TEST_PG_URL=set"
          sudo --non-interactive env \
            "PATH=$HOME/.cargo/bin:$PATH" \
            "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" \
            "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" \
            "PYO3_PYTHON=$PYO3_PYTHON" \
            "BLASTIT_TEST_PG_URL=$BLASTIT_TEST_PG_URL" \
            sh -c 'echo "root: uid=$(id -u) home=$HOME"; echo "root cargo: $(command -v cargo) ($(cargo --version))"; echo "root PYO3_PYTHON=$PYO3_PYTHON"; test -x "$PYO3_PYTHON" || { echo "::error::root cannot execute PYO3_PYTHON=$PYO3_PYTHON"; exit 1; }'
```

```yaml
      - name: Build the test binaries (runner user, no root)
        timeout-minutes: 10
        run: |
          set -euo pipefail
          cargo test -p blastit --locked --no-run --message-format=json > "$RUNNER_TEMP/artifacts.json"
          python3 - "$RUNNER_TEMP/artifacts.json" "$RUNNER_TEMP/test-binaries.txt" <<'PY'
          import json
          import sys

          # `--message-format=json` is JSON Lines, not one array.
          with open(sys.argv[1], encoding="utf-8") as stream:
              artifacts = [json.loads(line) for line in stream if line.strip()]
          binaries = sorted(
              artifact["executable"]
              for artifact in artifacts
              if artifact.get("reason") == "compiler-artifact"
              and (artifact.get("profile") or {}).get("test") is True
              and artifact.get("executable")
          )
          if not binaries:
              sys.exit("no test binary in cargo's JSON: there is nothing to run as root")

          with open(sys.argv[2], "w", encoding="utf-8") as out:
              out.write("\n".join(binaries) + "\n")
          print(f"{len(binaries)} test binaries, built as the runner user:")
          for binary in binaries:
              print(f"  {binary}")
          PY
```

```yaml
      # `timeout` bounds each binary instead of the whole step. The budget adds
      # up: 90 s per binary, and the crate has six test binaries today, so the
      # worst case is 9 minutes inside this step's 10-minute bound — every binary
      # reaches its own timeout and its own `::error::` naming the blocked test
      # instead of being cut off by the step. (The whole suite takes ~5 s when
      # healthy, so 90 s is ~18x headroom; a seventh test target would have to
      # raise this step's bound with it.)
      - name: Run every test binary as root, one at a time
        timeout-minutes: 10
        run: |
          set -euo pipefail
          mapfile -t binaries < "$RUNNER_TEMP/test-binaries.txt"
          log="$RUNNER_TEMP/root-tests.log"
          # 90 s per binary: 6 × 90 s = 9 min, inside this step's 10-minute bound.
          binary_timeout=90
          : > "$log"
          echo "running ${#binaries[@]} test binaries as uid 0, one at a time"
          for binary in "${binaries[@]}"; do
            echo "==> root: $binary"
            set +e
            timeout --kill-after=30s "${binary_timeout}s" \
              sudo --non-interactive env \
                "PATH=$HOME/.cargo/bin:$PATH" \
                "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" \
                "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" \
                "PYO3_PYTHON=$PYO3_PYTHON" \
                "BLASTIT_TEST_PG_URL=$BLASTIT_TEST_PG_URL" \
                stdbuf -oL -eL "$binary" --test-threads=1 --nocapture 2>&1 \
              | tee -a "$log"
            status=${PIPESTATUS[0]}
            set -e
            if [ "$status" -eq 124 ] || [ "$status" -eq 137 ]; then
              echo "::error::$binary did not finish as root within ${binary_timeout}s: the last 'test <name> ...' line in root-tests.log is the test that blocked (MA.9 candidate 3), not a deadlock in the job."
              exit 1
            fi
            if [ "$status" -ne 0 ]; then
              echo "::error::$binary failed as root (exit $status)"
              exit "$status"
            fi
          done
```

```yaml
      - name: Run the crate's doc tests as root
        timeout-minutes: 3
        run: |
          set -euo pipefail
          sudo --non-interactive env \
            "PATH=$HOME/.cargo/bin:$PATH" \
            "RUSTUP_HOME=${RUSTUP_HOME:-$HOME/.rustup}" \
            "CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}" \
            "PYO3_PYTHON=$PYO3_PYTHON" \
            cargo test -p blastit --doc --locked -- --test-threads=1 \
            2>&1 | tee "$RUNNER_TEMP/root-doctests.log"
```

```yaml
      - name: Upload the root test logs
        if: failure()
        uses: actions/upload-artifact@v4
        with:
          name: smoke-root-logs
          path: |
            ${{ runner.temp }}/root-tests.log
            ${{ runner.temp }}/root-doctests.log
            ${{ runner.temp }}/artifacts.json
          if-no-files-found: ignore
```

Removed: `- run: cargo build -p blastit --tests --locked` (fully subsumed by the `--no-run` build
step, which now also names the binaries) and the `sudo env …` line MA.7 wrote. Every `${…}` is still
expanded by *this* shell before sudo, so the homes still resolve to the runner user's. `sudo --non-interactive`
is spelled the same by classic sudo and sudo-rs (`sudo-rs(8)`: `-n, --non-interactive: Avoid
prompting the user for input of any kind`), so the step is valid either way.

## Acceptance evidence

### 1. The step is self-diagnosing and bounded

YAML parses, and every `run` script in the job is valid bash (extracted from the parsed YAML, so this
is what Actions will execute, not a copy):

```console
$ uv run --no-project --with pyyaml python -c 'import yaml; d=yaml.safe_load(open(".github/workflows/ci.yml")); print("YAML OK; jobs:", len(d["jobs"]), "| smoke timeout:", d["jobs"]["smoke"]["timeout-minutes"])'
YAML OK; jobs: 10 | smoke timeout: 20
$ for f in /tmp/opencode/steps/step*.sh; do bash -n "$f"; done
bash -n: all OK
```

**Happy path** (executed for real on this host, `RUNNER_TEMP`/`PYO3_PYTHON`/URL set as the job sets them):

```console
$ bash /tmp/opencode/steps/step6.sh            # the preflight step, verbatim
sudo: Sudo version 1.9.16p2
sudo: root uid 0, resolved non-interactively
runner: uid=1000 user=remy cwd=/home/remy/offworld-nexus/blastit
runner cargo: /home/remy/.cargo/bin/cargo (cargo 1.100.0 (3acc79c0c 2026-11-05))
runner python: PYO3_PYTHON=/…/python3.14
homes: RUSTUP_HOME=<unset> CARGO_HOME=<unset>
postgres: BLASTIT_TEST_PG_URL=set
root: uid=0 home=/root
root cargo: /home/remy/.cargo/bin/cargo (cargo 1.100.0 (3acc79c0c 2026-11-05))
root PYO3_PYTHON=/…/python3.14
EXIT:0
```

(No secret is printed: the PostgreSQL URL is reduced to `set`. That also shows the sudo-rs failure
mode being *proved*: root's `cargo --version` answering is what candidate 1 would have blocked.)

**Denied / broken path — a sudo that would have prompted.** A `sudo` shim that rejects
`-n`/`--non-interactive` with a password error, exactly as a password-requiring policy does:

```console
$ PATH=/tmp/opencode/shim:$PATH bash /tmp/opencode/steps/step6.sh
sudo: Sudo version 1.9.16p2 (simulated password-required policy)
sudo: a password is required
::error::sudo --non-interactive id -u failed: this runner's sudo wants a password (or denies the command), so the privileged step could only block on a prompt. That is MA.9 candidate 1; the NOPASSWD rule no longer matches what this job runs.
EXIT:1   (immediately — no prompt, no hang)
```

**Denied path — a silently-skipping PostgreSQL service** (the MA.7 lesson, now loud):

```console
$ BLASTIT_TEST_PG_URL="" bash /tmp/opencode/steps/step6.sh
…
::error::BLASTIT_TEST_PG_URL is empty: the service's URL did not reach the environment, and the smoke tests would silently skip every PostgreSQL check.
EXIT:1
```

**Bounded, streaming, and self-naming.** The build step names every binary from cargo's own JSON:

```console
$ bash /tmp/opencode/steps/step7.sh            # the build step, verbatim
    Finished `test` profile [unoptimized + debuginfo] target(s) in 0.10s
6 test binaries, built as the runner user:
  …/target/debug/build/blastit/40841d6f9f4794cc/out/blastit-40841d6f9f4794cc
  …/target/debug/build/blastit/820647768b49cfb5/out/critical_check_engine-820647768b49cfb5
  …/target/debug/build/blastit/98997e28a8a88429/out/smoke-98997e28a8a88429
  …/target/debug/build/blastit/a0eb3b2a97f4e4c1/out/blastit-a0eb3b2a97f4e4c1
  …/target/debug/build/blastit/bb327fcd8e8a409f/out/critical_config_and_presets-bb327fcd8e8a409f
  …/target/debug/build/blastit/d801155d8279d096/out/critical_dev_and_fail_closed-d801155d8279d096
BUILD EXIT:0
```

The privileged loop, run with `sudo` stripped so the same script can be exercised as the runner user
(proves the loop, the parse, `PIPESTATUS`, `timeout`, `--nocapture` streaming and `tee`):

```console
$ bash /tmp/opencode/steps/step8-as-runner.sh
running 6 test binaries as uid 0, one at a time
==> root: …/out/blastit-40841d6f9f4794cc        running 179 tests   ok. 178 passed; 1 ignored      0.89s
==> root: …/out/critical_check_engine-…          running 16 tests   ok. 16 passed                 0.06s
==> root: …/out/smoke-98997e28a8a88429           running 31 tests   ok. 31 passed (all skipped: needs root)  0.01s
==> root: …/out/blastit-a0eb3b2a97f4e4c1         running 0 tests    ok                            0.00s
==> root: …/out/critical_config_and_presets-…    running 11 tests   ok. 11 passed                 0.06s
==> root: …/out/critical_dev_and_fail_closed-…   running 19 tests   ok. 19 passed                 2.64s
STEP8 EXIT:0 WALL:4s
```

And the timeout branch, with a stand-in "blocked test binary" and the budget shortened to 5 s (the
only difference from the committed step is `binary_timeout=90` → `binary_timeout=5`):

```console
$ bash /tmp/opencode/steps/step8-demo.sh
running 2 test binaries as uid 0, one at a time
==> root: …/out/blastit-40841d6f9f4794cc
running 179 tests
… (every test name streams live: this is the in-flight log that did not exist)
test result: ok. 178 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.93s
==> root: /tmp/opencode/fake/fake-blocked-test
test a_test_that_blocks ... 
::error::/tmp/opencode/fake/fake-blocked-test did not finish as root within 5s: the last 'test <name> ...' line in root-tests.log is the test that blocked (MA.9 candidate 3), not a deadlock in the job.
STEP8-DEMO EXIT:1 WALL:6s
```

### 2. Root cause named with evidence

See "Root cause" above: candidate 1 **refuted** (the same job's `sudo chmod` succeeds; `--non-interactive`
now settles it in milliseconds), candidate 2 **refuted by measurement** (root compiles 0 units,
`Finished in 0.10s`, and a flip is one crate that announces itself; `LD_LIBRARY_PATH` is not in the
fingerprint) and **removed by construction** (root never runs cargo for the test binaries), leaving
candidate 3 — a root-phase block — narrowed to the two unbounded waits in `tests/smoke.rs`
(`waitpid(pid, …, 0)` at :2109, `child.wait()` at :408), with the 31-test/5-second comparison showing
it is a block and not suite growth. It cannot be settled locally (the privileged suite cannot run
from a `0700` home — evidence above), so the job now prints the answer. Nothing is *inferred* as the
cause here: 1 and 2 are refuted with measurements, and 3 is stated as open with its evidence and the
one command that closes it.

### 3. `cargo test -p blastit` still runs as root

The six binaries are exactly the targets `cargo test -p blastit` builds (lib unit, bin unit, three
`tests/critical_*.rs`, `tests/smoke.rs`), and each is launched as uid 0 through the same
`sudo --non-interactive env …` handoff. Proof on this host, with the committed step's incantation and
the lib unit binary:

```console
$ sudo -n env "PATH=$HOME/.cargo/bin:$PATH" "PYO3_PYTHON=$PYO3_PYTHON" sh -c 'id -u; id -un'
0
root
$ bash /tmp/opencode/steps/step8.sh          # step 8 verbatim, list = the lib unit binary
running 1 test binaries as uid 0, one at a time
==> root: …/out/blastit-40841d6f9f4794cc
running 179 tests
test result: ok. 178 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.93s
EXIT:0
```

No `continue-on-error`, no `|| true`, no skip, no dropped `sudo`: a failing or timed-out binary exits
the step non-zero. The doc-test step keeps the crate's `--doc` tests under root as before (it reports
`running 0 tests` for `blastit` today, measured locally: `cargo test -p blastit --doc --locked -- --
--test-threads=1` → `test result: ok. 0 passed`).

### 4. `make check` and `plan.py validate`

```console
$ CARGO_NET_OFFLINE=true make check > /tmp/opencode/make-check.log 2>&1; echo EXIT:$?
EXIT:2
```

`make` runs the prerequisites in Makefile order and stops at the first failure, so an exit at the
**last** one means every earlier target returned 0:

```console
uv run --no-project tools/guard.py                        guard: OK (2 staged/untracked file(s) checked)
uv run --no-project tools/critical_guard.py --base HEAD   critical-guard: HEAD..working tree, 2 changed file(s)
cargo fmt --all -- --check                                0
cargo clippy --workspace --all-targets --locked -D warnings  0
cargo doc --workspace --no-deps --locked                   0
cargo test --workspace --all-targets --locked              all "test result: ok"
cargo test --workspace --doc --locked                      0
cargo test -p <crate> --test critical_*                    10 critical suites, ok
python -m unittest discover -s python/tests/critical       Ran 50 tests in 3.214s / OK
uvx --offline ruff check python tools                      All checks passed!
uvx --offline ruff format --check python tools             61 files already formatted
… test_runnable_docs.py test_harness.py                    ok
uvx --offline zensical build --strict                      No issues found
docs harness                                               38 fences: 26 executed, 12 accounted, 0 failed · 17.5s
uv run --no-project tools/plan.py validate                 MA: done but has open children
make: *** [Makefile:54: plan-validate] Error 1
```

**The last target that fails is `plan-validate`, and it is not mine.** Non-mutating proof: the tree at
`HEAD` validates, and `plan/` is untouched by this node (only the ledger lines the conductor/lead
wrote at 11:52, which are in `plan/JOURNAL.md` at `HEAD`'s parent commit already):

```console
$ rm -rf /tmp/opencode/validate-head && mkdir -p /tmp/opencode/validate-head
$ git archive HEAD | tar -x -C /tmp/opencode/validate-head
$ (cd /tmp/opencode/validate-head && uv run --no-project tools/plan.py validate); echo EXIT:$?
plan OK
EXIT:0
$ git diff --name-only           # tracked changes in my tree
.github/workflows/ci.yml
plan/JOURNAL.md                  # ledger lines written by conductor/lead at 11:52, not by this node
$ git status --short
 M .github/workflows/ci.yml
 M plan/JOURNAL.md
?? plan/nodes/MA/MA.9/
```

`MA` is `status: done` while its child `MA.9` is open — the same tree-wide
inconsistency MA.7 and MA.8 reported (`tools/plan.py:445`). I did not touch `plan/`.

### 5. What the next CI run must show for this job to be green

The `smoke (root)` job, in order, inside 20 minutes:

1. **Preflight** — `sudo: Sudo version …`, `sudo: root uid 0, resolved non-interactively`,
   `runner cargo: … (cargo 1.x.y)`, `homes: RUSTUP_HOME=… CARGO_HOME=…`,
   `postgres: BLASTIT_TEST_PG_URL=set`, then `root: uid=0 home=/root` and root's
   `cargo --version`. If the runner's sudo is sudo-rs, that first line says `sudo-rs` and the run
   proves "no prompt" by *not* stopping there.
2. **Build** — cargo's `Finished … in Ns` as the runner user, then `6 test binaries, built as the
   runner user:` and six absolute paths. No `Compiling` line after this point in the job: if one
   appears under the next step's heading, candidate 2 is back.
3. **Run every test binary as root** — six `==> root: …` headings, each followed by
   `running N tests` and `test result: ok. …` (the lib binary's 179 tests stream one line each),
   with no test taking more than a few seconds. The smoke binary is the one to read: 31 tests, all
   `ok`.
4. **Doc tests** — `Doc-tests blastit` / `running 0 tests` / `test result: ok`.
5. Job conclusion `success`, total wall time well under 20 minutes.

If instead it goes red, the log now says which of the three it was, with no inference left:

* `::error::sudo --non-interactive id -u failed …` → candidate 1 is real on this image (it will be
  the *first* thing in the log, seconds in).
* a `Compiling`/`Blocking waiting for file lock` line under a step heading → candidate 2.
* `::error::<binary> did not finish as root within 90s` plus the last `test <name> ...` line →
  candidate 3, naming the exact test; `root-tests.log` is uploaded as the `smoke-root-logs` artifact.

## Files

- `.github/workflows/ci.yml` — the `smoke (root)` job: `timeout-minutes: 20` on the job, the bounded
  `--non-interactive` chmod step, the preflight step, the runner-user `--no-run` build step, the
  streaming privileged step, the doc-test step, the failure artifact upload (+158/−13, two hunks).
- `plan/nodes/MA/MA.9/RESULT.md` — this report.
- `plan/BUDGET.md` — **not mine**: `+1/−1`, only its `updated:` timestamp, which `tools/plan.py`
  writes; no other plan file changed (no status, no `bumped`).
- `plan/JOURNAL.md` — likewise not mine: the four `[MA.9]` ledger lines the conductor and lead wrote at
  11:52, before this attempt started.

Nothing else was modified: no test, no Makefile, no Cargo file, no plan status (all `plan/` commands I
ran were read-only: `plan.py validate`). `git status --short` is `.github/workflows/ci.yml`,
`plan/BUDGET.md`, `plan/JOURNAL.md` and the untracked `plan/nodes/MA/MA.9/`. No `git add`, commit,
push, tag or `gh`.

## Follow-ups

1. **Conductor, must land before `make check` can be green (pre-existing, not this node):** `MA` is
   `done` with the open child `MA.9`, so `plan-validate` fails tree-wide
   (`tools/plan.py:445`). One of: reopen `MA` for this leaf, or reparent it. Proven not-mine
   above with `git archive HEAD` → `plan OK`.
2. **Conductor/lead:** the next push is the only place the job can be proved. Expected log contents
   are listed in acceptance item 5; if it is red, the `smoke-root-logs` artifact carries the whole
   root-phase log plus cargo's JSON.
3. **`tests/smoke.rs` (out of this node's `paths`, worth its own node regardless of the CI outcome):**
   two waits in the privileged suite have no deadline — `connect_as`'s
   `libc::waitpid(pid, &mut status, 0)` (`:2109`) and `Server::drop`'s `child.wait()` (`:408`). Every
   other wait in the file is bounded by `READY_TIMEOUT`/`EXIT_TIMEOUT`. A blocked test can therefore
   outlive all of them; a `waitpid(…, WNOHANG)` poll against `EXIT_TIMEOUT`, and a `try_wait` loop in
   `Drop`, would make the suite self-limiting even outside CI.
4. **`docs/knowledge/LESSONS.md` (outside this node's `paths`, so recorded here):** three facts worth
   one line each —
   (a) `cargo test --no-run --message-format=json` is **JSON Lines**, not a single array
   (`json.load` fails with "Extra data");
   (b) cargo 1.100 places test executables in
   `target/debug/build/<pkg>/<unit-hash>/out/<name>-<hash>`, not `target/debug/deps/`, so take the
   path from cargo's own `executable` field rather than composing one;
   (c) a root `cargo` run leaves root-owned files in `target/` (5,572 here, and a runner-user `chown`
   is refused for every one) that then block the runner user's next build — and blastit's privileged
   suite cannot run at all from a `0700` `$HOME` checkout ("… (mode 0700) is not traversable by uid
   21000"), so the root smoke path is CI-only on a developer machine.
5. **The same bare-sudo step in `pytest-plugin`** (`.github/workflows/ci.yml`, the `chmod go-w /opt`
   copy at ~line 224) still uses plain `sudo` and has no step bound. It is not in this review's scope
   and the job has no privileged test, so it cannot be the MA.9 hang — but the same
   `--non-interactive` + `timeout-minutes: 1` treatment would make it consistent with `smoke (root)`.

## Review fixes (after `VERDICT: PASS` with five MINOR findings)

Four findings applied, nothing else restructured. The fifth (the `pytest-plugin` copy of the
`chmod /opt` step, out of this review's scope) is reported as follow-up 5 rather than changed here.

1. **The `chmod /opt` step can no longer block before the preflight** (`ci.yml`, `smoke (root)`): it
   was `sudo chmod go-w /opt && sudo chmod -R go-w /opt/hostedtoolcache`; it is now
   `sudo --non-interactive chmod go-w /opt` and `sudo --non-interactive chmod -R go-w /opt/hostedtoolcache`
   under `set -euo pipefail` (identical semantics), with `timeout-minutes: 1`, so a prompting sudo
   fails in milliseconds with its own message instead of holding the job. Proven with both paths, the
   paths redirected to a scratch tree so no real `/opt` was touched:

   ```console
   $ bash /tmp/opencode/steps/step3.sh      # committed text, /opt → /tmp/opencode/chmodprobe
   EXIT:0 (mode before: 777 777 → after: 755 755)      # `go-w` and nothing else
   $ PATH=/tmp/opencode/shim:$PATH bash /tmp/opencode/steps/step3.sh   # password-required sudo
   sudo: a password is required
   EXIT:1 (mode still 777: nothing changed, no hang, no prompt)
   ```

2. **The build step is bounded**: `timeout-minutes: 10` on `Build the test binaries (runner user, no
   root)` — the acknowledged long phase (a cold `rust-cache`) can no longer eat the privileged step's
   budget silently; it is a red step with its own log at 10 minutes.

3. **The per-binary budget now adds up inside the step**: `timeout 240s` → `binary_timeout=90`
   (`timeout --kill-after=30s "${binary_timeout}s"`, with the `::error::` interpolating the same
   variable), and the comment states the arithmetic — 90 s × the crate's six test binaries = 9 minutes,
   inside the step's `timeout-minutes: 10`, so every binary reaches its own timeout and its own message
   instead of being cut off by the step (the healthy suite is ~5 s, so this is ~18× headroom; a seventh
   target would have to raise the step bound with it). 120 s was rejected for exactly this reason:
   6 × 120 s = 12 min would still have outrun the 10-minute step. Branch re-proven with the committed
   script and `binary_timeout=5`:

   ```console
   running 2 test binaries as uid 0, one at a time
   ==> root: …/out/blastit-40841d6f9f4794cc
   running 179 tests
   test result: ok. 178 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.90s
   ==> root: /tmp/opencode/fake/fake-blocked-test
   test a_test_that_blocks ... 
   ::error::/tmp/opencode/fake/fake-blocked-test did not finish as root within 5s: the last 'test <name> ...' line in root-tests.log is the test that blocked (MA.9 candidate 3), not a deadlock in the job.
   STEP8-DEMO EXIT:1 WALL:6s
   ```

4. **Accuracy in this report**: "root never invokes cargo, so root cannot compile anything" now reads
   "root never **builds** the test binaries", with the doc-test step named as the one remaining root
   `cargo` call (`--doc`; rustdoc compiles snippets, not the test targets, and it has its own 3-minute
   bound). The diff is **two hunks**, not one. `Files` now lists `plan/BUDGET.md` (only `plan.py`'s
   `updated:` line) and the root-owned-path count is the measured **5,572**, with the note that a
   runner-user `chown` is refused for every one of them.

Re-verified after the fixes: YAML parses (`jobs: 10`, `smoke` `timeout-minutes: 20`, step bounds
1/2/10/10/3) and every re-extracted `run:` script passes `bash -n`; the preflight happy path exits 0
and prints root's uid and both `cargo --version`s, the password-required shim exits 1 with the
candidate-1 `::error::`; `git status --short` is `.github/workflows/ci.yml`, `plan/BUDGET.md`,
`plan/JOURNAL.md`, `?? plan/nodes/MA/MA.9/` — the tracked diff is still `.github/workflows/ci.yml`
alone (plus the two plan ledger files that are not mine). Nothing staged, committed or pushed.
