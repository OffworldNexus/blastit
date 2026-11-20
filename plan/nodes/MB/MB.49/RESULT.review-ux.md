# MB.49 — review-ux (developer-facing behaviour, messages, terminal craft)

Reviewed `git diff ca3d5be` for `.github/`, `crates/blastit/src/cli/handoff.rs`,
`crates/blastit/tests/critical_handoff.rs`, `e2e/`, `python/`, `scripts/`
(13 files, +427/−42). No `docs/` page was touched, so the docs-visual-hierarchy
rules do not apply; only inline comments were audited for false or diary prose.

## Verdict list

1. **[BLOCKER] `e2e/venv-smoke.sh:66` — the new work directory cannot be created
   by the uid that runs it, so `wheel-smoke` fails harder than before.**
   Lines 61-64 make `/var/lib/blastit-venv-smoke` `root:root` `0755`, then line 66
   runs `mkdir -p "$work"` **as the container's unprivileged `user`** (the same
   comment block, lines 45-46, says the container keeps the image's own `user`
   and root is reached only through `sudo`). `0755 root:root` is not writable by
   that uid, so the step dies with
   `mkdir: cannot create directory '/var/lib/blastit-venv-smoke/run-NN': Permission denied`.
   That is exactly the class of failure this node was meant to delete, and the
   comment at 53-55 ("created here by the container `user`") is now false.
   Concrete replacement for lines 61-67:

   ```sh
   parent=/var/lib/blastit-venv-smoke
   sudo --non-interactive mkdir -p "$parent"
   sudo --non-interactive chmod 0755 "$parent"
   sudo --non-interactive chown root:root "$parent"
   work="$parent/run-$$"
   sudo --non-interactive install -d -m 0755 -o "$(id -u)" -g "$(id -g)" "$work"
   trap 'sudo --non-interactive rm -rf "$work"' EXIT
   ```

   and reword 53-55 to name the real creator: "a root-owned `0755` parent
   outside `/tmp`; the run directory inside it is created by root and handed to
   the container `user` (`MB.49`)."

2. **[BLOCKER] `e2e/venv-smoke.sh` / `e2e/venv-smoke/run.sh:161,174,189-192,230`
   — the *ownership* half of the reported refusal is not fixed, so the boot still
   refuses the wheel.** `NODE.md` item 5 prescribes
   `sudo chown -R root:root "$work/.venv"`; no `chown` of the venv exists in the
   diff or in `run.sh` (verified: `grep -rn chown e2e/` finds only the rootless
   image). Root's stage-0 gate is `blastit::host`'s `protected()`
   (`crates/blastit/src/host.rs:389-396`), reached from `cannot_be_replaced`
   (`host.rs:756-775`): owner must be `0` or `init_uid`. `uv venv` / `uv pip
   install` run as `user`, so `blastit/_bin/libblastit_py314-*.so` is owned by
   uid 1000 and the boot answers
   `… could be replaced by another uid: it is owned by uid 1000, and only root or
   init's uid (0) may own it`. Moving the tree out of `/tmp` fixes only the
   "directory above it is group- or other-writable" clause. Note the real
   tension the node did not resolve: `run.sh` mutates the venv *as the user*
   (`chmod 0700 "$dbproxy"`:161, `rm -f`/`printf >`:189-192, `mv`:230), so
   chowning the venv to root breaks those lines. Either run the whole fixture as
   root (`sudo --non-interactive bash /fixture/run.sh …`, with `uv`/`sudo -u`
   for the two read-only probes) or route every venv mutation through `sudo`
   after a single `chown -R root:root "$work/.venv"`. `RESULT.md` #6 claims "its
   ownership invariants unchanged", which as delivered means the job still fails.

3. **[MAJOR] `scripts/standalone-runtime-check.sh:104,206` — `env -u BLASTIT_BIN`
   is the weaker half of what the node specified, and it leaves the *printed
   evidence* able to describe a binary that was never run.** `NODE.md` item 2 asks
   for `BLASTIT_NO_LOCAL_HANDOFF=1`; that is what makes the check drive `$binary`
   unconditionally. `env -u` only removes the *explicit* candidate: the
   auto-discovered ones are still live, and the script never `cd`s, so `dev` runs
   in the caller's directory. In this very worktree `.venv/bin/blastit` is a
   17 MB ELF, `0755`, owned by the caller, under 0755 ancestors — in a normal
   (non-group-writable) developer checkout that passes §4 and `dev` hands off to
   it, so the script prints `standalone-runtime-check: target/debug/blastit dev
   --port N` and `3/3 passed` about a binary it never started. Fix: put
   `BLASTIT_NO_LOCAL_HANDOFF=1` on both launches
   (`env -u BLASTIT_BIN BLASTIT_NO_LOCAL_HANDOFF=1 BLASTIT_MANIFEST=… "$binary"
   dev …`); with handoff disabled the `env -u` becomes redundant and may stay or
   go. Then the comment at 100-103 is also true without qualification. (In CI,
   cwd is `/`, so this is a local-proof defect, not the job's failure — the "3/3
   passed on a host with no CPython" wording and the `uv python install 3.14`
   assertion are themselves still truthful.)

4. **[MINOR] `crates/blastit/tests/critical_handoff.rs:821-825` — the last
   assertion cannot fail, so it does not prove what its message claims.**
   `exec_verified` re-execs the *same inode* with `BLASTIT_HANDOFF_DONE=1`, and
   `dev` then prints the same missing-`Blastfile` refusal, so `!stderr.is_empty()`
   holds whether or not a silent self-exec happened. The regression the test
   really catches is assertion 1 (`!stderr.contains("refusing local handoff")`) —
   keep that and either reword this one to "`dev` answered" or make the exec
   observable by comparing stderr byte-for-byte against a control run with
   `BLASTIT_NO_LOCAL_HANDOFF=1` (which must be identical, since the *contract* is
   "handoff would have been a no-op").

5. **[MINOR] `crates/blastit/src/host.rs` via `handoff.rs:271-276` — "not 0 or 0"
   for the root case.** `format!("it is owned by uid {}, not {euid} or 0", …)`
   prints `blastit dev: refusing local handoff to /x: it is owned by uid 1001,
   not 0 or 0; set BLASTIT_NO_LOCAL_HANDOFF=1 to use the global blastit` when a
   root user hits it — the sentence is *accurate*, but reads like a bug. Pre-existing
   (not introduced here), yet it is the sentence this node is judged on; branch it:
   `if euid == 0 { "it is owned by uid {} (only root may own it here)" } else { … }`.

6. **[MINOR] `scripts/repack_universal_wheel.py` — correct, and the assertions
   would have caught the regression.** `[pytest11] blastit =
   blastit.pytest_plugin` is reproduced verbatim from `pyproject.toml:88`
   (`[project.entry-points.pytest11] blastit = "blastit.pytest_plugin"`), and
   `test_packaging_gates.py:2161-2176` raises (via `_expect` →
   `AssertionError:2164`) before the second `_expect` can `ValueError` on a
   missing section, so a regression fails cleanly with the file's contents
   printed. `wheel-smoke.sh:104-127` reads the *installed* metadata, which is what
   `pytest` reads, and `python/tests/test_pytest_plugin.py:117` alone would not
   have caught it (it inspects the dev install). No prose defect; the docstrings
   state the past behaviour in the past tense and name the fix.

7. **[MINOR] `python/tests/runnable_docs/harness.py:_pytest_argv` — "the system
   CPython of any distribution" overstates.** Debian/Ubuntu and `uv python
   install` build `EXTERNALLY-MANAGED`; Arch's and Alpine's do not. Replace with
   "a distribution-packaged system CPython (and every `uv python install` build)".
   The `--break-system-packages` comment and the `--system` correction are
   accurate and belong where they are.

8. **[MINOR] `e2e/venv-smoke.sh:60-68` — the "no `sudo`" user gets
   `sudo: command not found` and exit 127, not a sentence naming the contract.**
   A read-only `/var/lib` does give an intelligible
   `mkdir: cannot create directory …: Read-only file system`, so that half is
   fine. Add before line 61:
   `command -v sudo >/dev/null || { echo "venv-smoke: IMAGE=$image must provide sudo with passwordless escalation (modeluv/base does); the server leg boots as root" >&2; exit 1; }`
   (write it with the actual `$image` value, not the literal).

9. **[NOTE] No user-visible CLI surface moved.** `--help`/`--version`/usage
   (`crates/blastit/src/cli/help.rs`) are untouched; no flag, preset name,
   `stdout`/`stderr` stream choice or formatting changed. The single behaviour
   change a user can observe is the one `# Goal` item 2 asks for: a
   `$BLASTIT_BIN` that *is* `/proc/self/exe` now continues in-process (exit code
   becomes `dev`'s own) instead of `refuse()`ing with exit 1 — and
   `.venv/bin/blastit`, `vendor/bin/blastit`, `node_modules/.bin/blastit` keep
   today's behaviour (a console script is never the running inode, so it still
   hands off; a hardlink to the running image is still answered silently by
   `attempt`'s `is_this_image` arm, `handoff.rs:140-145`). The surviving refusal
   sentence (`handoff.rs:185-192`) is still accurate and actionable.

VERDICT: fix

## Round 2

Re-read `git diff ca3d5be -- e2e/venv-smoke.sh e2e/venv-smoke/
scripts/standalone-runtime-check.sh e2e/php/README.md`, plus `run.sh`/`check.py`
in full and `plan/nodes/MB/MB.49/RESULT.md`. Two micro-tests in `bash` were run
to settle the trap questions (§2 below). I did not re-run the Docker leg; I read
the pasted transcript instead, and it is internally consistent (26 `ok` lines,
`exit=0`, `"environment":"…/app/.venv"` present, `"privilege":"root"`).

**Both round-1 blockers are closed, and closed the right way.** (1) The work
directory is now `sudo mktemp -d "$parent/run.XXXXXX"` under a root-owned `0755`
parent, `chown`ed to the invoking uid — no `mkdir` as `user` under a `root:root`
parent. (2) Ownership is fixed at the source the node named: the fixture *and*
the venv live in `$work/app`, one `sudo chown -R root:root "$app"` (run.sh:74),
and every later mutation inside that tree goes through `sudo`. The proof is the
transcript itself: stage 0 booted (`"privilege":"root"`) and served, which is
exactly what the `owned by uid 1000` refusal used to prevent. Nothing in this
round touches a CLI surface (`--help`, `--version`, presets, exit codes, stream
choice): still clean under my standing check.

### What is still wrong (all prose/robustness; no blocker left)

1. **[MAJOR] `e2e/venv-smoke/run.sh:31-34` — the comment states the fact the
   same file disproves 200 lines later.** "init `execve`s `_bin/blastit-dbproxy`
   *after* `setuid(B+3)` (§4) … is the one realistic layout that would hide **the
   wheel's own binary** from the sandbox", against run.sh:241-242: "the child
   that runs the role is this image, `/proc/self/exe --role=dbproxy`, not the
   `_bin/blastit-dbproxy` copy" (measured here, and the reason E2b was rewritten).
   `_bin/blastit-dbproxy` is the one file that provably does *not* matter to this
   boot. A maintainer who reads only the header will add `chmod`s for a file
   nothing execs, and will not know what the `0755` really protects. Replace
   31-35 with:

       # The directory has to stay traversable by the uids blastit drops to: every
       # view imports the app and the wheel's CPython shim as uid B+1000+i, so a
       # 0700 directory is the one realistic layout that would hide both from the
       # sandbox — and /app, /srv/app or a project checkout are all 0755.

   (The `chmod 0755` itself stays; only the reason changes. The wheel's `_bin`
   mode is `check_wheel_shims.py`'s business, as run.sh:235-236 now says.)

2. **[MAJOR] `e2e/venv-smoke.sh:77-80` — a failing cleanup turns a green run into
   a silent `exit 1`.** `cleanup()` is `sudo --non-interactive rm -rf "$work"`
   with no guard, under `set -euo pipefail`, as the `EXIT` trap. Measured:

       $ bash -c 'set -euo pipefail; trap "false" EXIT; echo body-ok; exit 0'
       body-ok          → exit=1     (trap aborted, no message printed)

   So if `sudo -n` stops working (sudoers/timestamp change) or `rm -rf` fails,
   a run with every `ok` line prints nothing about the failure and exits 1. That
   is the identical bug class `run.sh:142-144` was patched for in this same
   delta ("a failing pipeline under `set -e` … becomes the *script's* exit
   status") and RESULT §"Two harness bugs" says cost a round. Fix:

       cleanup() {
           sudo --non-interactive rm -rf "$work" \
               || echo "venv-smoke: could not remove $work (leaving it in the container)" >&2
       }

3. **[MINOR] `e2e/venv-smoke.sh:67-70` — the sudo guard checks presence, so the
   contract sentence is only printed for a *missing* `sudo`.** An image with
   password-prompting sudo passes `command -v sudo` and then dies on
   `sudo: a password is required` (because of `--non-interactive`, so it does
   *not* hang — that part is fixed and good) — sudo's line, not the sentence
   naming what the image must provide. One extra probe makes the guard match its
   message and its name:

       sudo --non-interactive true 2>/dev/null || {
           echo "venv-smoke: IMAGE=$3 must provide sudo with passwordless escalation (modelw/base does); the server leg boots as root" >&2
           exit 1
       }

4. **[MINOR] `e2e/venv-smoke/run.sh:71-73` — the parenthetical enumerates
   operations that no longer exist.** "every mutation below that lands inside it
   goes through `sudo` (the dbproxy stub's `chmod`/`rm`/`write`/`mv`)" — the stub
   is now `$work/dbproxy-stub` (`sudo tee`/`sudo chmod`), and there is no `rm` or
   `mv` of a venv file anywhere in the file (`grep -n 'rm -f\|mv '` → nothing).
   The rule is right; the examples are stale: "(the stub's `tee`/`chmod`, and the
   interpreter's `chmod 0750`/`0755` at 196-209)".

5. **[MINOR] `e2e/venv-smoke/run.sh:92-94` — the one `ok` whose evidence value is
   not printed.** Measured: `head -1` of the console script *names*
   `$app/.venv/bin/python`; printed: "the console script runs on the venv's
   interpreter", with no path — the neighbour at 218-226 shows `($named)` for
   exactly this reason, and `check.py`'s `info` line now says `sys.executable` is
   the base install, so the transcript is the place where "venv's interpreter"
   needs its evidence. Suggested:

       checked "#!$app/.venv/bin/python" \
           "the console script's shebang names the venv's interpreter ($(head -1 "$app/.venv/bin/blastit"))" "$work/console-script.log"

   This is the only over-reading left in the suite, and it is a tightening, not
   a falsehood: the shebang is what the kernel honours, and the launch is
   documented as `$app/.venv/bin/blastit`.

6. **[MINOR] `e2e/venv-smoke/check.py:81-86` — the `info` line is a fact, but its
   pointer is one hop off.** It reads as a fact ("the base install the venv was
   built from"), not as an apology, and it is actionable — good, and the demotion
   is right per `env.rs`'s `PyConfig.home` (RESULT Follow-up 1). But "the venv's
   packages are on sys.path — see the console script and the boot's
   `environment`" hands the reader two facts that prove the *interpreter/venv
   identity*, while the sys.path claim is the `ok` line printed directly above
   it. Say that, and name the file so the pointer is followable:

       f"  info sys.prefix={document.get('prefix')!r} "
       f"sys.executable={document.get('executable')!r} "
       "(the base install, per §2.3 — hence not asserted; the checks above prove "
       "the venv's site-packages, and `run.sh` the console script and the boot's "
       "`environment`)"

7. **[MINOR] `scripts/standalone-runtime-check.sh:102-108` — the comment explains
   why `BLASTIT_NO_LOCAL_HANDOFF=1` is needed but not why `env -u BLASTIT_BIN`
   now stays.** Half the paragraph is about the setting that is *no longer the
   defence*, and the sentence "the auto-discovered candidates … are still live"
   describes the old hole in the present tense. One clause fixes it: after
   "turns the whole handoff off", add "(`env -u` stays only so the run does not
   inherit a caller's `BLASTIT_BIN`; it is no longer what makes this honest)".
   Separately, the quoted failure mode is one shape too strong: with a handoff,
   the static half still passes but the third check *fails* (`NoShim`, or a
   served WSGI route), as lines 34-37 of this same file say — so it is "results
   that describe a binary that never ran", not literally `3/3 passed`. The claim
   that matters ("the artifact this run was handed is the only program allowed to
   answer", 211-212) is true.

8. **[MINOR] `e2e/venv-smoke/run.sh:243-248` — "executable by another uid" should
   name the uid that checked.** `[ -x ]` is this shell's `access(2)`; it answers
   the *other* bits only because the file is `root:root` after line 74 — which is
   the one thing that would silently stop being true if the `chown` were ever
   dropped (the check would then be reading owner bits and still print the same
   sentence). The `stat` parenthetical is the right instinct; add the checked
   identity: `(755 root:root, access() as uid $(id -u))`. The wording is
   otherwise accurate and strictly better than the retired boot-refusal claim.

9. **[MINOR] `e2e/php/README.md:30-34` — "floats with the suite" is ambiguous,
   and the sentence is an incident narrative.** In this page "suite" first reads
   as *this* e2e suite, which pins nothing; the intent is Debian's `trixie`
   suite. And "started answering `Version '…' was not found` and the image
   stopped building — so …" retells the breakage instead of stating the rule
   (line 39-40 already states it correctly). Replacement for 30-34's tail:

       the `libphp8.4` package is **not** point-pinned: `trixie-security`
       supersedes a point version on Debian's own schedule, so a `=8.4.27-…` pin
       stops resolving. The digest is the pin this file can keep; the package
       follows the `trixie` suite (`MB.49`).

   The reproducibility claim *is* correctly withdrawn — nothing else in the page
   pins `libphp`, and `Dockerfile:11` says "the built wheel + Debian's NTS
   libphp8.4" with no version. The `(MB.49)` tag matches this page's existing
   `MB.3.32`/`MB.3.56` usage, so it is not a leak.

### Informational

- `e2e/venv-smoke.sh:81-82`'s `INT`/`TERM` traps are inert for the work they look
  like they do. Measured: with a foreground child, bash defers a trapped signal
  until the child returns (`sleep 2` ran to completion, `EXIT-TRAP-ran` printed
  twice, exit 0, `INT` trap never ran), and in a non-TTY `docker run` the CLI
  forwards the signal to PID 1 only. So Ctrl-C during `uv pip install` returns
  the shell and leaves the container running to completion; `--rm` still cleans
  up, and a cancelled CI job SIGKILLs the container anyway. Harmless as written —
  worth one clause so nobody reads them as an interrupt path.
- `e2e/venv-smoke/run.sh:10-17` still says "the two things the wheel could get
  wrong"; the delta added a third (console-script shebang) and re-scoped E2b. Say
  "three".
- `check.py:3-4`'s usage line uses `/app/.venv/...` while the real path is
  `$work/app/.venv/...` — schematic examples in a docstring are fine by me, noted
  only so it is a choice, not a drift.

### Answers to the five questions

1. **Still honest?** Yes, and the split is now defensible: `sys.prefix`/
   `sys.executable` are printed as facts (not excused), `library` proves the
   site-packages, the console script proves the shebang, the boot's `environment`
   proves what the boot recorded. One `ok` still says a hair more than it
   measured and shows no value (finding 5); nothing claims a `sys.prefix` fact.
2. **`__pycache__`/`sudo`:** no `__pycache__` problem exists — after the `chown`
   the view uids (B+1000+i) silently cannot write `$app/__pycache__`, which Python
   skips without a message, and `check.py` is `__main__`, so it writes none. No
   residual `Permission denied`: `EXIT` runs `sudo rm -rf "$work"`, which removes
   the root-owned `$app` too. The read-only `/var/lib` case prints `mkdir: …:
   Read-only file system`; the sudo case is finding 3 (a sentence, not a hang —
   `--non-interactive` guarantees that); the trap chain has the silent-status bug
   in finding 2.
3. **`standalone-runtime-check.sh`:** the printed evidence is now true — with
   `BLASTIT_NO_LOCAL_HANDOFF=1` the only program that can answer is `$binary`, so
   `3/3 passed` (2 static + 1 WSGI refusal) is fair, and the host guard at 32-52
   still backs the "on a host with no CPython" wording. Nothing in the file claims
   self-handoff coverage, so nothing over-claims after the change; two wording
   nits in finding 7.
4. **`e2e/php/README.md`:** the file no longer claims a reproducibility it does
   not have, and the "surface absent by construction" claim is preserved and
   still the honest one. "Floats with the suite" plus the incident retelling want
   the rewording in finding 9; no roadmap/apology tone, no diary heading.
5. Nothing else user-visible moved: no CLI help/flag/exit-code/stream change, no
   new refusal sentence beyond the (improved) venv-smoke guard.

VERDICT: fix

### Round 2 addendum (image name)

My round-1 note quoted the pilot image as `modeluv/base`; that was a typo on my
side. Every occurrence in the tree is `modelw/base` — `e2e/venv-smoke.sh:4,23,68`,
`e2e/README.md:57`, `.github/workflows/ci.yml:700` — so the new guard's
parenthetical `(modelw/base does)` is correct, and `IMAGE=$3` prints the actual
value when a different image is passed. Nothing to change.

VERDICT: fix
