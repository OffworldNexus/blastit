# Summary

MB.3.4.4 hardens the `blastit-fastcgi` codec oracle in exactly two files:

- **`crates/blastit-fastcgi/src/cgi.rs`** — two appended unit tests in the
  existing `#[cfg(test)] mod tests` (no production line, no existing test
  touched):
  - `status_with_a_non_digit_byte_is_bad_head` kills the surviving
    `cgi.rs:221:66` `<` → `<=` mutant: `':'` is byte 58 and `58 - b'0' == 10`,
    so `Status: 2:0`, `Status: 20:` and `Status: :00` are
    `Err(CgiError::BadHead)`, while `Status: 200`, `Status: 200 OK` and
    `Status: 599` still parse.
  - `a_blank_line_only_head_is_200_text_html` pins the `head_end` early return
    (`head_end(b"\r\n") == Some(2)`, `head_end(b"\r\n\r\n") == Some(2)`) and the
    composed `head_end → parse_head` path for `b"\r\n\r\nbody"` → 200,
    `text/html`, zero fields, so a regression to the generic `windows(4)` search
    (a legal field-less head becoming a `502`) fails.
- **`crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs`** (new, protected)
  — 12 tests, every one asserting the denied path *and* its accepted twin:
  1. `the_four_proxy_control_names_are_refused_case_insensitively`
  2. `benign_x_accel_directives_are_dropped_not_faulted`
  3. `non_token_header_names_are_refused`
  4. `status_outside_200_599_is_bad_head`
  5. `status_with_a_non_digit_byte_is_bad_head`
  6. `duplicate_status_is_bad_head`
  7. `absent_status_is_200_text_html`
  8. `header_count_cap_is_exactly_100`
  9. `value_cap_is_exactly_8192`
  10. `head_total_cap_is_exactly_64_kib`
  11. `end_request_requires_complete_protocol_status`
  12. `overlong_record_header_and_truncated_pair_are_refused`

The record/param walls that lived only in-module (`EndRequest` bad
`protocolStatus`, `Header::decode` `CL + PL > 65 535`, a truncated name-value
pair) now have a protected oracle that also pins the accepted boundary
(`Complete` → `Ok`, `CL=65 535/PL=0` → `Ok`, the whole pair → `Ok`).

**Deviation from the node text (stated, not chased).** The NODE.md acceptance
line "*348 mutants tested … 8 missed, 322 caught, 18 unviable*" was written
against `0f441af`, before `MB.3.4.3` closed audit survivors #1, #2, #4, #5 and
#7 (`lib.rs` `Display`/`source`, `lockdown.rs` `Display`/`Params::Debug`,
`script_name_bytes`'s `?`). Arithmetic: `8 − 5 = 3` missed and
`322 + 5 = 327` caught at base `7af86b2`, and this node moves the last real
survivor (`cgi.rs:221:66` `<` → `<=`) into `caught.txt`, leaving the three known
equivalences. The measured result is exactly that: **3 missed / 327 caught / 18
unviable**, with the 3 being the two `|` → `^` equivalences and
`Params::is_empty -> true`.

# Evidence

## 1. New protected codec suite

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_codec --locked
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

## 2. Whole crate (unit + both protected suites + doc-test)

```console
$ cargo test -p blastit-fastcgi --locked
running 43 tests
test result: ok. 43 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 12 tests
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
running 30 tests
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

`43` = 41 base unit tests + the 2 appended; `30` = the unmodified
`critical_fastcgi_lockdown` suite; `1` = the `lib.rs` doc-test.

## 3. Protected-test guard

```console
$ uv run --no-project tools/critical_guard.py --base 7af86b2 --node MB.3.4.4
critical-guard: 7af86b2..working tree, 7 changed file(s)
node MB.3.4.4: paths ['crates/blastit-fastcgi/src/cgi.rs', 'crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs']
  no change to a protected critical test
```

(The new file is an addition, always allowed; `critical_fastcgi_lockdown.rs` is
byte-identical to `7af86b2`.)

## 4. Mutation campaign (private scratch)

```console
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-4-mut make mutants-rust PKG=blastit-fastcgi
mutants: scratch=/home/remy/.cache/blastit-mutants/mb34-4-mut fs=xfs
Found 348 mutants to test
ok       Unmutated baseline in 0s build + 0s test
MISSED   crates/blastit-fastcgi/src/lockdown.rs:185:9: replace Params::is_empty -> bool with true
MISSED   crates/blastit-fastcgi/src/lockdown.rs:581:32: replace | with ^ in hex_pair
MISSED   crates/blastit-fastcgi/src/param.rs:60:37: replace | with ^ in encode_len
348 mutants tested in 84s: 3 missed, 327 caught, 18 unviable
```

`make` returns non-zero here by design (any surviving mutant), so `Error 2` is
expected, not a harness failure.

Recomputed from the per-outcome files (`LESSONS`: cargo-mutants 27.1.0 writes
no `outcomes.txt`):

```console
$ wc -l mutants.out/{caught,missed,unviable}.txt
  327 mutants.out/caught.txt
    3 mutants.out/missed.txt
   18 mutants.out/unviable.txt
  348 total

$ grep "cgi.rs:221" mutants.out/caught.txt
crates/blastit-fastcgi/src/cgi.rs:221:66: replace < with == in parse_status
crates/blastit-fastcgi/src/cgi.rs:221:66: replace < with > in parse_status
crates/blastit-fastcgi/src/cgi.rs:221:66: replace < with <= in parse_status

$ awk -F'src/' '{print $2}' mutants.out/caught.txt | cut -d: -f1 | sort | uniq -c
     80 cgi.rs
      2 lib.rs
    185 lockdown.rs
     26 param.rs
     34 record.rs
```

The previously missed `cgi.rs:221:66` mutant is now caught; the only missed
entries left are the three already-classified equivalences (`lockdown.rs` 2,
`param.rs` 1), so no other file's caught count dropped.

## 5. Lint / format

```console
$ make rs-lint
PYTHONHOME="…" PYO3_PYTHON="…" LD_LIBRARY_PATH="…" cargo clippy --workspace --all-targets --locked -- -D warnings
    Checking blastit-fastcgi v0.2.0 (/home/remy/offworld-nexus/blastit/crates/blastit-fastcgi)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.29s

$ cargo fmt --all -- --check
(no output; exit 0)
```

## 6. Full unprivileged gate

```console
$ make check
… test result: ok. 12 passed; 0 failed …   # critical_fastcgi_codec
… test result: ok. 30 passed; 0 failed …   # critical_fastcgi_lockdown (unmodified)
57 fences: 30 executed, 27 accounted, 0 failed · 22.0s
plan OK
EXIT=0
```

(exit `0`: guard, critical-guard, rs-fmt, rs-lint, rs-doc, rs-test,
test-critical, py-lint, docs-unit, docs-check, docs-test and plan-validate all
passed.)

# Files

- `crates/blastit-fastcgi/src/cgi.rs` (modified): two appended tests only.
- `crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs` (new): 12 protected
  codec tests.

# Follow-ups

- None out of scope. `scripts/mutants-run.sh`/`Makefile` kick off a baseline and
  repo-root `mutants.out`; the campaign leaves `mutants.out/` in the workspace
  root (gitignored) — no tracked file changed.

# Review response

Reviewer VERDICT PASS with three MINORs, all cleared inside
`crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs` (no `src/`, no Cargo
file touched):

1. **Exact cap values, not `.is_ok()`.** `header_count_cap_is_exactly_100` now
   parses 100 fields and asserts both `headers.len() == MAX_HEADERS` and the
   full ordered `Vec<Field>` (dropped/reordered fields fail), with the 101-field
   `BadHead` refusal kept. `value_cap_is_exactly_8192` now asserts the parsed
   `X-Big` value's length is exactly `MAX_VALUE_BYTES` (and one field survives),
   with the 8 193 refusal kept.
2. **Redundant absence loop deleted.** The four `head.header(name) == None`
   assertions are gone; the exact `head.headers == vec![Field { "X-Test", "1" }]`
   above already proves absence, and the "the drop rule never swallows a control
   name" assertion is kept.
3. **Named literals.** `assert_eq!(MAX_HEADERS, 100)` and
   `assert_eq!(MAX_VALUE_BYTES, 8_192)` pin the §4.5.2 numbers so a drifting
   constant cannot stay green. `MAX_HEAD_BYTES` gets no extra literal: the
   total-cap buffer is a fixed 7·8 199 + 8 141 + 2 = 65 536 bytes, so
   `assert_eq!(at_cap.len(), MAX_HEAD_BYTES)` already fails on a drift (noted in
   a one-line comment).

Re-run evidence:

```console
$ cargo test -p blastit-fastcgi --test critical_fastcgi_codec --locked
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ cargo test -p blastit-fastcgi --locked
test result: ok. 43 passed …   # unit
test result: ok. 12 passed …   # critical_fastcgi_codec
test result: ok. 30 passed …   # critical_fastcgi_lockdown (unmodified)
test result: ok. 1 passed …    # doc-test

$ cargo fmt --all -- --check   # clean
$ make rs-lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.26s
```

