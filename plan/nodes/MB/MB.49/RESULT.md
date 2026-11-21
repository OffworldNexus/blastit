# MB.49 — the nine PR #64 failures, fixed in one pass

STATUS: done

Every failure was root-caused to the line named in `NODE.md`; nothing outside the
node's `paths` was touched. The one new rule in `check_workflows.py` is a
**structural** shell parse (`bash -n` over each extracted `run:` block), not a
quote-counting heuristic, and its own limits are written down in the module
docstring.

## What changed

1. **`crates/blastit-py-shim/src/buffer.rs`** — `PANIC`/`REFUSED` are read by
   `lib.rs`'s `mod surface`, which is *absent* from a test build that enabled
   `extension-module`, so the pair was dead code in one of the two clippy runs.
   Gating them on the feature would break the default build, so they are
   **exercised**: `the_two_refusal_codes_are_the_frozen_ones` asserts `70`/`1`.
2. **`crates/blastit/src/cli/handoff.rs`** — `verify()` compares the opened
   leaf's `(st_dev, st_ino)` with `/proc/self/exe` **immediately after `fstat`**,
   before the regular-file / executable-bit / owner / mode / tree refusals, and
   carries the answer as `Verified::is_self`. `attempt()` reads that field
   instead of stat'ing `/proc/self/exe` a second time, so a transient `/proc`
   failure cannot `execve` the image that is already running. The root branch of
   the ownership refusal now reads "… and only root may own it here" instead of
   "not 0 or 0".
3. **`crates/blastit/tests/critical_handoff.rs`** — the explicit-`$BLASTIT_BIN`
   case now *serves* a real `dev` (static manifest, so the process outlives the
   handoff decision) and asserts `/proc/<pid>/exe` is still the file the test
   started. The output alone could not discriminate: a silent self-`execve`
   prints exactly what a refusal-free run prints. Measured under this node with
   a handoff to a **copy** of the binary: `/proc/<pid>/exe` then named the
   candidate, not the copy — the assertion fails if a handoff happens. The
   source half also pins **one** `exe_path()` call in the module.
4. **`scripts/standalone-runtime-check.sh`** — both `dev` launches pass
   `BLASTIT_NO_LOCAL_HANDOFF=1` *and* unset `BLASTIT_BIN`: `env -u` alone left
   the auto-discovered candidates live, so a run from a checkout would hand off
   to `.venv/bin/blastit` and the checks would be about a binary that was never
   started.
5. **`e2e/venv-smoke.sh` + `e2e/venv-smoke/run.sh`** — the work directory is
   `sudo mktemp -d "$parent/run.XXXXXX"` under a root-owned `0755` parent, chowned
   to the invoking user (every `>"$work/*.log"` is that user's), with an
   `EXIT`/`INT`/`TERM` trap that removes it with `sudo`; a no-`sudo` image exits
   with a sentence naming the contract. The probe is `sudo -n true`, so a sudo
   that would prompt for a password is refused rather than waited on, and the
   trap's `rm` is guarded with `|| echo … >&2` so a failed cleanup cannot turn a
   green run into `exit 1` with no output — the `INT`/`TERM` traps `exit` through
   the same `EXIT` trap and keep 130/143. The project and its venv (`$work/app`,
   `$work/app/.venv`) are then `chown -R root:root`'d — the three claims D6
   names — and every later mutation inside that tree (`sudo tee` for the stub,
   `sudo chmod`) goes through `sudo`.
6. **`e2e/venv-smoke/run.sh`, the E2b boot witness** — the wheel's
   `_bin/blastit-dbproxy` is on neither exec path, so the `-x` leg above only
   measures the packaged mode. The wiring that *is* live —
   `stage0::fork_exec::fork_dbproxy`'s `visibility::unexecutable` call — now has
   a denied launch of its own: a root-owned `0700` stub named by `BLASTIT_EXEC`
   (the documented seam, the `extra_env` mechanism already used for the dying
   and hanging stubs) makes the boot refuse, and the leg asserts the three
   sentences the old block grepped (`is not executable`, the file it names,
   `chmod o+rx`). Deleting the `unexecutable` call now turns this leg red.

7. **`scripts/repack_universal_wheel.py`** — `preserved_entry_point_sections`
   matches a header by its leading `[name]` token, so `[pytest11]  # plugin`
   keeps its section; a repeated name is merged into one block (`configparser`
   raises on a duplicate). `scripts/test_packaging_gates.py` ships that annotated
   header as the fixture and unit-tests the merge.
8. **`python/tests/runnable_docs/harness.py`** — the cache probe passes
   `--break-system-packages` (`--system` does not lift `EXTERNALLY-MANAGED`), and
   the comment now says "a distribution-packaged system CPython (Debian, Ubuntu)
   and every `uv python install` build" — Arch's and Alpine's are not managed.
9. **`scripts/build-wheel.sh`** — the 3.13 fallback prints why, on both paths
   (no host `>=3.13` to probe, and no `uv`/`PYO3_PYTHON` at all).
10. **`.github/workflows/*` + `scripts/check_workflows.py` rule 23** — the musl
   `sh -c '…'` comment is reworded (that apostrophe ended the word and ran
   `uv python install` / `arch=${TARGET%%-*}` on the runner with `TARGET`
   unbound), and rule 23 is now `bash -n` over every extracted `run:` block after
   masking `${{ … }}` — no quote counting. The mask is brace-balanced, so
   `fromJSON('{"a":{"b":1}}')` stays one expression instead of being cut at its
   first `}` and blamed on an apostrophe; the refusal message quotes `bash -n`
   rather than asserting the apostrophe shape. A self-test case pins the nested
   object. A step whose `shell:` is not bash/sh is skipped; a quote pair closed
   early and reopened balances and still parses, and the docstring says so.
11. **`e2e/php/README.md`** — the prose no longer claims `8.4.27` is pinned by
    version: the digest pins the base image, a Debian point version is superseded
    by `trixie-security` on Debian's schedule, so the package follows the suite.

## Evidence

```
$ cargo clippy -p blastit-py-shim --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.08s
$ cargo clippy -p blastit-py-shim --all-targets --locked --features extension-module -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.05s
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.25s
```

```
$ cargo test -p blastit --test critical_handoff --locked
running 16 tests
test an_explicit_blastit_bin_that_is_this_image_is_not_refused ... ok
...
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

The discriminating measurement (a handoff to a *copy* of the binary, which is not
the running image, so `handoff` really `exec`s it):

```
started: .../target/debug/blastit
readlink: /home/remy/handoff-proof.cPDf6y/.venv/bin/blastit     # the candidate
cmdline:  /proc/self/exe --stage=1 --config-fd=4 ...
```

`dev` re-execs itself with `/proc/self/exe` as `argv[0]`, so `cmdline` cannot be
the discriminator — which is why the case reads `/proc/<pid>/exe`.

```
$ make workflow-check
workflow-check: rule 23 self-test ok — every bash `run:` block is parsed with `bash -n` after masking `${{ … }}`
workflow-check: ok — 713 checks over 9 workflows and 1 actions, 0 problem(s)

$ make packaging-gates-test
packaging-gates-test: ok — 131 cases passed (..., repack=16, ...)

$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared; ...

$ make fast-check
test result: ok. 16 passed; ...        # critical_handoff, in `rs-test`
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

## `e2e/venv-smoke.sh`, end to end (modelw/base:2026.07, the local wheel)

```
$ WHEEL=dist/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl e2e/venv-smoke.sh
image:  modelw/base:2026.07
wheel:  .../dist/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
python: /opt/python/cpython-3.14-linux-x86_64-gnu/bin/python3.14
  ok   the console script runs on the venv's interpreter (shebang: #!/var/lib/blastit-venv-smoke/run.…/app/.venv/bin/python)
  ok   importing blastit starts no thread (1 thread)
  ok   the response is 200
  ok   a module only in the venv's site-packages was importable
  ok   the app imports from the venv's site-packages ('…/app/.venv/lib/python3.14/site-packages' == '…/app/.venv/lib/python3.14/site-packages')
  info sys.prefix='/opt/python/cpython-3.14-linux-x86_64-gnu' sys.executable='/usr/local/bin/python3' (the base install the venv was built from, by design §2.3; the venv's packages are on sys.path — the `ok` above reads the console script's shebang, and run.sh checks the boot's `environment`)
  ok   PATH_INFO reached the app ('/')
  ok   the manifest routed it ('root')
  ok   the working directory blastit was started in is first on sys.path ('…/app', expected '…/app')
  ok   the app's own working directory is the view's scratch directory, so the entry above is the server's, not the view's
  ok   the event stream carries boot
  ok   the event stream carries ready
  ok   boot is stage 0
  ok   ready is serving
  ok   the boot records the venv it serves from
  ok   a Python the view uids cannot read is refused
  ok   the message says what is wrong with it
  ok   and names the directory that blocks (/opt/python/cpython-3.14-linux-x86_64-gnu/lib/python3.14)
  ok   and the fix that moves the interpreter
  ok   the wheel's dbproxy is executable by uid 1000 (755 root:root)
  ok   a dbproxy another uid cannot execute is refused
  ok   the message says it is not executable
  ok   and names the file that blocks
  ok   and the fix that opens it
  ok   a dbproxy that dies before ready fails the boot
  ok   its own words reach the event stream
  ok   the boot says what happened
  ok   a dbproxy that never reports ready fails the boot
  ok   it gave up after 31s instead of waiting for the pipe
  ok   what it printed before hanging reaches the event stream
  ok   the boot says it timed out
exit=0
```

The new denied launch is a refusal, not a serve: the boot stops after
`runtime`, and the event stream carries

```
/var/lib/blastit-venv-smoke/run.…/dbproxy-unexecutable (mode 0700) is not executable by uid 20003 — … `chmod o+rx /var/lib/blastit-venv-smoke/run.…/dbproxy-unexecutable` here and on every directory above it
```

while the packaged `_bin/blastit-dbproxy` is `755 root:root` and the role child
is `/proc/self/exe --role=dbproxy`, exactly as the header now says.


The boot's own stream, for the record (`"privilege":"root"`, the venv named as
the environment, one `200` served from the venv's packages):

```
{"event":"boot","stage":"stage0","privilege":"root","bounding_set":"dropped"}
{"event":"runtime","entrypoint":"python","version":"3.14.10","environment":"…/app/.venv"}
{"event":"ready","stage":"serving","routes":1,"workers":16,"outputs":{"db":["DATABASE_URL"]}}
{"event":"request","method":"GET","path":"/","outcome":"ok","status":200}
```

Two harness bugs this run surfaced, both fixed and re-proven above: killing the
`sudo` wrapper left the server holding port 8080 (the next scenario answered
`Address already in use` instead of its own sentence), and `stop_server`'s
`pgrep -P` probe failed on an already-stopped server *inside the `EXIT` trap*,
which turned a green run into `exit 1`.

## Follow-ups

1. **`sys.prefix` is the *base* install, by design (§2.3).** The CPython shim
   initialises libpython with the venv's recorded `home` (`pyvenv.cfg`
   `home = …/bin` → parent), so a view reports
   `sys.prefix=/opt/python/cpython-3.14-linux-x86_64-gnu` and the venv's
   `site-packages` arrives on `sys.path` (`critical_python_shim`'s venv cases say
   the same). `e2e/venv-smoke/{wsgi.py,check.py}` now assert what that guarantees:
   the marker imports **from the venv's `site-packages`**, the console script's
   shebang names the venv's interpreter, and the boot's own event names the venv
   as its `environment`. `sys.prefix`/`sys.executable` are printed for the record.
2. **The `_bin/blastit-dbproxy` copy at `0700` still does not refuse — and the
   boot refusal is now witnessed through the seam that is live.** Measured: with
   `_bin/blastit-dbproxy` at `0700 root:root` the boot serves, because the child
   that runs the credentials role is this image
   (`/proc/self/exe --role=dbproxy`, uid 20003), not that copy. The `-x` leg
   therefore only asserts the packaged mode (the installed file is executable by
   uid 1000, `755 root:root`, and `0100755` is pinned by
   `test_packaging_gates.py`/`check_wheel_shims.py`); the denial itself is a
   `BLASTIT_EXEC` launch, where `fork_dbproxy`'s `unexecutable` call does run.
   Worth a decision later: either the wheel stops shipping an unused
   `blastit-dbproxy`, or the boot execs it, and then the `_bin` mode is the
   refusal.
3. **`crates/blastit/src/host.rs:51-52` and `ARCHITECTURE.md:1802` still say a
   wheel never reads `BLASTIT_EXEC`.** The restored E2b depends on the opposite:
   `Host::binary()` reads it unconditionally, for the wheel as well, and that is
   how a root-owned `0700` stub reaches `fork_dbproxy`. The `host.rs` doc comment
   and the architecture sentence are stale; both are outside this node's `paths`
   (`host.rs` is not listed), so they are recorded rather than edited.
4. **`uv pip install --system` does not clear `EXTERNALLY-MANAGED`.** Proved with
   uv 0.13.9: only `--break-system-packages` lifts it; `harness.py` uses it.
5. **`docs-gate`'s cache key changed shape** (`py313-py314-pytest-9.2.0`), so the
   first run after this lands warms the cache again.
6. **Rule 23's blind spots** (in its docstring): a non-bash `shell:` step is
   skipped, and a quote pair closed early and *reopened* balances, so `bash -n`
   accepts it although the host ran the middle lines. The unbalanced shape — the
   one that occurred — and every other bash syntax error are refused. The mask is
   brace-balanced now, but a literal `}}` inside a quoted string of a
   hand-written expression would still end it early (a matrix reference cannot
   contain one).
