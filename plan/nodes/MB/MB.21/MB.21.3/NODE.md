---
id: MB.21.3
title: "Blastfile toolchain: bounded lexer, LL(1) parser, path compiler, fmt, surgical editor, docs_fences"
kind: epic
status: done
priority: 1
depends_on: [MB.21.2]
paths: []
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:38"
updated: "2026-11-16T08:01"
---

# Goal

Build the Blastfile toolchain in `crates/blastit-manifest`: a bounded lexer, an LL(1) parser and the
lossless CST it builds, the lowering that produces the `Manifest` the rest of Blastit consumes, a
path-pattern compiler, the canonical `blastit fmt`, the surgical comment-preserving editor `--learn`
depends on, and the `docs_fences` validator for ```` ```blastfile ```` blocks.

Everything here is **additive**: the retired reader keeps working until `MB.21.4.5` deletes it, so each
leaf lands green on its own and `make check` never depends on a future leaf.

# Context

- Design and every decision (grammar, CST, lower, error contract, refusals) are in
  `docs/design/blastfile-dsl.md` + `ADR-0012` (`MB.21.2`). Follow it; if it is silent, stop and open a
  `human` request instead of inventing a rule.
- The parser's proof obligation is model **equality**, but never against a second file format: for every
  row of `MB.21`'s capability map, the lowered `Manifest` is compared with the **expected `Manifest`
  built in the test**, and every row the design drops is asserted absent. That single oracle is what
  guarantees "nothing real is dropped" (`MB.21.3.8`).
- Protected suites rewritten here (`crates/blastit-manifest/tests/critical_manifest_edit*.rs`,
  `edit_branches.rs`) are listed in `paths` with the reason in each child's `# Goal`; every leaf runs
  `uv run --no-project tools/critical_guard.py --base <sha> --node <id>`.
- Leaves share the `blastfile/` module tree, so they are **serialized** by `depends_on` rather than run
  in parallel; `lead` may pair a leaf with a docs leaf from `MB.21.5` because those paths are disjoint.
- `docs/knowledge/LESSONS.md` applies: bound every loop with the iterator (never `i += 1`), keep
  `cargo fmt --all` as the formatter check, and never widen a protected test to get green.

## Children

| Node | What | Depends |
|---|---|---|
| `MB.21.3.0` | Model parity (a): `Manifest` name/mounts/php/`equivalent`, `StaticMount`, `Route.sandbox`, `MountTable`, uid by sandbox, `StaticPolicy::fallback` — design `MB.21.3.0a` | `MB.21.2` |
| `MB.21.3.7` | Model parity (b): delete the per-view static/PHP accessors, `StaticOutcome::fallback_missing`, the scheduler and `blastit` call sites, the three suites — design `MB.21.3.0b` | `MB.21.3.0` |
| `MB.21.3.1` | `blastfile/` module + bounded lexer (tokens, `$IDENT`, raw/escaped strings, trivia, `line:col`) | `MB.21.3.7` |
| `MB.21.3.2` | LL(1) parser and the lossless CST it builds — design `MB.21.3.2a` | `MB.21.3.1` |
| `MB.21.3.8` | Lowering to `Manifest`, the parity oracle, the nine suites and `tests/fixtures/` — design `MB.21.3.2b` | `MB.21.3.2` |
| `MB.21.3.3` | Path-pattern compiler, `regex '…'`, and `HEAD` implied by `GET` | `MB.21.3.8` |
| `MB.21.3.4` | Lossless CST printer + `blastit fmt [--check] [PATH]` | `MB.21.3.3` |
| `MB.21.3.5` | Surgical comment-preserving `--learn` editor on the CST (replaces `edit.rs`'s old patcher) | `MB.21.3.4` |
| `MB.21.3.6` | `docs_fences` validator for ```` ```blastfile ```` complete documents (strict flip in `MB.21.5.2`) | `MB.21.3.5` |

The design's `MB.21.3.0a`/`3.0b` and `3.2a`/`3.2b` are four plan leaves here: the planner cannot mint a
letter-suffixed id, so the second half of each split is `MB.21.3.7` and `MB.21.3.8`, and the design id
is carried in each title.

# Acceptance

```console
$ uv run --no-project tools/plan.py tree --under MB.21.3
$ make check
$ cargo test -p blastit-manifest --locked
$ ls crates/blastit-manifest/src/blastfile/ crates/blastit-manifest/tests/critical_blastfile_*.rs
$ grep -rn 'blastfile' crates/blastit-manifest/src/lib.rs
```

1. Every child is `done` with command + output evidence in its `RESULT.md`.
2. `make check` exit 0 for the whole tree — no leaf needed a later leaf to be green.
3. The model-equality oracle exists and is protected: one fixture per capability-map row, compared with
   the `Manifest` the test builds (`Manifest::equivalent`), with every row the design drops asserted
   absent.
4. Zero new workspace dependencies: `git diff Cargo.lock` is empty unless a child lists `Cargo.toml`
   and `Cargo.lock` in `paths` and says why (it should not; the dependency removal is `MB.21.4.5`'s).
5. The retired reader is untouched and every existing manifest test still passes (through a bridge named
   in `MB.21.3.0`/`MB.21.3.7`'s `RESULT.md`), so `MB.21.4` starts from green.

# Notes

- Keep `Manifest`'s names and `Manifest::version()`/`global_groups()`/`PhpPolicy::workers` as they are;
  the only model changes are the ones `MB.21.3.0`/`MB.21.3.7` carry. If a spelling needs a *new* field,
  that is an `ADR-0012` amendment and a design change, not an implementer's choice.
- Mutation survivors here are closed by `MB.21.5.3`; write the deny-side assertion now anyway
  (skill `testing`: a policy test that passes with the check removed is fluff).
