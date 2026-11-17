# RESULT — MB.21.5.5 (attempt 1)

## Summary

R24's gate no longer stops at one literal. `scripts/check_manifest_purity.py` now reads each
covered file twice: the raw lines for the literal `std::env` (unchanged, comments and strings
included, which is design §17.2's own sentence), and the code tokens for every wider spelling —
`std :: env`, `::std::env`, `use std::env as e`, `use std::{fs, env}` at any nesting or
multi-line layout, `use ::std::{env as e}`, `use {std::env}`, `use std::{io, env::{self, var}}`,
and any `env::…` path. The token pass is a tiny single-pass Rust lexer (nested `/* */`,
`//`, `"…"`, `c"…"`, `r#"…"#`, char literals/lifetimes) plus a recursive `use`-tree walker, so a
comment inside `use std::{…}` is trivia rather than a parse failure, and prose mentioning `env`
cannot trip it. Stdlib only (`re`, `argparse`, `pathlib`, `sys`), no new dependency, no wiring
change: `make manifest-purity` and its place in `make check` are untouched, and the refusal keeps
the `std::env is not allowed here` line and the verbatim sentence "a Blastfile names variables and
never reads them (R20/R24)".

**The R24 rule chosen (design §17.2 is "if `std::env` appears"):**

1. The literal `std::env` is a hit on any raw line — comments and string literals included (as
   before; no input that failed before now passes).
2. The wider spellings are read from **code only**; comments and string/char/raw literals are
   trivia, so a comment cannot break a `use` tree and prose cannot trip the rule. A comment that
   spells `use std::{env}` imports nothing and passes; a comment that spells `std::env` still hits
   (pass 1).
3. `std::env::consts` **fails**: it names the module, §17.2 says "appears", the pre-fix gate failed
   it, and the audit's own proposed `\bstd::env\b` does too. It is an over-approximation, and a
   deliberate one — allowing it would be a §17.2 design change, not a gate relaxation. The covered
   crate uses no target constants.
4. Near-misses that pass: `env` on its own (`let env = …`, `env.len()`, prose in a comment, a
   `"env"` or `"env::var is a phrase"` literal, `r#"env::var"#`), and every other std module
   (`std::fs`, `std::process`, `std::{fmt, io}`).
5. Not chosen: clippy `disallowed-methods` (audit §5.1's stronger alternative). The script now
   resolves the path through nested imports, so the workspace-wide `clippy.toml` change is not
   needed.

## Evidence

### Acceptance 1 — the three spellings are caught, plus the nesting the audit named

`scripts/test_manifest_purity.py` carries one fixture per spelling (37 fixtures, 82 assertions);
each expects exit 1 and the exact `file:line` in stderr. Measured before/after on the audit's own
scratch tree (`/tmp/opencode/mb2155-purity`, `docs/testing/AUDIT-MB.21.md` §5.1):

```console
$ git show HEAD:scripts/check_manifest_purity.py > /tmp/opencode/mb2155-purity/old_gate.py
$ uv run --no-project /tmp/opencode/mb2155-purity/old_gate.py --root /tmp/opencode/mb2155-purity
manifest-purity: ok — 1 source file(s), no std::env
exit=0

$ uv run --no-project scripts/check_manifest_purity.py --root /tmp/opencode/mb2155-purity
crates/blastit-manifest/src/evasive.rs:2: std::env is not allowed here: use std::{env};
crates/blastit-manifest/src/evasive.rs:3: std::env is not allowed here: env::var("HOME").ok()
manifest-purity: 2 std::env use(s) and 0 unclassifiable construct(s) under crates/blastit-manifest/src/; a Blastfile names variables and never reads them (R20/R24)
exit=1
```

Fixtures proved (all exit 1, all named in stderr): `evasive.rs` [2, 3], `list.rs`, `alias.rs`,
`group.rs`, `multi.rs` [3] (hit on the module's line, not `use`), `absolute.rs`, `root_group.rs`,
`spaces.rs`, `call_only.rs`, `comment_in_group.rs` [4], `comment_before.rs`, `string_then_use.rs`
[2], `string_literal.rs` [1], `raw_alias.rs`, `raw_in_group.rs`, `raw_rename.rs` (raw identifiers),
the alias-root group (`alias_root.rs` [2], `extern_root.rs` [2], `chained_root.rs` [3],
`alias_call.rs` [2]), and the refusals (`alias_overlong.rs` [1], `glob_alias.rs` [1],
`comment_glob.rs` [2], `mod_b.rs` [1, 2] beside a clean `mod_a.rs`).

### Acceptance 2 — both directions in the self-test, and it fails without the fix

```console
$ UV_OFFLINE=1 uv run --no-project scripts/test_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 82 assertions, 37 fixtures proved, 16 real source file(s)
exit=0
```

The same self-test run against the pre-fix gate (copied from `HEAD`) refuses to be green:

```console
$ uv run --no-project /tmp/opencode/mb2155-purity/test_old_gate.py
manifest-purity self-test: the scan gave [] for crates/blastit-manifest/src/evasive.rs, expected [2, 3]
exit=1
```

The positive direction is fixture 4 (`LEGAL`: comments, `"env"`, `"env::var is a phrase"`,
`let env`, `env.len()`, `std::fs`, `std::process`, `std::{fmt, io}`) which must scan `[]` and exit
0, plus the plain clean fixture (`fn main() {}`) and the audit's direct spelling.

### Acceptance 3 — no false positive on the real tree

```console
$ grep -rn "env" crates/blastit-manifest/src --include=*.rs | grep -E "env::|std::env|use std"
(no output)
$ UV_OFFLINE=1 uv run --no-project scripts/check_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
exit=0
```

No doc-comment exception or allowlist is needed, but the crate does mention `env`: five source
files match, so five run in strict mode and still pass. Three are prose or a local name
(`validate.rs` fixture paths, `lexer.rs`'s `env` method, `lower.rs`'s comment); the other two are
the compile-time `env!("CARGO_MANIFEST_DIR")` inside `mod tests` at `blastfile/syntax.rs:705` and
`blastfile/print.rs:942`. Those two are the macro spelling the gate cannot see — see the `env!`
Follow-up — and every construct around them is classifiable, which is why the tree is `ok`.

### Acceptance 4 — `make manifest-purity` and `make check` stay green

```console
$ make manifest-purity
uv run --no-project scripts/check_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
uv run --no-project scripts/test_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 82 assertions, 37 fixtures proved, 16 real source file(s)
exit=0
```

```console
$ make docs-unit
...
107 passed, 65 subtests passed in 1.84s          # exit 0

$ timeout 1800 make check            # /tmp/opencode/mb2155-check-final.log
uv run --no-project scripts/check_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
uv run --no-project scripts/test_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 44 assertions, 18 fixtures proved, 16 real source file(s)
uv run --no-project tools/plan.py validate
plan OK
exit=0                                 # 4616-line log, 0 `test result: FAILED`, 0 `make: ***`
```

That `make check` is the attempt-1 run (44 assertions, 18 fixtures). The review fixes are proved
below; `make check` is deliberately not re-run now because the shared tree is red from another
epic's uncommitted docs.

Hygiene: `uvx --offline ruff check` and `ruff format --check` on both scripts are clean (scripts
are outside `py-lint`'s `python tools` scope; run anyway).

Note on the dispatch's `scripts/check_manifest_purity.py crates/` form: the interface is
`--root <repo root>` and `COVERED` (`crates/blastit-manifest/src`) is appended, so a positional
`crates/` is a usage error (exit 2) both before and after this node. The real-tree invocation is
the no-argument one pasted above (equivalently `--root .`).

## Files

- `scripts/check_manifest_purity.py` — lexer (`_tokens`, `_block_comment`, `_quoted`,
  `_raw_string`, raw identifiers), `use`-tree walker (`_tree`, `_use_paths`, `_use_end`, `_alias`,
  `_use_spans`), origin resolution (`_std_names`, `_unknown_aliases`), the fail-closed classifier
  (`_inspect`, `_hits`, `_refusals`), `_sources`, `scan`, `scan_refusals`, and `main`. `NEEDLE`,
  `covered`, the exit codes, the hit line and the R20/R24 sentence are unchanged.
- `scripts/test_manifest_purity.py` — the nested/absolute/alias/`self`/raw-identifier/multi-line,
  comment and string deny fixtures, the alias-root fixtures, the fail-closed fixtures (glob,
  comment+glob, cross-file `crate::` alias, over-budget window), the `LEGAL` + `LEGAL_FILES` allow
  corpus, the `std::env::consts` over-approximation case, and self-counting assertions/fixtures in
  the closing line.

## Review Resolution

- reviewer MAJOR (raw identifiers evade the gate) -> fixed ->
  `scripts/check_manifest_purity.py:182` (`_tokens` emits `r#env` as `env`, `r#e` as `e`; never a
  bare `r` plus `#`), fixtures `scripts/test_manifest_purity.py:95` `raw_alias.rs`, `:100`
  `raw_in_group.rs`, `:105` `raw_rename.rs`, `:110` `raw_bare.rs`.
- reviewer MINOR (an unclassifiable `use` is silently skipped) -> fixed ->
  `scripts/check_manifest_purity.py:307` (`_use_paths` returns None over the token budget),
  `:428` (`_inspect` marks a discarded window that names `env` as a refusal), `:584`
  (`scan_refusals`), `:620` (`main` refuses with "cannot classify within the token/depth budget"
  and the `file:line`); fixture `scripts/test_manifest_purity.py:152`
  (150 aliases). `_use_end` is now uncapped so the whole window is visible to the check.
- reviewer MAJOR, round 3 (a `std` alias root hides the import: `use std::{self as s}; use s::env
  as e; e::var("HOME")`) -> fixed by **resolving origin, not enumerating spellings** ->
  `scripts/check_manifest_purity.py:355` (`_std_names` grows `std` through `use std::{self as s}`,
  `extern crate std as s` and `use s as t` chains to a fixed point, bounded by
  `_MAX_ALIAS_PASSES`), `:428` (`_inspect` hits any `use` path whose root is a std name and whose
  leaf anywhere is `env`, and any `X::env` call where `X` is a std name). Rule: R24 is about
  `std::env`, so an honest `mycrate::env` module stays legal; a local module that reads the
  environment contains its own `std::env` and fails there.
- reviewer MINOR, round 3 (missing aliased-root fixtures) -> the corpus is now every bypass from
  all three rounds: `scripts/test_manifest_purity.py:95–148` (nested groups, `pub use`, raw
  identifiers, alias roots, `extern crate`, chained aliases, aliased call, over-budget window) and
  the allow side at `:163–214` (`LEGAL`, `clean_overlong.rs`, `mycrate_env.rs`, `glob_no_env.rs`).
- reviewer BLOCKER, round 3 (`use std::*; use env as e; e::var("HOME")` — a glob names the module
  unnameably) and MAJOR (a cross-file `pub use std::{self as s}` re-export read through
  `crate::mod_a::s::env`) -> fixed by the lead's structural rule: **in a file that mentions `env`,
  every `use` item and `X::…` path must be fully classifiable, else refusal** ->
  `scripts/check_manifest_purity.py:105` (`_ENV_WORD` sets strict mode), `:102`/`:391`
  (`_UNTRUSTED_ROOTS`, `_unknown_aliases` resolve `crate::`/`super::`/`self::` and deep-path
  aliases as outside-file), `:504` (glob refusal), `:428` (`_inspect`), `:584` (`scan_refusals`).
  Fixtures `scripts/test_manifest_purity.py:162` `glob_alias.rs`, `:168` `comment_glob.rs`
  (deliberate over-approximation), `:176` `mod_a.rs`+`mod_b.rs`.
- reviewer MINOR, round 3 (the module docstring over-promised: it said *any* alias of an
  outside-file origin is refused, but `check_manifest_purity.py:524` refuses only when the path
  names `env` or carries an alias, so a strict file's alias-less `use crate::a::b;` passes) ->
  fixed -> `scripts/check_manifest_purity.py:39-46` now states the actual rule: a glob and an
  over-budget window are fail-closed, and an alias — or any path naming `env` — whose origin is
  outside the file is refused; a plain alias-less `crate::` path that names no `env` passes (the
  five real strict files rely on it).
- reviewer MINOR, round 3 (`RESULT.md` claimed "the crate never mentions `env` at all") -> fixed ->
  `RESULT.md:97-102` now names the five strict files, including the two compile-time
  `env!("CARGO_MANIFEST_DIR")` calls at `blastfile/syntax.rs:705` and `blastfile/print.rs:942`
  (inside `mod tests`), and points at the `env!` Follow-up.



Both directions, on `/tmp/opencode/mb2155-review` (the four fixtures compile: `rustc --edition 2021
--crate-type lib` exits 0 on each, so they are real evasions, not syntax errors):

```console
$ git show 06874ae:scripts/check_manifest_purity.py > /tmp/opencode/mb2155-review/old_gate.py
$ uv run --no-project /tmp/opencode/mb2155-review/old_gate.py --root /tmp/opencode/mb2155-review
manifest-purity: ok — 4 source file(s), no std::env
old-exit=0

$ uv run --no-project scripts/check_manifest_purity.py --root /tmp/opencode/mb2155-review
crates/blastit-manifest/src/raw_alias.rs:1: std::env is not allowed here: use std::r#env as e;
crates/blastit-manifest/src/raw_in_group.rs:1: std::env is not allowed here: use std::{r#env as e};
crates/blastit-manifest/src/raw_rename.rs:1: std::env is not allowed here: use std::{env as r#e};
crates/blastit-manifest/src/alias_overlong.rs:1: suspect `use` item: cannot classify within the token/depth budget, and its window names `env`: use {
manifest-purity: 3 std::env use(s) and 1 suspect `use` item(s) under crates/blastit-manifest/src/; a Blastfile names variables and never reads them (R20/R24)
new-exit=1
```

After the fixes, the clean tree, the self-test and the Makefile stage:

```console
$ uv run --no-project scripts/check_manifest_purity.py
manifest-purity: ok — 16 source file(s), no std::env
real-exit=0

$ uv run --no-project scripts/test_manifest_purity.py
manifest-purity self-test: ok — 82 assertions, 37 fixtures proved, 16 real source file(s)
self-test-exit=0

$ make manifest-purity
manifest-purity: ok — 16 source file(s), no std::env
manifest-purity self-test: ok — 82 assertions, 37 fixtures proved, 16 real source file(s)
make-exit=0
```

`std::env::consts` still fails (self-test case 3), the refusal sentence and the `NEEDLE` line are
verbatim, stdlib only, no Makefile change.

### Round 2 evidence (alias roots)

The fixtures are the self-test's own (`NESTED` + `SUSPECTS`, written to
`/tmp/opencode/mb2155-denies`; the allow side to `/tmp/opencode/mb2155-allows`). Of the 25 deny
fixtures, 22 are invisible to `06874ae` (the other three contain the literal `std::env` and were
already caught — that is the old gate's whole reach):

```console
$ uv run --no-project /tmp/opencode/mb2155-review/old_gate.py --root /tmp/opencode/mb2155-missed
manifest-purity: ok — 22 source file(s), no std::env
old-exit=0

$ uv run --no-project scripts/check_manifest_purity.py --root /tmp/opencode/mb2155-missed
crates/blastit-manifest/src/two_deep.rs:1: std::env is not allowed here: use std::{io::{self, Write}, env};
crates/blastit-manifest/src/alias_overlong.rs:1: suspect `use` item: cannot classify within the token/depth budget, and its window names `env`: use {
manifest-purity: 22 std::env use(s) and 1 suspect `use` item(s) under crates/blastit-manifest/src/; a Blastfile names variables and never reads them (R20/R24)
new-exit=1

$ uv run --no-project scripts/check_manifest_purity.py --root /tmp/opencode/mb2155-allows
manifest-purity: ok — 3 source file(s), no std::env
allows-exit=0
```

The reviewer's three examples now name their import line (`alias_root.rs:2`, `extern_root.rs:2`,
`chained_root.rs:3`), the aliased call names `alias_call.rs:2`, and at that round the corpus (25
fixtures then) ended with 25 hits and 1 suspect, exit 1.

### Round 3 evidence (fail-closed)

The fixtures are the self-test's own (`NESTED` + `REFUSED` + `TWO_FILE` written to
`/tmp/opencode/mb2155-denies`; `LEGAL` + `LEGAL_FILES` to `/tmp/opencode/mb2155-allows`): 29 deny
files (28 must refuse, `mod_a.rs` is clean), 4 allow files.

```console
$ uv run --no-project /tmp/opencode/mb2155-review/old_gate.py --root /tmp/opencode/mb2155-denies
manifest-purity: 3 std::env use(s) under crates/blastit-manifest/src/; ...   # only the 3 literal spellings
old-exit=1
# per fixture: glob_alias.rs old=0, comment_glob.rs old=0, mod_b.rs old=0, mod_a.rs old=0

$ uv run --no-project scripts/check_manifest_purity.py --root /tmp/opencode/mb2155-denies
crates/blastit-manifest/src/alias_overlong.rs:1: suspect `use` item: cannot classify within the token/depth budget in a file that mentions `env`
crates/blastit-manifest/src/comment_glob.rs:2: cannot classify a glob import (`use …::*`): it can inject `std::env` into the file
crates/blastit-manifest/src/glob_alias.rs:1: cannot classify a glob import (`use …::*`): it can inject `std::env` into the file
crates/blastit-manifest/src/mod_b.rs:1: cannot classify `crate::…`: its origin is outside this file
crates/blastit-manifest/src/mod_b.rs:2: cannot classify `e::…`: `e` is an alias whose origin is outside this file
manifest-purity: 25 std::env use(s) and 5 unclassifiable construct(s) under crates/blastit-manifest/src/; a Blastfile names variables and never reads them (R20/R24)
new-exit=1

$ uv run --no-project scripts/check_manifest_purity.py --root /tmp/opencode/mb2155-allows
manifest-purity: ok — 4 source file(s), no std::env
allows-exit=0
```

Per-fixture sweep: 28 of 29 deny files exit 1 here (25 of those exit 0 on `06874ae`; the three
literal spellings `alias_direct.rs`, `root_group.rs`, `string_literal.rs` exit 1 on both), and
`mod_a.rs` alone exits 0 — it names no `env`, which is the rule, not a gap.

**What is still not classified:** nothing that could reach `env` is passed. A non-std alias whose
origin crosses the file boundary (`use crate::x as y;` then `y::env`) is refused, never resolved;
a glob or a re-export is refused, not followed to its definition. Two constructs are outside the
gate's subject and remain undetected: the builtin `env!("NAME")` macro (no path), and a `std::env`
read hidden inside a macro body imported from elsewhere (no path either). Both are named in
Follow-ups.

## Follow-ups

- **`env!("NAME")` is a compile-time environment read the gate does not see** (the builtin macro,
  no path), and neither is a `std::env` read inside a macro body imported from another crate. Both
  are outside R24's path-shaped surface; file a small node if the owner wants macro spellings
  covered too.
- **A glob in a file that never mentions `env` still passes** (`use std::*;` alone). That is
  deliberate: strict mode is keyed on the file mentioning `env`, so the 16 clean sources gain no
  refusals. In a file that does mention `env`, the glob is refused (`glob_alias.rs`,
  `comment_glob.rs`).
- **`std::env::consts` fails by design** (see the rule above). If the covered crate ever needs
  target constants, that is a §17.2 change with its own review, not an allowlist here.
- **Shared-worktree measurement caveat:** another session ran `make check`/`make docs-test` in this
  worktree; a concurrent run of mine failed once with `could not execute process
  .../critical_learn_executor_blocks-… (never executed)` (a rebuilt `target/`), and the final
  serialized run above is green. The two `critical_guard` `[VIOLATION]` reports in its log are
  other sessions' edits to `critical_blastfile_parser.rs` / `critical_blastfile_print.rs` — this
  node's diff is only the two `scripts/` files.
