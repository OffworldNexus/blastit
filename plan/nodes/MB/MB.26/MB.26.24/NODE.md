---
id: MB.26.24
title: "Close the three escapes a goldfish review proved against the committed lsp-portable guard: target-class paths, src/bin roots, package renames"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: [scripts/check_lsp_portable.py, scripts/test_check_lsp_portable.py, "docs/testing/E2E-MB.35.md"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T12:14"
updated: "2026-11-18T12:14"
---

# Goal

Close the three escape classes a goldfish review of `MB.35.6` proved against the **committed**
`scripts/check_lsp_portable.py` (`3d0d235`). Each was reproduced on the real crate by the lead,
with `cargo` exit 0 and the guard exit 0, and each makes the guard print a sentence that is not
true ("target paths under `src/`", "`[dependencies] = ['blastit-manifest', 'serde_json']`").

1. **`[[test]]` / `[[bench]]` / `[[example]]` paths are never read.** Adding
   `[[test]] name = "escapetest" path = "/tmp/opencode/esc/escape_test.rs"` whose body is
   `use std::process::Command` compiles under `cargo check -p blastit-lsp --offline --all-targets`
   (exit 0, `Finished dev profile`), which is exactly what `make rs-test`
   (`cargo test --workspace --all-targets`) builds, while `_check_target_path` sees only `[lib]`
   and `[[bin]]`. Refuse a `test`/`bench`/`example` table, or pass every one of them through the
   same containment check.
2. **An auto-discovered `src/bin/*.rs` is a crate root nobody pins.**
   `crates/blastit-lsp/src/bin/probe.rs` holding `unsafe { *p = 2 }` compiles under
   `--all-targets` (exit 0) and the guard answers `ok — … 4 source file(s) …` : it counts the
   file, but `#![forbid(unsafe_code)]` is required only of `src/lib.rs` and `src/main.rs`. Derive
   the crate-root set from `cargo metadata`'s `targets[].src_path` (or refuse `src/bin/`
   outright) and require the attribute in **every** root, so design §1.3 row 3 is a checked fact
   for each binary the crate can produce.
3. **The dependency ceiling matches TOML *keys*, not packages.**
   `serde_json = { version = "2.8.3", package = "memchr" }` leaves the guard green and its ok line
   prints `[dependencies] = ['blastit-manifest', 'serde_json']` — the key, not the package that
   will be linked. (The build still fails with `E0425`/`E0432` on this crate, so it is a false
   sentence rather than a shipped escape; a rename a crate's code could satisfy is the dangerous
   case.) Refuse a spec whose `package` differs from its key, or intersect `cargo metadata`'s
   direct dependency package names with `ALLOWED`.

# Context

`MB.35.6`'s round 5 replaced the line-local text scan with a whole-file token analysis, and the
reviewer's source-token probes all failed to defeat it (`use std::{self as s}`, alias chains,
globs, `$r` macro passes, raw identifiers, split paths). The three defects above live on the
other side of that boundary — the **manifest** and the **crate-root set** — which the guard reads
but does not fully derive. Keep the token analysis; extend what it is anchored to.

**A fourth item, added by the lead after `MB.36.3`.** The `closure` promise's `SERVING` tuple
still names only the pre-`MB.36` crates, so `blastit-view-static`, `blastit-view-python` and
`blastit-view-php` — the three crates `MB.36` creates, each of which links a runtime or a
privilege the portable server must never see — are absent from the denylist that is supposed to
name them. Add all three (and check the tuple against `crates/` so the next crate cannot be
forgotten), or close the property the way the docstring already proposes by freezing the
reviewed closure in a checked-in file.


`docs/testing/E2E-MB.35.md` §6/§7 quote the guard's sentences, so the wording change ("target
paths under `src/`" only when it is true for every target class) must be reflected there, and the
one stale number it still carries (2000639 archive bytes vs the surviving artefact's 2000670) is
corrected in the same pass.

# Acceptance

```console
$ uv run --no-project scripts/check_lsp_portable.py          # green on this tree
$ UV_OFFLINE=1 uv run --no-project scripts/test_check_lsp_portable.py
$ make lsp-portable
$ make check
```

plus one **compiling** negative twin per class, proved red for the right property:
`[[test]]` with a path outside `src/`; `src/bin/x.rs` with an `unsafe` block and no
`#![forbid(unsafe_code)]`; a `package =` rename; and a `closure` twin whose fixture reaches a new
view crate by name, so the `SERVING` tuple cannot silently omit one again. Each twin's fixture
must be shown to build with `cargo check --all-targets` first, or it proves nothing.

# Notes

- No `--allow`, no `--force`, no table of exceptions: the fix is derivation, not a new denylist.
- The guard's success line must be re-worded to match what it actually checked; a sentence it
  cannot support is the defect this node exists for.
- Do not reopen `MB.35.6`; it is `done` and committed (`3d0d235`, gate `4aa39d9`).
