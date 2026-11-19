# MB.42.9 — an unsafe `$VIRTUAL_ENV` is skipped, not `exit 1`

## What changed

`crates/blastit/src/cli/handoff.rs` — `$VIRTUAL_ENV/bin/blastit` is **ambient**, so it now
carries `explicit: false` and shares the auto-discovered policy: one calm skip line
(`skipping local binary <path> (<reason>); continuing with the next candidate or the global
binary`) and probing continues to `vendor/bin`, `node_modules/.bin`, then the global binary.
Every §4 check is unchanged — an unsafe binary is still never executed; only the reaction
changed.

`exit 1` now belongs to exactly one candidate: **`$BLASTIT_BIN`** (the wheel's own binary
override), which is the only name that really points at a path. It is appended **last**, so
the pinned order (`$VIRTUAL_ENV`/`./.venv` → `vendor/bin` → `node_modules/.bin`) is untouched
and a named binary can never shadow a working project install; when it fails a check the
sentence and status are the old ones (`refusing local handoff to …; set
BLASTIT_NO_LOCAL_HANDOFF=1 to use the global blastit`, `exit 1`).

`Candidate` keeps one boolean meaning "the user named this path", `refuse()` keeps its single
job, `handoff.rs` is 428 prod LOC.

`crates/blastit/tests/critical_handoff.rs` — `an_unsafe_activated_venv_is_refused_with_the_fix`
became `an_unsafe_activated_venv_is_skipped_and_the_next_candidate_runs` (skip line on `stderr`
naming path + `0777` + "continuing with the next candidate or the global binary", the next
candidate's `done=1` marker proves it ran, `exit 0`, no "refusing", the unsafe shim never runs).
The `$BLASTIT_*`-explicit path stays covered by
`an_unsafe_blastit_bin_is_refused_with_the_fix` (`exit 1`, refusal sentence, the fix named, no
skip line, stdout empty, the unsafe shim never runs). 15/15 pass.

`Makefile` — the `BLASTIT_NO_LOCAL_HANDOFF=1` workaround is **gone** from `test-critical`; the
tier is green without it (evidence below), because the `$VIRTUAL_ENV` that `uv run` exports now
skips the `0775` checkout instead of refusing it.

## Evidence

```
$ VIRTUAL_ENV=$PWD/.venv ./target/debug/blastit fmt --check e2e/venv-smoke/Blastfile
blastit fmt: skipping local binary /…/release-0.2.0/.venv/bin/blastit (/home/remy/offworld-nexus/blastit
  is mode 0775, so another user could replace the binary); continuing with the next candidate or the global binary
exit=0
```

(The literal `fmt --check Blastfile` in this checkout reaches `fmt`'s own `Blastfile: no such file
or directory` → `exit 2`, because this repository has no root `Blastfile`; pre-fix it never got
that far — it exited `1` on the refusal. With a real formatted manifest the same command shape
prints the skip line and exits `0`, above.)

The refusal is still real, and only for a named path — note both policies on the *same* file:

```
$ BLASTIT_BIN=$PWD/.venv/bin/blastit ./target/debug/blastit fmt --check e2e/venv-smoke/Blastfile
blastit fmt: skipping local binary /…/.venv/bin/blastit (/home/remy/offworld-nexus/blastit is mode 0775, …);
  continuing with the next candidate or the global binary
blastit fmt: refusing local handoff to /…/.venv/bin/blastit: /home/remy/offworld-nexus/blastit is
  mode 0775, so another user could replace the binary; set BLASTIT_NO_LOCAL_HANDOFF=1 to use the global blastit
exit=1
```

```
$ cargo test -p blastit --locked --test critical_handoff
running 15 tests
test an_unsafe_activated_venv_is_skipped_and_the_next_candidate_runs ... ok
test an_unsafe_blastit_bin_is_refused_with_the_fix ... ok
…
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
```

```
$ make test-critical            # no BLASTIT_NO_LOCAL_HANDOFF in the recipe any more
protected critical tests hold no skip construct
cargo build --locked -p blastit
python -m unittest discover -s python/tests/critical
.....................................................
Ran 53 tests in 9.974s
OK
```

```
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.06s
$ cargo clippy -p blastit --all-targets --locked --no-default-features -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.69s
```

```
$ make fast-check
… clippy --workspace --all-targets --features python -- -D warnings (clean)
… 68 files already formatted
… plan OK
… critical_handoff: test result: ok. 15 passed; 0 failed; …
exit=0
```

## Design amendment for the lead to route (no `docs/**` touched here)

`docs/design/universal-distribution.md` §4 (and the "skip vs exit" sentence it echoes —
also `docs/decisions/ADR-0017-universal-distribution.md`) needs a one-line amendment:

> `$VIRTUAL_ENV` is **ambient**, not explicitly named: an unsafe activated venv is **skipped**
> and probing continues. `exit 1` is reserved for an explicitly named install (`$BLASTIT_BIN` /
> `BLASTIT_*`), where the user really did point at a path.

§4’s candidate list should also note `$BLASTIT_BIN` as the trailing explicit candidate.

## Follow-ups

- `$BLASTIT_BIN` is now a handoff candidate (last, explicit). If the docs page (`MB.42.4`)
  documents the candidate order it should mention it; the environment table already lists
  `BLASTIT_BIN` for the wheel.
- On a `0755` checkout `$VIRTUAL_ENV/bin/blastit` becomes *safe*, so `uv run` + `make
  test-critical` would then hand off to the checkout's own venv binary (pre-existing
  behaviour, unrelated to this node).
