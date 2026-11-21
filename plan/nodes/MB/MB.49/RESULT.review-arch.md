# MB.49 — architecture / security / test review

Base `ca3d5be`. Read in full: `crates/blastit/src/cli/handoff.rs` (whole file),
`crates/blastit-py-shim/src/{buffer,lib}.rs` + `Cargo.toml`,
`crates/blastit/tests/critical_handoff.rs`, `host.rs`'s `protected()` /
`Provenance`, `scripts/{repack_universal_wheel,check_workflows,build-wheel,
wheel-smoke,standalone-runtime-check}.sh`,
`python/tests/runnable_docs/harness.py`, `e2e/venv-smoke.sh` + `venv-smoke/run.sh`,
`e2e/php/{Dockerfile,README.md}`, `.github/workflows/{ci,publish-pypi}.yml`.

Commands I ran myself:

```
cargo clippy -p blastit-py-shim --all-targets --locked -- -D warnings
  Finished `dev` profile … in 2.00s                      # 0 warnings
cargo clippy -p blastit-py-shim --all-targets --locked --features extension-module -- -D warnings
  Finished `dev` profile … in 1.98s                      # 0 warnings
make workflow-check
  workflow-check: ok — 713 checks over 9 workflows and 1 actions, 0 problem(s)
UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
  packaging-gates-test: ok — 130 cases passed (… repack=15 …)
uv 0.13.9 probe on a `uv python install`ed 3.14 (EXTERNALLY-MANAGED marker present):
  --system              -> error: The interpreter … is externally managed
  --break-system-packages -> error: No solution found … (cache/offline), i.e. marker lifted
```

## Verdict list

1. **`[BLOCKER]` `e2e/venv-smoke.sh:66` — the work directory cannot be created.**
   Line 64 makes `/var/lib/blastit-venv-smoke` `root:root 0755`, then line 66
   `mkdir -p "$work"` runs **as the container's unprivileged user** (uid 1000 —
   that is exactly the uid the node blames for the venv's `.so`), which cannot
   create `run-$$` in a root-owned `0755` directory: `EACCES`. Under
   `bash -euo pipefail` the `wheel in a uv virtualenv` job now dies *before*
   `run.sh` ever runs, so MB.49 replaces the original failure with an earlier one.
   `mkdir -p "$work"` at `venv-smoke/run.sh:25` fails the same way, for the same
   reason. Fix (keep the parent root-owned so the ancestor chain stays
   non-world-writable, hand the *work* dir to the caller):

   ```sh
   sudo --non-interactive install -d -o "$(id -u)" -g "$(id -g)" -m 0755 "$parent"
   work=$(sudo --non-interactive mktemp -d "$parent/run.XXXXXX")
   sudo --non-interactive chown "$(id -u):$(id -g)" "$work"
   ```

   `install -d`/`mktemp -d` also fixes finding 3 (`$$` reuse).

2. **`[BLOCKER]` no venv ownership fix — the `owned by uid 1000` refusal the node
   set out to kill is still there.** `NODE.md` item 5 asks for
   `sudo chown -R root:root "$work/.venv"`; the diff contains **no** `chown` of
   the venv anywhere (`git diff ca3d5be -- e2e/venv-smoke/run.sh` is empty, and
   `venv-smoke.sh` chowns only `$parent`). `host::protected()` requires
   `facts.owner == 0 || facts.owner == init_uid`, and with `sudo .venv/bin/blastit`
   `init_uid == 0`; the wheel's `blastit/_bin/libblastit_py3NN-*.so` and
   `the project virtualenv` are both `Claim`s (`stage0/project_python.rs:257-266`),
   and both are installed by uid 1000 in `run.sh:43`. Moving the tree out of
   `/tmp` only removes the *third* part of that message
   (`a directory above it is … other-writable`); the owner part survives, so the
   boot still refuses. Note the fix must be **surgical**: a blanket
   `chown -R root:root "$work/.venv"` breaks the rest of `run.sh`, which mutates
   the installed metadata as uid 1000 (`chmod 0700/0755 "$dbproxy"`,
   `rm -f "$dbproxy"`, `printf > "$dbproxy"`, `mv …` → `EPERM`). Chown the claims
   (`site-packages/blastit/_bin/*` and the venv root) and route those four
   mutations through `sudo`, or run every venv-touching step as root and give the
   uid-1000 shells group-free reads only.

3. **`[MAJOR]` `e2e/venv-smoke.sh:65-67` — `run-$$` is not a unique work directory,
   and the trap is `EXIT`-only.** `mktemp -d` was unique; `run-$$` is the shell's
   PID, which a *stale* run (SIGKILL, cancelled job, no trap) leaves behind, so
   the next run's `mkdir -p` silently reuses a tree full of root-owned leftovers
   instead of a fresh one — the "a fresh work dir" invariant this test rests on.
   `trap … EXIT` also does not fire for SIGTERM/SIGINT (bash needs
   `trap … EXIT TERM INT` or an explicit handler), so cleanup is best-effort
   only. Use `mktemp -d "$parent/run.XXXXXX"` (see 1) and widen the trap.

4. **`[MINOR]` `scripts/repack_universal_wheel.py:106-135` — a section header with
   anything after `]` is silently lost.** `preserved_entry_point_sections` only
   recognises `stripped.startswith("[") and stripped.endswith("]")`; probed:

   ```
   "[pytest11]  # plugins\nblastit = x\n"   -> []
   "[pytest11]\na = b\n[pytest11]\nc = d\n" -> two [pytest11] blocks
   "[pytest11]\r\nblastit = …\r\n"          -> ok (CRLF normalised to LF)
   ""                                       -> [] (console scripts only — correct)
   ```

   So a trailing annotation on the header drops the whole `[pytest11]` section —
   the exact user-visible regression this node fixes, silently back. Maturin writes
   clean headers today, so this is defence, not a live bug. Parse the first `]`
   (`stripped.startswith("[")` then `split("]", 1)`) and de-duplicate by name
   (first wins, or last wins — but exactly one section per name). Everything else
   about the preservation is right: `[console_scripts]` from maturin is dropped
   rather than doubled, ordering is console-scripts-first then maturin's order,
   a missing `entry_points.txt` degrades to console scripts only, indentation is
   normalised away harmlessly, and `RECORD` is recomputed *after* the rewrite
   (`repack()` line ~318), so the hashes stay consistent — `repack=15` cases green.

5. **`[MINOR]` `scripts/check_workflows.py:1130-1201` — rule 23 is a heuristic, not a
   structural assertion.** It sits on top of the (structural) YAML `run:` extraction,
   then does a regex (`SH_C_WORD`) plus a line walk. It is self-tested, fails closed
   when its own self-test breaks, and does not fire on the current 9 workflows — but
   probed false negatives for the *same defect class*:

   ```
   "sh -eu -c '\n  # uv's musl\n …"            -> [3]   (the real bug)  ✔
   "sh -eu -c '\n  echo \"it's broken\"\n …"   -> []    (non-comment apostrophe)
   "sh -eu -c \"\n  # uv's musl\n …\""         -> []    (double-quoted word)
   "sh -eu -c \\\n  '\n  # uv's musl\n …"      -> []    (quote on the next line)
   ```

   The `elif "'" in line: break` treats *any* apostrophe on a non-comment line as the
   closing quote, and the walk only ever reports comment lines. That is honest
   (the docstring says so) and it is workflow tooling, not one of the forbidden
   `scripts/check_*.py` product-source guards — so I do not block on it — but the
   `RESULT.md` claim that it "reproduces the exact `sh -c '…'` defect" should be read
   as "the *comment-line* shape". Cheapest real improvement: quote the `-c` word with
   a parser (`shlex` over the extracted `run:` payload, or reuse the same word-walk
   to flag a non-comment line whose apostrophe does not leave the word balanced) and
   only treat a *lone* `'` line (`line.strip() == "'"`) as the close.

6. **`[MINOR]` `scripts/build-wheel.sh:351-364` — the 3.13 fallback is silent.**
   The guard is right (`uv python find '>=3.13' 2>/dev/null || true`, then
   `py_minor="3.13"`), and 3.13 is the manylinux container's own
   `/opt/python/cp313-cp313`, so the fallback cannot pick a *nonexistent* minor.
   But when `PYO3_PYTHON`/`--interpreter` named 3.14 and `command -v "$probe_py"`
   failed, the caller now silently gets a 3.13 wheel. Add one line —
   `echo "build-wheel: no host >=3.13 (…); assuming 3.13 for the manylinux probe" >&2`
   — and consider constraining the probe to `'3.13'`/`'3.14'`: `>=3.13` can return a
   `uv`-installed 3.16a, whose `/opt/python/cp316-…` the image does not have (loud,
   but a needless red).

7. **`[MINOR]` `e2e/php/README.md:25-36` now contradicts the Dockerfile.**
   It still says "the package by version, so the run is reproducible" and names
   `8.4.27` as "Debian's current package". After the unpin, the package floats with
   `trixie`; `e2e/` is in this node's `paths`, so the sentence should be reworded in
   the same pass (digest-pinned base image, package floating with the suite).
   The Dockerfile change itself is right: the digest pin on the base image is the
   pin a Dockerfile can honour, and keeping a `=8.4.27-1~deb13u1` point pin against
   a rolling `trixie-security` is a build that breaks on Debian's schedule.

8. **`[MINOR]` `crates/blastit/src/cli/handoff.rs:246-256` + `:335-341` — the
   self-image identity is compared twice.** `verify()` compares the opened leaf's
   `(dev, ino)` against `/proc/self/exe`, then `attempt()` calls `is_this_image()`,
   which stats `/proc/self/exe` again. If that second stat fails transiently
   (`/proc` unavailable, hidden by a sandbox), `attempt` falls through to
   `exec_verified()` on the image that is already running — one pointless exec
   (not a loop: the re-exec'd image reaches the same early return). No security
   hole — nothing is exec'd that was not already running — but the pair `verify()`
   just compared should be carried on `Verified` (e.g. `is_self: bool`) instead of
   re-derived. Note this is *not* the TOCTOU risk the question asks about: `meta`
   is `fstat` of the `O_NOFOLLOW`-opened leaf, so the identity is the opened file,
   and `/proc/self/exe` is fixed for the process's lifetime; there is no window to
   steer.

9. **`[MINOR]` cohesion debt, worsened.** `scripts/check_workflows.py` is now
   **2122 LOC** (+103) and `python/tests/runnable_docs/harness.py` **3942 LOC**
   (+26); the standard is `<500` prod LOC / one owner per rule. Neither is this
   node's fault, but every CI failure adds another rule to a 4×-over file. Split
   the rule set (`scripts/workflow_rules/*.py`, one self-test each) the next time
   one lands, rather than at the point where it is 3000 lines.

10. **`[MINOR]` `ci.yml` `docs-gate` comment is imprecise about *which* minor the
    harness picks.** `interpreter_for()` → `_wheel_python()` returns `HOST_PYTHON`
    (`sys.version_info` of the harness process) for the `py3-none` wheel, i.e.
    whichever minor `uv run` gives `make docs-test` — never "either, per fence".
    Warming both 3.13 and 3.14 is still the right belt (and the 3.14 base path is
    resolved in the *ambient* env and passed absolutely, so it survives the
    throwaway `HOME`), but the comment should say so; otherwise the next reader
    assumes a per-fence choice that does not exist.

## What is fine (checked, not assumed)

- **`handoff.rs` ordering genuinely holds.** `verify()` (`:246`) opens the parent
  `O_NOFOLLOW`, opens the leaf `O_NOFOLLOW`, `fstat`s *that descriptor*
  (`:243`), and answers `is_running_image(meta.dev(), meta.ino())` **before**
  `is_file`, `0o111`, owner, `0o022` and `check_tree`. `attempt()` then returns
  `None` for it (`:135-141`) — no refusal, no `exec_verified`, no
  `BLASTIT_HANDOFF_DONE` set. Every other candidate still meets every check
  unchanged; the only behavioural delta is that an *implicit* candidate which is
  this image now continues silently instead of printing "skipping local binary"
  (the refusals' text and the `explicit → exit 1` policy are untouched, so the
  implicit path still skips-and-continues rather than bailing). The new critical
  test is not vacuous: `run_with` does `.env_clear()` (`critical_handoff.rs:129`),
  so an inherited `BLASTIT_NO_LOCAL_HANDOFF` cannot green it, and the `EXDEV`
  copy fallback keeps the inode identity it asserts on. `standalone-runtime-check.sh`
  unsets `BLASTIT_BIN` on both `dev` launches (`:104`, `:207`) — the only two.
- **`buffer.rs`: the unit test is the right call, and the gating alternative would
  have been wrong.** `lib.rs` compiles `mod surface` (and `boot/env/serve/syntax`)
  under `#[cfg(not(all(test, feature = "extension-module")))]`; the pair is read in
  the *non-test, feature-on* build too, so `#[cfg(feature = "extension-module")]`
  would break the default build. A `#[cfg(not(all(test, feature = "extension-module")))]`
  mirror would compile, but then the frozen `70`/`1` values would have no witness
  in the one configuration that clippy complains about — asserting them is strictly
  better than hiding them. No target now compiles differently between the two
  clippy invocations, and both runs are green here.
- **`harness.py` `--break-system-packages`.** The claim is true and I reproduced it
  on uv 0.13.9 (see the probe above): `--system` only changes the hint
  ("Virtual environments were not considered…") and still refuses; the marker is
  lifted by `--break-system-packages`. The probe is still `--dry-run`, and
  `base_env()` still pins `UV_OFFLINE=1`, `UV_PYTHON_DOWNLOADS=never`,
  `UV_LINK_MODE=copy`, so it cannot fetch and cannot write. `install_pytest` keeps
  `managed=False` and targets the gate's own venv — correct. One hardening I would
  like: if `--dry-run` is ever dropped from `_require_cached_pytest`, that flag
  lets the probe mutate the runner's base CPython; assert `"--dry-run" in argv` in
  `_pytest_argv` (or name the helper `_cache_probe_argv` and append it there).
- **`ci.yml`.** Both symlinks come after `uv python install` and before the step
  that needs them (`make python-runtime`, and the WSGI tests' `python3.1N`
  discovery); an empty `$(uv python find …)` turns into `ln -sfn '' /usr/local/bin/python3.1N`,
  which fails the step (fails closed — though `-f` unlinks the old target first, so
  a transient failure leaves no symlink; the message is `ln: failed to access ''`);
  `${UV_PYTHON_INSTALL_DIR:-$HOME/.local}` covers both the set and default uv
  layouts and the `chmod -R go-w` runs after the install, as it must; the
  `docs-gate` cache key `uv-docs-gate-${runner.os}-py313-py314-pytest-9.2.0`
  matches the two venvs the warm step fills. `make workflow-check` is green.
- **`repack`'s console-script ownership** stays single-owner (maturin's
  `[console_scripts]` dropped, ours written first), which is what keeps the
  installed scripts equal to `CONSOLE_SCRIPTS`; `wheel-smoke.sh`'s new check reads
  the *installed* metadata (`importlib.metadata`), which is what pytest reads —
  the right place, not the zip.

VERDICT: fix

## Round 2

Base `ca3d5be`. Re-read: `e2e/venv-smoke.sh` (whole), `e2e/venv-smoke/{run.sh,check.py,wsgi.py}`
(whole), `scripts/{check_workflows,repack_universal_wheel,build-wheel,wheel-smoke,
standalone-runtime-check}.py/.sh` diffs, `crates/blastit/src/cli/handoff.rs` diff, and —
because the new E2b rests on them — `host.rs` (`binary`, `protected`, `facts_of`,
`Provenance`), `stage0/project_python.rs:257-268`, `stage0/fork_exec.rs:352-470`,
`check.rs:1146-1155`, `python/blastit/_blastit.py:120-200,530-575`.

Commands I ran myself:

```
bash -n e2e/venv-smoke/run.sh && bash -n e2e/venv-smoke.sh    -> ok
make workflow-check        -> 713 checks / 9 workflows / 0 problem(s)  (rule 23 self-test ok)
UV_OFFLINE=1 uv run --no-project scripts/test_packaging_gates.py
                           -> 131 cases (repack=16)                    (was 130/repack=15)
uv run --no-project tools/critical_guard.py --base ca3d5be… --node MB.49
                           -> OK: all 1 protected change(s) authorized by MB.49
probe of `shell_syntax_problems` (5 shapes, see 5.)
chown -R symlink probe on coreutils 9.7 (see 3.)
kill -0 probe on a `sudo` wrapper from uid 1000 (see 4.)
```

### 1. Is coverage lost in E2b? — *Not real coverage, but the denied half has no witness left*

The old leg was **false, not vacuous**: `chmod 0700 "$dbproxy"` then "boot must refuse"
could never pass, because on a wheel boot nothing `execve`s `blastit/_bin/blastit-dbproxy`.
I verified the mechanism end to end rather than taking the note's word:

- `_blastit.main` (`python/blastit/_blastit.py:558-566`) `execvpe`s `_bin/blastit`; that
  universal binary's `/proc/self/exe` **is** the server, so `Host::binary()`
  (`host.rs:146-155`) sets `dbproxy = Reexec::program(exe_path())`;
- `check.rs:1146-1155` prefers `host.dbproxy().program` too, so even `blastit check` runs
  `/proc/self/exe --role=dbproxy` — the file is off **both** exec paths.

So the rewrite is honest, and `[ -x ]` on `755 root:root` is a *true* statement the old
leg could not make. **The one thing that now regresses silently:** the gate the old leg
claimed to be the witness for — `fork_exec.rs:372-380`
(`visibility::unexecutable(&[placement.dbproxy], layout.dbproxy(), …)`) — has **no
boot-level witness anywhere in the tree**. `visibility.rs:1378-1413` tests the pure
function; `smoke.rs:2460-2502` tests the sibling `unreadable` refusal with a real boot;
`fork_dbproxy`'s *wiring* of `unexecutable` is witnessed by nothing. Delete that call and
every job stays green — including venv-smoke, whose `[ -x ]` measures a file the boot
never opens.

**Fix (≈10 lines, reuses the machinery this delta added).** Keep the stub branch and add
one denied launch on the *real* path, root-owned and mode `0700`:

```sh
printf '#!/bin/sh\nexit 0\n' | sudo tee "$work/not-executable" >/dev/null
sudo chmod 0700 "$work/not-executable"      # root-owned `$work` (0755) keeps D6 happy
extra_env=(BLASTIT_EXEC="$work/not-executable")
if run_server >"$work/unexecutable.log" 2>&1; then
    echo "  FAIL a dbproxy the child cannot execute booted"; status=1
else
    echo "  ok   a dbproxy the child cannot execute is refused"
fi
extra_env=()
checked 'not executable' "the message says what is wrong with it" "$work/unexecutable.log"
checked "$work/not-executable" "and names the file that blocks"     "$work/unexecutable.log"
checked 'chmod o+rx' "and the fix that opens it"                    "$work/unexecutable.log"
```

That is the same three sentences the deleted block grepped, on the file the child actually
`execve`s, refused by the gate that actually runs — i.e. E2b keeps its denied half instead
of trading it for an allowed half. (Checked: `plain_is_protected` never reads the exec
bits, so `0700` passes `placement` and the refusal comes from `fork_dbproxy`, which is
exactly the wiring at risk.)

### 2. Is the replacement at least as strong? — *Yes for the half it covers, and two of the five checks are better than what they replaced*

Judged one by one:

| check | verdict |
|---|---|
| `run.sh:92-94` console-script shebang `#!$app/.venv/bin/python` | **stronger** than the dead `sys.executable.startswith(prefix)`: it asserts the file `sudo` runs, and the kernel honours it |
| `run.sh:174-175` `"environment":"$app/.venv"` | **real**, not a log cosmetic: that field is `PythonBoot::environment` = *the venv root* (`project_python.rs:74-80`), emitted as the `runtime` event (`gates.rs:535-548`). It proves stage 0 resolved `<manifest>/.venv` in §2.2 order — the old leg asserted nothing about discovery |
| `check.py:74-81` `library` realpath == venv site-packages | **stronger** than `sys.prefix`: it proves the *dependency*, from inside a view uid, and after `chown -R root:root` it now also proves the venv is world-readable (which the old uid-1000-owned tree never did) |
| marker import / `--cwd` / `path_first` | unchanged, still real |
| `run.sh:243-248` `[ -x ]` | sound: the file is root-owned, the prober is uid 1000, so `access(X_OK)` answers the **other** bit — and the whole path, so a `0750` ancestor also fails it |

Two caveats, neither fatal: `[ -x ]` is the *allowed* half only (see 1), and
`run.sh:31-35` still justifies `chmod 0755 "$work"` with "init `execve`s
`_bin/blastit-dbproxy` *after* `setuid(B+3)`" — the sentence 200 lines above `:242`,
which now proves the opposite. A reader cannot tell which is true; say "the venv's
`_bin/blastit` (the real `/proc/self/exe`) and the shims must stay reachable, and the
`_bin/blastit-dbproxy` copy is shipped for a deployment that names it in `BLASTIT_EXEC`".

### 3. `sudo chown -R root:root "$app"` — *nothing writes into `$app` as uid 1000; measured, not assumed*

Enumerated every mutation after `run.sh:74`: `cd "$app"` (traverse, 0755), `head -1`
(read), `"$app/.venv/bin/python" - <<PY` (`:99`) and `check.py` (`:161`) and `sysconfig`
(`:194`) — all **read**; `ls -l`/`stat` (`:110`, `:244`); stub creation is
`sudo tee` into `$work` + `sudo chmod` (`:260-261`, `:278-279`); every log redirect is
`>"$work/…"` (uid 1000's own dir); `sudo chmod 0750/0755 "$stdlib"` targets `/opt/python`,
never `$app`. The only uid-1000 *writes* that could land inside `$app` are CPython's
`__pycache__` attempts (by this shell and by the view uids, since `check.py` imports from
root-owned site-packages) — CPython swallows `OSError` from bytecode writing, so they are
no-ops, not failures.

Two things I expected to be wrong and are not:

- **`chown -R` does not reach outside the tree.** `.venv/bin/python*` are symlinks into
  `/opt/python`; coreutils 9.7 `chown -R` lchowns the link entries and does not follow
  them (probe: a tree with `linkdir -> root-owned outside` came back with `linkdir` changed
  and `outside` untouched). So a `PYTHON=` pointing at a uv-managed base interpreter would
  not be re-owned.
- **`$work` (uid 1000, 0755) violates no D6 claim.** `protected()` (`host.rs:389-410`) asks
  ownership of the *claim* (`$app/.venv`, `$app`, `python_home` — all root now) and only
  "group- or other-writable" of the ancestors (`facts_of`, `host.rs:322-344`). A uid-1000
  0755 ancestor is exactly what the comment at `run.sh:45-46` says it is. Correct as
  written — and it is the reason `/tmp` (`1777`) had to go.

### 4. `stop_server` / `extra_env` — *correct; one claim in the comment is untested, one line is dead*

- `child=$(sudo pgrep -P "$pid" | head -1 || true)` (`:145`) is right: `|| true` covers
  both `pgrep` exit 1 and a `head -1` SIGPIPE under `pipefail`, and `stop_server` /
  `cleanup` both `return 0`, so neither the EXIT trap nor a finished green run can turn
  into exit 1. Confirmed by the recorded run (26 ok, `exit=0`).
- No orphan on 8080: `$server` is the `sudo` wrapper, the child is TERMed first, then both
  are KILLed, then `wait`. The proof is in the evidence itself — the next scenario binds
  8080 and answers instead of `Address already in use`.
- `kill -0 "$pid"` (`:147`) **works**: I checked the concern that uid 1000 cannot probe a
  root process — `sudo`'s *real* uid stays the invoking uid (measured:
  `RUID 1000 EUID 0 sudo`), and signal permission compares real/effective against
  real/saved, so the bounded wait is a real wait, not dead code.
- `extra_env`: initialised at `:131` immediately before the first call (so `set -u` is
  satisfied even without the `${arr[@]+…}` guard, which is the bash<4.4-safe form anyway),
  set once at `:262`, and **deliberately still live** for the hung launch at `:281` — the
  hung leg rewrites the same `$stub` and must keep `BLASTIT_EXEC`. So "reset before the
  last launch" would be a **bug**, not a fix. Two nits: the comment at `:130` says "for
  one launch" while it spans two, and `extra_env=()` at `:298` is dead code (it sits after
  the last launch, before `exit`) — harmless today, but a fifth scenario added below it
  would silently inherit the stub.
- Also correct, and worth keeping: the bare `blastit` is `Serve`, and `handoff::name()`
  only answers for `Dev|Check|Fmt|Lsp`, so venv-smoke does **not** need
  `BLASTIT_NO_LOCAL_HANDOFF=1` — unlike `standalone-runtime-check.sh:104,208`, where the
  delta rightly added it. The two are not inconsistent.
- Not tracked: the died/hung launches are foreground, so `cleanup` still only knows the
  first `$server`. Fine today (the hung leg is last, `--rm` reaps anything left).

### 5. Rule 23 as `bash -n` after masking `${{ … }}` — *sound for the class it hunts; one demonstrable false positive*

Good design choices: the `run:` payload comes from the YAML (structural), `shell:` is
honoured by its first word, `bash -n` executes nothing, a missing `bash` fails closed
instead of reporting a pass it did not make, and the self-test asserts the defect *and*
three positives (so the rule cannot pass by refusing everything). Heredocs and `>-`
folding are the loader's job and are unaffected; masking a template to a bare word is
quote-neutral, so `run: echo "${{ matrix.x }}"` is fine.

I probed `shell_syntax_problems` directly:

```
A  "echo ${{ matrix.x\nsh -c '\n echo hi\n"                  -> REFUSED (unbalanced quote still seen)
B  "echo ${{ fromJSON('{\"a\":{\"b\":1}}') }}\n"             -> REFUSED   ← false positive
C  "echo ${{ inputs.msg }}\n"                                -> parses
E  "cat <<'EOF'\n$'\nEOF\n"                                  -> parses
G  shell: C:\Program Files\Git\bin\bash.exe -e {0}           -> no verdict (silently skipped)
```

`[MINOR] scripts/check_workflows.py:1151` — `re.compile(r"\$\{\{.*?\}\}", re.DOTALL)` stops
at the **first** `}}`, so an expression whose own string literal ends in `}}`
(`fromJSON('{"a":{"b":1}}')`) masks only half of itself, leaves `') }}` behind, and the
block is refused with a message about "an apostrophe in a `#` comment" that has nothing to
do with the text. Loud, but it will one day go red on a legitimate workflow and send the
next author hunting a quote that is not there. Replace the regex with a ~15-line scanner:
on `${{`, walk forward treating `'…'`/`"…"` as opaque and closing on the `}}` that
leaves every inner `{` balanced; or at minimum, when `bash -n` refuses a block that
contains `${{`, re-run it with the expressions *deleted* and only report if it still
fails. Two blind spots the docstring should also admit (it names only non-bash `shell:`
and early-closed quote pairs): the *value* the runner substitutes can carry the quote that
breaks the line (C parses, cannot be judged statically — that is injection, not syntax),
and a `shell:` whose first token is not literally `bash`/`sh`/empty (a quoted Windows path,
G) is skipped with no verdict, i.e. unjudged, not passed; and `shell: ""` is judged as bash
although that is only right for Linux/macOS runners.

Nothing above is a reason to drop the rule — 713 checks green, no false positive on the
current 9 workflows, and it catches exactly the musl `sh -c '…uv's musl…'` shape.

### 6. Scope / other jobs

- Everything in the delta is inside MB.49's `paths` and maps to goals 1–9. The extra reach
  (`run.sh` +201, `check.py`/`wsgi.py`, `check_workflows.py` rule 23) buys a job that
  actually passes, and the two claims it rests on are real (checked in 1–3) — I would not
  refuse it as creep. `plan/{AUTOPILOT,BUDGET.md,JOURNAL.md}` are conductor/lead
  bookkeeping (`JOURNAL.md` attributes them), not the writer's.
- `critical_guard` ok; `critical_handoff.rs` (+149) is authorized by the node.
- Other jobs: `ci.yml`'s `ln -sfn "$(uv python find …)"` still fails closed on an empty
  result; `docs-gate`'s new key invalidates the old cache once (recorded);
  `build-wheel.sh:360-372` now names the 3.13 fallback; `repack`'s header parser is fixed
  properly (leading `[name]` token, duplicate names merged, `RECORD` recomputed after) with
  `repack=16` green; `wheel-smoke.sh:104-131` reads the **installed** metadata, which is
  what pytest reads. No job is broken by this delta.
- Follow-up, out of `paths` but created by this delta's reasoning: `host.rs:51-52` ("a
  wheel always `execve`s the `blastit-dbproxy` it ships") and `ARCHITECTURE.md:1802`
  ("`BLASTIT_EXEC` — the standalone install only") both contradict the code (`Host::binary`
  reads it unconditionally, for every install), and the new E2b *depends* on that. Decide
  which is true: either a wheel honours `BLASTIT_EXEC` (fix the two docs) or it does not
  (the new stub becomes a standalone-only leg, and the wheel should consider not shipping
  the unused binary at all — the writer already records this as follow-up 2).

### What is fine (checked, not assumed)

- `venv-smoke.sh:71-83` — root-owned `0755` parent, `sudo mktemp -d run.XXXXXX` (no
  `run-$$` collision), `chown` of the *work* dir only, `sudo rm -rf` in the trap, and
  `-- "$wheel" "$python" "$image"` so `$3` really is the image in the error message.
  `docker run --rm` also means a killed run leaves nothing behind for the next one.
- `handoff.rs` round-1 item 8 is done the right way: `verify()` computes the `/proc/self/exe`
  pair **once** from the opened descriptor and carries `is_self` on `Verified`; `attempt()`
  no longer re-stats, and every other candidate still meets every refusal unchanged. The
  new `euid == 0` wording on the ownership sentence is honest, not a weakening.
- `check.py`'s move from "which interpreter" to "which packages": `prefix`/`executable` are
  printed for the record with the reason they are not asserted, so the next reader does not
  re-add the impossible assertion.

VERDICT: fix
