# Result MB.21.4.6

## Summary

All six Blastfile budgets moved, with every assertion, fixture, doc comment and doc row that
pinned an old number: `MAX_SOURCE_BYTES` 16 MiB, `MAX_TOKENS`/`MAX_TRIVIA` 4 194 304,
`MAX_PATTERN_BYTES` 4 096, `MAX_LIST_ITEMS` 4 096, and `app "<name>"` 256 `char`s — the last
newly **enforced** in `lower::app` (empty / over-cap / control byte) as `AppNameInvalid` at the
name's own `line:col`, with the cap interpolated from `MAX_APP_NAME_CHARS` and
`PatternTooLong`'s sentence interpolated from `pattern::MAX_PATTERN_BYTES` so neither can drift.
The token/trivia refusals now name real remedies (table/column grants, or a `trait` many views
`use`) instead of "split the file", and `MAX_PATTERN_BYTES`'s cap doc states that
`REGEX_SIZE_LIMIT` still bounds the compiled program (also in §8.1/§17.4/S10 and the reference).
Cap witnesses were restructured so no unit test allocates in proportion to the production budget:
`lexer::Caps` (production = the constants) lets the token/trivia boundary run on nine items, while
the real 4 194 304 boundary stays proven end-to-end by the protected
`critical_blastfile_lexer.rs` witness. Docs: design §4.5's six rows + refusal column + recomputed
headroom paragraph, and a new `## Limits` table in the reference linked from `## Errors`.

`make check` is green (2m01s), `make docs-check` exit 0, and `critical_guard --base a3ac338
--node MB.21.4.6` reports all six protected changes authorized (the five manifest suites plus
`crates/blastit/tests/critical_blastfile_fmt.rs`, which this node's `paths` now cover and whose
three numbers moved with the cap).

## Evidence

### 1. `grep -rn '1_048_576\|262_144\|MAX_PATTERN_BYTES: *256\|MAX_LIST_ITEMS: *256' crates/ docs/reference/blastfile.md docs/design/blastfile-dsl.md`

No Blastfile constant survives; `MAX_PATTERN_BYTES: *256` and `MAX_LIST_ITEMS: *256` print nothing
at all. The 11 remaining hits are budgets this node does not own, listed by eye as the node asks:

```
crates/blastit-scheduler/src/config.rs:45   FQ_INNER_MAX_DEFAULT = 262_144      # fair-queue inner flows
crates/blastit-scheduler/src/config.rs:428  assert_eq!(fair.inner_max, 262_144) # that budget's test
crates/blastit-sys/src/mem.rs:281,334,346,357,366,377,392 (7)                  # /proc/meminfo + cgroup fixtures
crates/blastit/src/config.rs:3542           assert_eq!(config.fq_inner_max, …) # the same fair-queue cap
```

### 2. `grep -rn 'split the file' crates/ docs/`

```
(no output)   # exit 1
```

### 3. `grep -rn 'at most 256 bytes\|1048576\|262144\|1 MiB' crates/ docs/reference/blastfile.md docs/design/blastfile-dsl.md`

Remaining hits are other budgets (this node does not own) plus the retired TOML validator's
static-glob sentence — none is a Blastfile budget row, and the by-eye list is:

```
crates/blastit-frame/src/lib.rs:17, src/limits.rs:7, blastit-http/src/config.rs:21   # 1 MiB frame default
crates/blastit-ingress/src/lib.rs:15,122                                             # 1 MiB credit windows
crates/blastit-scheduler/tests/flow.rs:38, blastit-view/tests/flow.rs:134,188        # body budgets
crates/blastit-sys/src/mem.rs:348,393                                                # cgroup v2 fixtures
crates/blastit-manifest/src/validate.rs:28:const AT_MOST_PATTERN_BYTES: &str = "at most 256 bytes";  # ImmutableGlob static globs (R5), not in paths
crates/blastit-manifest/tests/critical_static_manifest.rs:413, tests/static_mounts.rs:92             # that static-glob sentence
```
The `immutable` static-glob `≤ 256 bytes` prose at design §9.4 / S4 is untouched, as the node
directs (a different budget). No `1 MiB`/`1048576`/`262144`/`at most 256 bytes` remains in
`docs/reference/blastfile.md` or `docs/design/blastfile-dsl.md`.

### 4. `timeout 180 cargo test -p blastit-manifest --locked`

```
real 0m15.267s   exit 0   33 × "test result: ok", 0 failed
```
Before (base `a3ac338`, same command, measured by stashing only this node's paths):
`real 0m6.990s exit 0`. Both runs include the crate rebuild (~5–6 s); the growth is the three
rewritten 16 MiB `fmt` witnesses. Per-witness cost and peak child RSS (polled `VmHWM`):

| witness | wall | peak RSS |
|---|---|---|
| `critical_blastfile_lexer::the_token_cap_is_reachable_through_the_public_entry_point` | ~1.3 s | 142 MiB |
| `critical_blastfile_print::fmt_refuses_a_file_it_would_grow_past_the_cap` | ~2.6 s | 340 MiB |
| `blastfile_fmt::over_cap_is_refused` | ~2.6 s | 340 MiB |

All restructured cap witnesses:
- token/trivia boundary (`lexer.rs` `mod tests`): `Lexer::with_caps(&source, Caps{tokens:9,…})` —
  nine items instead of 4 194 304; at-cap accepted, one past refused with the production sentence;
  `Caps::PRODUCTION.{tokens,trivia}` is asserted equal to `MAX_TOKENS`/`MAX_TRIVIA`.
- hostile-at-the-size-cap (`lexer.rs`): token/trivia walls at lowered caps; the single-run cases use
  one buffer of exactly `MAX_SOURCE_BYTES`.
- real 4 194 304 boundary: unchanged protected witness through the public entry point
  (`"a ".repeat(4_194_305)`), as protected-test edits stay limited to numbers/strings.
- `fmt` growth: a generated 9 MiB source whose canonical form is 17 000 037 bytes > 16 MiB (the only
  input that can trip it, since growth is one indent per line).

### 5. The two previously-red targets (`make test-critical`, `make rs-test`)

```
$ timeout 1800 make test-critical
test result: ok. … (40 suites, 0 failed)
cargo test -p blastit --features python --test critical_blastfile_fmt   → ok (5 passed, 0 failed)
python -m unittest discover -s python/tests/critical
Ran 50 tests in 4.754s   OK
make test-critical exit 0        real 0m38.515s

$ timeout 3600 make rs-test
121 × "test result: ok", 0 failed; all doctests ran
make rs-test exit 0              real 1m28.835s
```

### 6. `make check`

```
$ timeout 5400 make check
… doc-tone: ok — 28 user-facing page(s), no banned phrase
… manifest-purity: ok — 17 real source file(s); self-test ok
… 49 fences: 28 executed, 21 accounted, 0 failed · 26.1s
plan OK
exit 0                           real 2m0.867s
```

### 7. `make docs-check`

```
$ timeout 600 make docs-check
uvx --offline zensical build --strict
No issues found
Build finished in 0.17s          exit 0
```

### 8. `uv run --no-project tools/critical_guard.py --base a3ac338 --node MB.21.4.6`

```
critical-guard: a3ac338..working tree, 26 changed file(s)
  M crates/blastit-manifest/tests/critical_blastfile_{errors,lexer,parser,patterns,print}.rs  [authorized by crates/blastit-manifest/tests/**]
  M crates/blastit/tests/critical_blastfile_fmt.rs                                  [authorized by crates/blastit/tests/critical_blastfile_fmt.rs]
critical-guard: OK: all 6 protected change(s) are authorized by node(s) MB.21.4.6
exit 0
```

### 9. `wc -l docs/blastfile.md`

```
98 docs/blastfile.md
```

### 10. The `fmt` cap witness in the critical tier

`crates/blastit/tests/critical_blastfile_fmt.rs` now builds the same generated source at the new
cap (`0..1_000_000`, `1_048_576` → `16_777_216`, `"1048576"` → `"16777216"`); the message follows
the constant:

```
$ timeout 600 cargo test -p blastit --features python --locked --test critical_blastfile_fmt \
    -- a_file_it_would_grow_past_the_cap_is_refused
test result: ok. 1 passed; 0 failed; … finished in 2.69s     exit 0 (7.9 s incl. cargo)
```

The refusal it asserts, produced by the built CLI on that exact fixture:

```
$ wc -c Blastfile            # view v "/x" { postgres { 1 000 000 × "delete t" } }
9000029 Blastfile
$ target/debug/blastit fmt ; echo exit=$?
./Blastfile: the file is 17000037 bytes; Blastfile is limited to 16777216 bytes — split the grants into a `trait`, or delete a generated block
exit=2
```

## Files

- `crates/blastit-manifest/src/blastfile/mod.rs` — five budgets + `MAX_APP_NAME_CHARS`, `pub(crate) use pattern::MAX_PATTERN_BYTES`, reader doc 16 MiB
- `crates/blastit-manifest/src/blastfile/lexer.rs` — `Caps`/`with_caps`, cap witnesses restructured
- `crates/blastit-manifest/src/blastfile/pattern.rs` — 4 KiB cap + `REGEX_SIZE_LIMIT` doc, tests parameterised
- `crates/blastit-manifest/src/blastfile/lower.rs` — `app` validation (empty / 256 `char`s / control byte)
- `crates/blastit-manifest/src/blastfile/parser.rs` — docs + list/pattern/size assertions parameterised
- `crates/blastit-manifest/src/blastfile/print.rs` — fmt cap witness rebuilt at 16 MiB
- `crates/blastit-manifest/src/error.rs` — `TooLarge`/tokens/trivia docs + new sentences, `PatternTooLong` parameterised, new `AppNameInvalid`
- `crates/blastit-manifest/src/lib.rs` — reader doc
- `crates/blastit-manifest/tests/blastfile_entrypoint.rs` — `the_app_name_is_carried_and_bounded` (allowed + empty/over-cap/control)
- `crates/blastit-manifest/tests/{blastfile_fmt,blastfile_patterns}.rs` — caps and messages
- `crates/blastit-manifest/tests/critical_blastfile_{errors,lexer,parser,patterns,print}.rs` — numbers and expected strings only, plus one doc-comment number
- `crates/blastit/tests/critical_blastfile_fmt.rs` — the same numbers-only fix (`0..1_000_000`, `16_777_216`, `"16777216"`) plus its witness note
- `docs/reference/blastfile.md` — new `## Limits` (six rows) + link from `## Errors`
- `docs/design/blastfile-dsl.md` — §4.3 app row, §4.5 table/refusals/headroom, §8.1, §8.3, §17.4, R1, S10, the cap-bypass row

## Follow-ups

1. `crates/blastit/tests/critical_blastfile_fmt.rs` (created by `MB.21.4.1`) needed the same three
   numbers; this node's `paths` were widened by the lead for it, so `critical_guard` now authorizes it.
   The witness is ~2.7 s and ~350 MiB inside the critical tier (9 MiB source, 17 MiB canonical form);
   if that budget is ever tightened, `MB.21.4.2` (which also owns `crates/blastit/tests/**`) can
   restructure it — a comment wall grows ~5x per byte or a lowered cap would be cheaper.
2. `crates/blastit-manifest/src/validate.rs:28` (`AT_MOST_PATTERN_BYTES = "at most 256 bytes"`) and
   its two tests (`critical_static_manifest.rs:413`, `static_mounts.rs:92`) are the **retired TOML**
   validator's `immutable` static-glob cap (R5), a different budget, and `validate.rs` is not in this
   node's paths. It is what keeps acceptance grep 3 non-empty inside `blastit-manifest`.
3. Acceptance item 1 as written ("all three greps print nothing over `crates/`") cannot hold for any
   implementation: `1 MiB` / `1048576` / `262 144` are also the frame, ingress-credit, cgroup-fixture
   and fair-queue literals in `blastit-frame`, `blastit-http`, `blastit-ingress`, `blastit-sys`,
   `blastit-scheduler` and `blastit-view`, none of which this node owns. The by-eye list in Evidence
   1/3 confirms every remaining hit is one of those or the static-glob string; scoping the grep to
   `crates/blastit-manifest/` next time would make it empty.
4. `docs/knowledge/LESSONS.md` is outside `paths`, so this gotcha is unfiled: `ulimit -v` applied to
   a whole `cargo test` also caps `rust-lld`, which then dies with `LLVM ERROR: out of memory`
   instead of measuring RSS — build first, then bound or poll the test binary itself.
5. `docs/research/2026-11-16-zed-blastfile-lsp.md` (untracked, another node's) still quotes
   `MAX_SOURCE_BYTES = 1_048_576` and `MAX_TOKENS = 262_144`; it is not in this node's paths.
