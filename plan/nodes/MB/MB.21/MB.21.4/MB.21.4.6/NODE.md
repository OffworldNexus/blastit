---
id: MB.21.4.6
title: "Raise the six Blastfile budgets (16 MiB source, 4M tokens/trivia, 4 KiB patterns, 4K list items, 256-char app name) and fix the token-cap sentence"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.21.4.1]
paths:
  - "crates/blastit-manifest/src/blastfile/**"
  - "crates/blastit-manifest/src/error.rs"
  - "crates/blastit-manifest/src/lib.rs"
  - "crates/blastit-manifest/tests/**"
  - docs/reference/blastfile.md
  - "docs/design/blastfile-dsl.md"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T09:11"
updated: "2026-11-16T09:11"
---

# Goal

**Owner directive (2026-11-16): "let's allow something comfortable."** The Blastfile budgets were set
for a hand-written file and refuse a generated one. Raise all six ceilings, and fix the one refusal
sentence that tells the reader to do something impossible.

| Constant / cap | Today | New |
|---|---|---|
| `MAX_SOURCE_BYTES` | `1_048_576` (1 MiB) | **`16_777_216` (16 MiB)** |
| `MAX_TOKENS` | `262_144` | **`4_194_304`** |
| `MAX_TRIVIA` | `262_144` | **`4_194_304`** |
| `MAX_PATTERN_BYTES` | `256` | **`4_096` (4 KiB)** |
| `MAX_LIST_ITEMS` | `256` | **`4_096`** |
| `app "<name>"` max chars | `64` | **`256`** |

1. Change each number **where grep finds it** — do not guess a file. Measured today:
   `MAX_SOURCE_BYTES` `blastfile/mod.rs:175`, `MAX_TOKENS` `mod.rs:178` (enforced `lexer.rs:158`),
   `MAX_TRIVIA` `mod.rs:181` (enforced `lexer.rs:175`), `MAX_LIST_ITEMS` `mod.rs:189` (enforced
   `parser.rs:715`), `MAX_PATTERN_BYTES` `blastfile/pattern.rs:44` (enforced `pattern.rs:314`). Then
   every assertion, fixture, doc comment and table that pins an old number must move with it:
   `error.rs` (the `TooLarge`/`TooManyTokens`/`TooManyTrivia` doc comments say 1 MiB / 262144, and
   `PatternTooLong`'s `#[error("… at most 256 bytes")]` at `error.rs:1312` **hard-codes** 256 —
   parameterise it with `pattern::MAX_PATTERN_BYTES` so it can never drift again), `lib.rs:96`,
   `blastfile/parser.rs:39`, `blastfile/print.rs:789-792`, the in-crate `mod tests` in `lexer.rs`
   (`:752-795` token/trivia caps and their exact messages, `:809-834` the source-cap corpus),
   `parser.rs` (`:1797/:1803` pattern, `:2306-2311` list items, `:2353-2361` source),
   `pattern.rs` (`:586-616`, `:848`), and the suites
   `tests/critical_blastfile_lexer.rs` (`:11,:25,:29,:180,:190,:199,:203`),
   `tests/critical_blastfile_patterns.rs` (`:95-103`), `tests/blastfile_patterns.rs` (`:172-190`),
   `tests/blastfile_fmt.rs` (`:118-121`), `tests/critical_blastfile_print.rs` (`:486-490`), plus
   whatever `tests/blastfile_errors.rs` / `tests/critical_blastfile_errors.rs` /
   `tests/critical_blastfile_parser.rs` pin. `tests/critical_blastfile_parser.rs:61-67` uses
   `262_144` as a *corpus size*, not as the cap: keep its intent (a several-hundred-KiB file of views
   lowers) and use a non-colliding literal so the sweep is clean — no assertion is deleted or
   weakened. Edits inside `tests/critical_*` stay limited to the numbers and their expected strings.
2. `MAX_PATTERN_BYTES` becomes 4 KiB, but **`REGEX_SIZE_LIMIT` (`routing.rs:12`, `1 << 20`) still
   bounds the compiled program**. Say exactly that in `pattern.rs`'s cap doc comment (`:23-44`) and in
   the reference and design rows, so a bigger *source* pattern never reads as permission for a bigger
   *compiled* regex. Do **not** touch `REGEX_SIZE_LIMIT` or `routing.rs` here.
3. Docs must carry the same six numbers: `docs/reference/blastfile.md` (today it states the caps only
   qualitatively — no row names a number; add one compact `## Limits` table with the six rows,
   identical values and refusal shapes to design §4.5, and link it from `## Errors`) and
   `docs/design/blastfile-dsl.md` §4.5 (`:221-236`, including the "1 MiB is ~40x …" paragraph, which
   must be recomputed for 16 MiB) plus every R-requirement/§ row that cites an old number:
   `:621`, `:653`, `:1535`, `:1668` (R1's "1 MiB input"), `:1794`, `:2000`, and `:185-191` for the
   `app` cap. **Do not** touch `:817-818` / `:1788`: the 32-patterns/256-bytes caps there are
   `ImmutableGlob`'s static-glob cap, a different budget.
4. **Fix the token/trivia refusal sentence.** `error.rs:816` and `:828` (and the copies at
   `lexer.rs:758/:783`, `tests/critical_blastfile_lexer.rs:190`) say `… — split the file`, which is
   impossible advice: a Blastfile is one file with no `include`. Keep the position-less `<path>:`
   prefix (the caller prefixes once) and keep it one sentence; replace the advice with the real
   remedies — the file is too large, so replace repeated `query '…'` lines with table/column grants
   (`select`/`insert`/`update`/`delete`) or fold the shared grants into a `trait` that many views
   `use`. The `MAX_SOURCE_BYTES` sentence already says "split the grants into a `trait`" — make
   tokens/trivia match its voice. Update design §4.5's three refusal cells to the new sentences and
   assert the new text where it is pinned.
5. `app "<name>"`: the 64-`char` ceiling lives only in prose today (`docs/design/blastfile-dsl.md`
   §4.3's `Manifest::name` row) — grep finds no enforcement; `lower::app()` just stores the string.
   Raise the number to 256 and make the row true: validate non-empty, ≤ 256 `char`s, no control byte
   in `lower::app()`, with a `Blastfile:<line>:<col>:` refusal and one assertion. If grep does find an
   enforcement, change only its constant.
6. Scope: `crates/blastit-manifest/src/**` (blastfile, `error.rs`, `lib.rs`) and
   `crates/blastit-manifest/tests/**` only — no `crates/blastit/`, no `Cargo.toml`/`Cargo.lock`, no
   `routing.rs`. This leaf runs **after `MB.21.4.1` commits and before `MB.21.4.2` starts**
   (`MB.21.4.2` gains this node as a dependency) because it edits `error.rs`, `lib.rs` and the
   manifest test suites that `MB.21.4.2` also owns.

# Context

- The DX cost is real: `--learn` writes one `query '…'` line per distinct statement, so a
  six-hundred-statement app crosses 262 144 tokens long before it is a bad Blastfile, and the refusal
  told its author to "split the file" when there is no such thing.
- The budget table is "one table, referenced by every section" (`docs/design/blastfile-dsl.md` §4.5);
  the reference is the page a developer actually reads, so it needs the numbers too.
- A 16 MiB / 4 194 304-token ceiling still bounds a hostile boot: `blastfile::read` refuses a
  non-regular file and reads at most `MAX_SOURCE_BYTES + 1`, so the worst case is one 16 MiB
  allocation and a bounded token stream — note that in §4.5's paragraph, replacing the "~40x"
  reasoning with the new headroom against the largest 0.2.0 fixture.
- Test cost: the token/trivia cap witnesses now build ~4.2M tokens, and the source-cap witness an
  8–16 MiB string. Keep every boundary assertion, but if `cargo test -p blastit-manifest` grows past
  ~60 s or ~1.5 GiB RSS, restructure the witness (a counted document, or a test-only injected limit)
  rather than shrinking a constant; record the before/after timing in `RESULT.md`.
- Read first: `crates/blastit-manifest/src/blastfile/mod.rs` (`:1-30`, `:170-230`),
  `docs/design/blastfile-dsl.md` §4.5 and §8, `docs/reference/blastfile.md`,
  `docs/knowledge/LESSONS.md`. Load skills `rust-standards` and `testing`.

# Acceptance

```console
$ grep -rn '1_048_576\|262_144\|MAX_PATTERN_BYTES: *256\|MAX_LIST_ITEMS: *256' crates/ docs/reference/blastfile.md docs/design/blastfile-dsl.md
$ grep -rn 'split the file' crates/ docs/
$ grep -rn 'at most 256 bytes\|1048576\|262144\|1 MiB' crates/ docs/reference/blastfile.md docs/design/blastfile-dsl.md
$ timeout 180 cargo test -p blastit-manifest --locked
$ make check
$ make docs-check
$ uv run --no-project tools/critical_guard.py --base HEAD --node MB.21.4.6
$ wc -l docs/blastfile.md
```

1. All three `grep`s print nothing: no old constant or old sentence survives in `crates/`, the
   reference or the design (§817-818/§1788's static-glob 256 is prose about a different cap and reads
   `≤ 256 bytes` / `32/256`, so confirm by eye that only those remain).
2. `cargo test -p blastit-manifest --locked` exits 0 inside 180 s, and the cap suites still assert
   each boundary (at cap = accepted, one past = refused with the exact new message).
3. `make check` and `make docs-check` exit 0; `critical_guard.py --base HEAD --node MB.21.4.6`
   reports no unauthorized critical-test change.
4. `wc -l docs/blastfile.md` still prints `98` — the guide (a different page, not in `paths`) does not
   move; only the reference and the design are edited.
5. `docs/design/blastfile-dsl.md` §4.5's six rows, its refusal column and the headroom paragraph match
   the constants in code, and the `REGEX_SIZE_LIMIT` sentence is present in `pattern.rs` and both docs.
6. `RESULT.md` records the before/after `cargo test -p blastit-manifest` wall time and the shape of
   every restructured cap witness.

# Notes

- Ordering is the point: this leaf commits between `MB.21.4.1` and `MB.21.4.2`, so `MB.21.4.2` starts
  from a tree whose budgets, `error.rs` messages and manifest suites already agree with the docs.
- Do not widen `REGEX_SIZE_LIMIT`, `MAX_BLOCK_DEPTH` or `MAX_PATTERNS` here; the directive names six
  numbers and nothing else.
- If a fixture outside `paths` pins an old number, record it in `RESULT.md` and name the sibling that
  owns it (`MB.21.4.4` for `e2e/`, `MB.21.4.3` for the Python package); do not widen silently.
