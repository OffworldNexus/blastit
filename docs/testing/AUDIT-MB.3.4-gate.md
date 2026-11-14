# AUDIT MB.3.4-gate — `blastit-fastcgi` epic gate (delta re-audit)

- **Date:** 2026-11-14
- **Node / gate:** `plan/nodes/MB/MB.3/MB.3.4/NODE.md` (epic, 5 children `done`:
  `MB.3.4.1`–`MB.3.4.5`).
- **Scope:** `crates/blastit-fastcgi/**` — `src/{lib,record,param,cgi,lockdown}.rs`,
  `tests/critical_fastcgi_lockdown.rs` (30 tests), `tests/critical_fastcgi_codec.rs`
  (12 tests). Read-only for code.
- **Base / tip:** the prior audit read `0f441af`; this audit reads the delta
  `7af86b2` (`MB.3.4.3`) → `612ecad` (`MB.3.4.4`) → `cad3b72` (`MB.3.4.5`) → tip
  `2ad094b`. The working tree also carries the in-flight `MB.3.5.1` leaf's edits under
  `crates/blastit-manifest/**` (6 files, +1010/−15) and `plan/` bookkeeping. That does
  not touch this audit: `blastit-fastcgi` has **zero normal dependencies**, so neither
  the baseline nor the mutation campaign compiles or links the manifest crate.
- **Requirements:** `docs/design/php-fastcgi.md` §4.5.1–§4.5.2 (codec vs scheduler
  client; records, ids, lifetimes), §4.6.1–§4.6.3 (param contract), §4.7 (binary refusal
  matrix), §7.1 (tiers), §7.2 (`R1`–`R20` → enforcing module → suite);
  `docs/decisions/ADR-0008-*.md` (decision 4/§4 text), `docs/knowledge/LESSONS.md`
  #25/#26/#29 (`outcomes.txt`, private `MUTANTS_TMP`, `/tmp` tmpfs); skills `testing`
  §1–§4, `security-invariants` §2 (FastCGI/CGI param & header smuggling) and §5 (never
  leak, never panic), `dx-first`.
- **Not audited (unchanged):** the §4.7 rows another crate owns (2, 4, 15, 16, 19–22,
  24, 25 — `blastit-http`, `blastit-scheduler`, `blastit-manifest`), the
  `critical_fastcgi_dispatch`/`parent_fd`/`pool`/`learn` suites (`MB.3.6`/`MB.9` still
  `ready`, files absent), `e2e/php2shell.py` (`MB.3.7`), and the five in-module suites'
  tests that this delta did not touch.

## Verdict

**PASS on tier, teeth and the mutation bar. No real mutation gap, no public function or
type without an oracle, and no test to delete.** 331 of 334 viable mutants are killed
(**99.1 %**, bar ≥ 75 %); the 3 survivors are the two known `|`→`^` equivalences and
`Params::is_empty`. This **confirms** the batch's advertised state exactly
(3 missed / 331 caught / 18 unviable, 352 total) — I correct nothing.

One **test-only** task, `MB.3.4.6`, is filed for two branch-inspection residuals that
`cargo-mutants` never generates a mutant for and that no current test would catch if
regressed: `head_end` accepting an end offset *equal* to `MAX_HEAD_BYTES`
(§4.5.2's "≤ 64 KiB total") and the §4.6.2 "starts with a digit" header-name drop. No
BLOCKER. The three `MB.3.4.3`–`MB.3.4.5` tasks did what they claimed.

## Method (one read-only pass)

1. Read the epic `NODE.md`, all five child `RESULT.md`/`LOG.md`, the design sections
   above, the ADR's decision text, and LESSONS #25/#26/#29; read every line of the five
   source files and both protected suites.
2. Ran the baseline and the epic's two purity checks (§"Baseline").
3. `git diff --stat 0f441af..HEAD` and `--numstat` on the protected files to prove the
   delta is pure addition to `critical_fastcgi_lockdown.rs` (187/0) plus the new
   `critical_fastcgi_codec.rs` (290/0), and that `cgi.rs` gained only tests (35/0).
4. Re-ran `tools/critical_guard.py --base 0f441af --node MB.3.4`: the new file is
   "added (always allowed)", the lockdown edit is authorized by the epic's own path, no
   deletion.
5. One `cargo-mutants` campaign from this session in a **private** scratch
   (`MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-gate-mut`, disk-backed), then
   recomputed the ratio from `mutants.out/{caught,missed,timeout,unviable}.txt`
   (LESSONS #25: 27.1.0 writes no `outcomes.txt`).
6. Branch-inspected the paths cargo-mutants does not mutate: `head_end`'s
   `(end <= MAX_HEAD_BYTES)`, `is_representable_header`'s first-byte arm,
   `Params::is_empty`, and `FCGI_KEEP_CONN`'s only reference.

## Baseline

```console
$ CARGO_NET_OFFLINE=true cargo test -p blastit-fastcgi --locked
     Running unittests src/lib.rs (…/blastit_fastcgi-b0ea3a55ce11f1db)
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
     Running tests/critical_fastcgi_codec.rs (…/critical_fastcgi_codec-142dcb4462d745b0)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
     Running tests/critical_fastcgi_lockdown.rs (…/critical_fastcgi_lockdown-0264b7802ae706e8)
test result: ok. 30 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.02s
   Doc-tests blastit_fastcgi
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

$ test -z "$(cargo tree -p blastit-fastcgi -e normal --prefix none | tail -n +2)" && echo "zero dependencies"
zero dependencies
$ ! grep -rn 'tokio\|UnixStream\|TcpStream\|std::fs\|Command::new' crates/blastit-fastcgi/src && echo "pure codec, no I/O"
pure codec, no I/O
```

**44 / 12 / 30 / 1** is exactly the dispatched count. 44 unit tests = 41 base + 2
`cgi.rs` (`MB.3.4.4`) + 1 `lockdown.rs` (`MB.3.4.5`). `0 ignored`, `0 failed`, no
`#[ignore]`/`should_panic`/`needs_root` anywhere in the crate (`grep` count 0).

## Delta since the previous audit — what moved

| State | Mutants | Caught | Missed | Unviable | Killed / viable |
|---|---|---|---|---|---|
| Prior audit (`0f441af`) | 348 | 321 | 9 | 18 | 321/330 = 97.3 % |
| **This gate (`2ad094b`)** | **352** | **331** | **3** | **18** | **331/334 = 99.1 %** |

- `MB.3.4.3` moved five tasked survivors into `caught.txt` (`lib.rs:87` `Display`,
  `lib.rs:96` `source`, `lockdown.rs:109` `Display`, `lockdown.rs:133` `Params::Debug`,
  `lockdown.rs:515` `script_name_bytes`), closed MINOR (a) (the 65 535 **total** cap now
  fires through the public `build`) and pinned all nine hop-by-hop names.
- `MB.3.4.4` moved `cgi.rs:221` `<`→`<=` into `caught.txt`, pinned `head_end`'s
  field-less-head early return, and promoted the CGI/record/param walls into the new
  protected `critical_fastcgi_codec.rs`.
- `MB.3.4.5` added the `CONTENT_TYPE` cap to the public `resolve` and closed MINOR (b);
  its four new mutants (the `resolve` match arms, the `build` drop guard) are all caught.
- Survivors 9 → 3: six killed, three remain (all non-gaps, §"Survivors"). Mutant total
  348 → 352 because the rewritten `resolve`/`build` code adds four mutation sites.

## Inventory & classification

| Path | Tests | Tier | Class | Verdict |
|---|---|---|---|---|
| `tests/critical_fastcgi_lockdown.rs` | 30 | critical (protected) | **critical** ×30 | one test per owned §4.7 row + the exact-stream twin + the five `MB.3.4.3` hardenings; right tier |
| `tests/critical_fastcgi_codec.rs` (new) | 12 | critical (protected) | **critical** ×12 | the CGI head + record/param walls promoted out of the in-module tier; every test pins a denied path and its accepted twin |
| `src/cgi.rs` inline (`MB.3.4.4` delta) | 2 | standard (in-module) | **useful** ×2 | `status_with_a_non_digit_byte_is_bad_head`, `a_blank_line_only_head_is_200_text_html`; both duplicate the protected codec oracle on purpose (fail-fast unit tier), not fluff |
| `src/lockdown.rs` inline (`MB.3.4.5` delta) | 1 | standard (in-module) | **useful** ×1 | `resolve_applies_the_value_cap_to_content_type` is the only direct oracle for the public `resolve` boundary; the protected suite exercises the same rule through `build` |
| `src/{record,param,lib}.rs` inline / doc-test | 14 / 7 / 1 | standard / docs | **useful** | unchanged by this delta; `§7.1` assigns record/param to the unit tier |

**No `fluff` and no `misplaced` test in the delta.** Nothing asserts a constructor, a
getter or a serde default in isolation. The duplication between the in-module `cgi`
tests and `critical_fastcgi_codec.rs` is **promotion, not fluff**: the same oracle now
also runs in the protected tier, and the in-module copy still fails fast in the unit
tier. No test is skipped or weakened. `critical_fastcgi_codec.rs` is not named in
design §7.1's critical-file list; it is an additive promotion of §7.2's codec half (see
"Notes for the epic gate").

## Tier & teeth (anti-fluff, `testing` §3)

The question for each suite: *would it still pass if the wall returned `Ok`
unconditionally?*

| File | Passes under the permissive answer | Teeth |
|---|---|---|
| `critical_fastcgi_lockdown.rs` | Nothing as a file. Individually, `nul_in_header_value_is_malformed`, `malformed_request_facts_are_refused` and `malformed_pinned_paths_are_refused` assert only denials; `documented_index_request_serialises_to_the_exact_param_stream` and `error_display_and_source_are_not_empty` assert only positives. | An `Ok`-always builder fails `documented_index…` (it pins **every** name/value and the full encoded byte string, so a dropped, reordered or extra pair fails) and every refusal test. `assert_never_emitted` asserts both absence from the stream **and** `resolve(...) == Err(UnknownParam)`; `php_admin_value_…` also refuses the `PHP_ADMIN_*` prefix; `query_string_dash_d_…` pins the `-d` needle to exactly `[REQUEST_URI, QUERY_STRING]`; `the_total_param_byte_budget_…` measures `pair == 8 205 < 65 531` and `seven < 65 535 < seven + pair`, so only the total branch can refuse; `every_hop_by_hop_name_is_dropped` pins all nine folded names and the remaining 16-param count; `params_debug_…` asserts the names are present **and** neither `session` nor `secret` is. The two single-sided families are paired inside the same file, so an always-`Ok` or always-`Err` wall cannot pass the file. |
| `critical_fastcgi_codec.rs` | Nothing as a file. `absent_status_is_200_text_html` is a positive-only default/`head_end` twin. | Every refusal test carries its accepted twin in the same function: the four control names vs `X-Sendfile-Extra`/`X-Rewrite-URL2` (exact-name refusal) and `X-Accel-Buffering` (drop, not fault); non-token names vs the same name once the NUL/space is gone reaching `ForbiddenProxyHeader`; `Status: 100/199/600/099` vs `200`/`599`; `2:0`/`20:`/`:00` vs `200 OK`/`204 No Content`; duplicate `Status` vs one `Status` consumed and not forwarded; 100 vs 101 fields with the full ordered `Vec<Field>`; 8 192 vs 8 193 bytes with the parsed length; 65 536 vs 65 537 head bytes; `protocolStatus` ∈ {1,2,3,0xff} vs `Complete`; `CL+PL = 65 536` vs `65 535` and a cut pair vs the whole pair. `MAX_HEADERS`, `MAX_VALUE_BYTES` and `head_end`'s early return are pinned by literal/behaviour assertions. |

Teeth are also proven mechanically: every viable mutant in `cgi.rs`, `record.rs`,
`lib.rs` and `param.rs` is killed, and 189 of 191 in `lockdown.rs` (§"Mutation
evidence").

## Public-surface sweep

Every `pub` **function and type** in the five modules is reachable from a test, and
cargo-mutants kills at least one viable mutant in every one of them (the mechanical
proof; a `fn → Ok(...)`/`true`/`false`/`None` replacement that no test catches would
appear as a survivor). Grouped: `lib.rs` 4 consts + `Error` (+ `Display`/`source`/two
`From`, all asserted); `cgi.rs` 5 consts + `Field`/`Head`/`header` + `head_end`/
`parse_head` + `CgiError`; `param.rs` `Pair` + `encode_pair` + `Decoder`/`new`/
`next_pair` + `decode_all`; `record.rs` 11 consts + `RecordType`(+`code`,`from_code`) +
`Header`(+4) + `Record::decode` + `encode` + `BeginRequest`(+4) + `EndRequest`(+2) +
`RecordError`; `lockdown.rs` 2 consts + `FastCgiPaths` + `Scheme` + `RequestFacts`
(all 10 fields driven) + `LockdownError` (all four variants + phrases) + `Params`(9
methods; `encode_to` through `encode`) + `resolve` + `build`.

Two public items have **no effective oracle**, and neither is filed as a task:

1. **`record::FCGI_KEEP_CONN` — `src/record.rs:35`.** Its only reference outside docs is
   `record::tests::begin_request_is_the_pinned_responder_body`'s
   `flags & FCGI_KEEP_CONN == 0` (`record.rs:515`), which is vacuously true for **any**
   constant because `BeginRequest::responder()` hardcodes `flags: 0`; the constant has no
   production read. Changing its value changes no observable byte, so this is a
   documentation-only item, not a behaviour gap.
2. **`lockdown::Params::is_empty` — `src/lockdown.rs:184`** (the surviving
   `-> true` mutant). Its `false` direction has no oracle, but it has **no production
   caller** (the only call is `lockdown::tests::insert_refuses_reserved_names_even_inside_the_crate`,
   asserting the `true` direction); it exists to satisfy `clippy::len_without_is_empty`.
   Getter-only, deliberately untested — no task (§"Survivors").

No public **const** that gates real bytes is unpinned: `MAX_PARAMS`,
`MAX_PARAM_VALUE_BYTES`, `MAX_PARAMS_BYTES`, `MAX_PARAM_PAIR_BYTES`, `FCGI_MAX_CONTENT`,
`HEADER_LEN`, `MAX_HEAD_BYTES`, `MAX_HEADERS`, `MAX_VALUE_BYTES`, `DEFAULT_STATUS`,
`DEFAULT_CONTENT_TYPE` and the seven record-type codes are each asserted at their
boundary or by literal.

## Requirement → test map (rows that gained protection since `0f441af`)

| Req / rule | Enforcing module | Test(s) | Status |
|---|---|---|---|
| **`R19` encoded ≤ 65 535 (total)** | `lockdown::Params::insert` via `build` | `the_total_param_byte_budget_is_enforced_through_build` | **protected** (was `standard`) — MINOR (a) closed |
| §4.6.2 hop-by-hop names (all 9) dropped before `HTTP_*` | `lockdown::is_hop_by_hop` | `every_hop_by_hop_name_is_dropped` | **protected** (was 2 of 9) |
| §4.6.2 `SCRIPT_NAME` is `?`-free | `lockdown::script_name_bytes` | `script_name_with_a_query_is_malformed` | **protected** (was `gap`) |
| `R18` no param **value** in `Params` `Debug` | `impl Debug for Params` | `params_debug_prints_names_never_values` | **protected** (was `gap`) |
| §4.5.1 / §4.7 `Status:` is three **digits** | `cgi::parse_status` | `status_with_a_non_digit_byte_is_bad_head` (unit + protected) | **protected** (was `gap`) |
| §4.5.2 field-less head; "first `\r\n\r\n`" / leading blank line | `cgi::head_end` → `parse_head` | `a_blank_line_only_head_is_200_text_html`, `absent_status_is_200_text_html` | **protected** (was `gap`) |
| `R8` row 24 codec half: four proxy-control names, any other `X-Accel-*` dropped, non-token name, `Status` range, 100/8 192/64 KiB caps, `CL`/`TE` duplicates | `cgi` | `critical_fastcgi_codec.rs` (12 tests) | **protected** until `MB.3.6.2.2`'s `critical_fastcgi_dispatch` lands |
| `R9` record: `CL+PL ≤ 65 535`, `END_REQUEST` `protocolStatus`, truncated name-value pair | `record`, `param` | `overlong_record_header_and_truncated_pair_are_refused`, `end_request_requires_complete_protocol_status` | **protected** (in-module unit tier retained per §7.1) |
| §4.6.2 / `resolve`'s 8 192-byte `CONTENT_TYPE` rule | `lockdown::resolve`, `build` | `over_long_content_type_is_dropped_not_refused` (build, protected) + `resolve_applies_the_value_cap_to_content_type` (unit) | protected for `build`, unit for the direct `resolve` boundary — MINOR (b) closed |
| `lib` `Error` surface: `Display` + `source()` | `impl Display/Error for Error` | `error_display_and_source_are_not_empty` | **protected** (was `gap`) |
| `R7`/`R8` rows 1,3,5–14,17,18,23; `R8` 15/16 lockdown halves; name alphabet/unrepresentable name; NUL/CR/LF; 8 192 drop; collision first-wins; 128 count; one decode | unchanged | unchanged (25 base tests) | protected |

Still open **by design**, not by this crate: the dispatch-level `502` owner for `R8` row
24 (`critical_fastcgi_dispatch`, `MB.3.6.2.2`), and rows 2/4/15/16/19–22/25 owned by
`blastit-http`/`blastit-scheduler`. Two residuals inside this crate's own protected
suites are filed as `MB.3.4.6` (§"Branch-inspection residuals").

## Mutation evidence

```console
$ MUTANTS_TMP=$HOME/.cache/blastit-mutants/mb34-gate-mut CARGO_NET_OFFLINE=true \
    make mutants-rust PKG=blastit-fastcgi
mutants: scratch=/home/remy/.cache/blastit-mutants/mb34-gate-mut fs=xfs
mutants: command=cargo mutants -p blastit-fastcgi -j 2 --timeout 90
mutants: memory cap: cgroup v2 via systemd-run --user --scope (MemoryMax=12G, MemorySwapMax=0)
Found 352 mutants to test
ok       Unmutated baseline in 0s build + 0s test
MISSED   crates/blastit-fastcgi/src/lockdown.rs:185:9: replace Params::is_empty -> bool with true
MISSED   crates/blastit-fastcgi/src/lockdown.rs:589:32: replace | with ^ in hex_pair
MISSED   crates/blastit-fastcgi/src/param.rs:60:37: replace | with ^ in encode_len
352 mutants tested in 84s: 3 missed, 331 caught, 18 unviable
make: *** [Makefile:168: mutants-rust] Error 2      # cargo-mutants' "survivors exist", not a harness failure
```

```console
$ wc -l mutants.out/{caught,missed,timeout,unviable}.txt
  331 mutants.out/caught.txt
    3 mutants.out/missed.txt
    0 mutants.out/timeout.txt
   18 mutants.out/unviable.txt
  352 total
```

One command, one checkout, a private scratch on disk (not `/tmp`'s tmpfs — LESSONS #29);
`scripts/mutants-scratch.sh` pruned only this campaign's scratch. `mutants.out/` at the
repo root was replaced by this run (gitignored; the numbers above are the record). Note
that `mutants.out` is **per checkout** and a parallel `MB.3.5.1` leaf is active in this
one: this run's per-outcome files sum to exactly 352 with a clean baseline, so they are
internally consistent, but the `lead` should serialize `make mutants-rust` per checkout
(LESSONS #29).

| File | caught | missed | timeout | unviable | killed / viable |
|---|---|---|---|---|---|
| `src/lockdown.rs` | 189 | 2 | 0 | 3 | **189 / 191 = 99.0 %** |
| `src/cgi.rs` | 80 | 0 | 0 | 3 | **80 / 80 = 100 %** |
| `src/record.rs` | 34 | 0 | 0 | 7 | **34 / 34 = 100 %** |
| `src/param.rs` | 26 | 1 | 0 | 2 | **26 / 27 = 96.3 %** |
| `src/lib.rs` | 2 | 0 | 0 | 3 | **2 / 2 = 100 %** |
| **overall** | **331** | **3** | **0** | **18** | **331 / 334 = 99.1 %** |

Raw over every generated mutant: 331 / 352 = 94.0 %. Bar is ≥ 75 % → **PASS**. The 18
`unviable` are the same compile-failure classes as before: twelve "replace a function
with `Ok(Default::default())`/`Some(Default::default())`" on types that have no
`Default` (plus `Box::leak`), three `&&`→`||` rewrites inside `let`-chains
(`parse_status` ×2, `decode_path`), and two `+`→`*` in `hex_digit` that overflow `u8`.
They are not evidence either way.

## Survivors (3) — every one classified

| # | Mutant | Classification | Why |
|---|---|---|---|
| 1 | `lockdown.rs:185` `Params::is_empty -> true` | **deliberately untested** (equivalent in scope) | `pairs.is_empty()` → `true`. No production caller (clippy-required getter); the only call asserts the `true` direction after a refused insert. The `-> false` mutant **is** caught (`caught.txt:106`), so the getter's real direction is pinned. A getter-only assertion would be fluff (`testing` §3). No task. |
| 2 | `lockdown.rs:589:32` `\|` → `^` in `hex_pair` | **equivalent** | `hex_digit(high)? << 4` has its low nibble zero and `hex_digit` is always `< 16`, so `\| low` and `^ low` are identical for every input. |
| 3 | `param.rs:60:37` `\|` → `^` in `encode_len` | **equivalent** | `len` is filtered to `≤ 0x7fff_ffff` first, so bit 31 is clear and `len \| 0x8000_0000 == len ^ 0x8000_0000`. |

**No survivor changes observable behaviour**, so no mutation gap task.

## Branch-inspection residuals (cargo-mutants generated no mutant)

- **`cgi::head_end` at exactly `MAX_HEAD_BYTES`** — `src/cgi.rs:125`,
  `(end <= MAX_HEAD_BYTES).then_some(end)`. `head_total_cap_is_exactly_64_kib` builds a
  buffer of exactly 65 536 bytes ending in `\r\n\r\n`, but passes it to `parse_head`, not
  `head_end`; `head_end_finds_the_terminator` only reaches `MAX_HEAD_BYTES + 1` → `None`.
  cargo-mutants generated `replace <= with >` (caught) but **not** `<=` → `<`. With
  `<`, a legal exactly-64 KiB head reads as incomplete and the scheduler would fault a
  legal response (§4.5.2's "≤ 64 KiB total"). **Real → `MB.3.4.6`.**
- **A digit-leading header name** — `src/lockdown.rs:412-422`, `is_representable_header`'s
  `first.is_ascii_uppercase()`. §4.6.2 drops a folded name that "starts with a
  digit/`_`". `unrepresentable_header_name_is_dropped` covers `X.Foo`, `-x` (folds to
  `_X`) and non-ASCII, but no digit-leading name; `1x` is a legal RFC 7230 token from the
  front end. A regression accepting `A-Z0-9` first would emit `HTTP_1X` and every test
  would stay green (the `&&`→`||` and `-> true/false` mutants are caught, so this is a
  sub-branch hole, not a surviving mutant). **Real → `MB.3.4.6`.**
- **The empty folded name (`None` arm)** — reachable only through the public
  `RequestFacts` builder API, never from an HTTP head (the front end refuses an empty
  name). Included as an optional assertion in `MB.3.4.6`; on its own it would be a nit.
- `FCGI_KEEP_CONN` and `Params::is_empty` — see §"Public-surface sweep": no observable
  behaviour, no task.

## Gap task filed

| Task | Kind | Owner | Paths | Closes |
|---|---|---|---|---|
| `MB.3.4.6` | task, test-only | implementer | `crates/blastit-fastcgi/tests/critical_fastcgi_codec.rs`, `crates/blastit-fastcgi/tests/critical_fastcgi_lockdown.rs` | the two branch-inspection residuals above (`head_end` at the 64 KiB cap; digit-leading drop) |

Both files are already authorized by the task's paths, so the protected-file edits are
legal; nothing is weakened (pure assertion additions).

## Notes for the epic gate

- **Delta is pure addition in the protected tier.** `git diff --numstat 0f441af..HEAD`
  for the tests: `290/0` (`critical_fastcgi_codec.rs`, new) and `187/0`
  (`critical_fastcgi_lockdown.rs`, append-only). `cgi.rs` in the same range is `35/0`
  (tests only). `critical_guard.py --base 0f441af --node MB.3.4` reports
  `OK: all 2 protected change(s) are authorized`.
- **Design-doc drift (no task).** §7.1's "critical (new)" list names
  `critical_fastcgi_lockdown.rs` and the scheduler-level suites but not the new
  `critical_fastcgi_codec.rs`; §7.2's codec half still correctly points at
  `critical_fastcgi_dispatch.rs` as the eventual `502` owner. The new file is an
  additive interim promotion, recommended in the previous audit; the architect/lead may
  want §7.1's list updated when `MB.3.6.2.2` lands. Not a code or test defect.
- The epic's own `paths` list does not name `critical_fastcgi_codec.rs` (only
  `MB.3.4.4` does). That is fine while no node edits it; `MB.3.4.6` now lists it too.
- The crate is still zero normal dependencies, `#![forbid(unsafe_code)]`,
  `#![deny(missing_docs)]`, no `tokio`/socket/`std::fs`/`Command::new` in `src`, and no
  `unwrap`/`expect`/`panic!` outside `#[cfg(test)]`.
- `blastit-fastcgi` is covered by `make rs-test` and `rs-test-critical`'s
  `crates/*/tests/critical_*.rs` glob; no `Makefile --test` line is missing.
