# MB.40.3.1 — `ObsVerdict` → `blastit_sqlpolicy::LearnVerdict` (wire serde adapter)

**STATUS: done.** The `blastit-dbproxy` verdict fork is gone; `WireSqlObs.verdict`
is `LearnVerdict` behind exactly one adapter module, and the JSON record is
byte-identical in both directions.

---

## 1. What was deleted

`crates/blastit-dbproxy/src/learn.rs` no longer declares `ObsVerdict`:

| removed | replaced by |
|---|---|
| `pub enum ObsVerdict { Query{normalized}, Refused{redacted,reason:String} }` + `#[serde(tag="kind", rename_all="snake_case")]` | `mod verdict_wire` — one adapter: a private `Wire` shape enum plus `serialize`/`deserialize` |
| `impl ObsVerdict { is_query, normalized, redacted, reason }` | `LearnVerdict`'s own methods (identical signatures, except `reason() -> Option<&RefuseReason>`) |
| `impl From<LearnVerdict> for ObsVerdict` (with its non-exhaustive `_ =>` fallback) | the fallback moved, verbatim in intent, into `verdict_wire::serialize` |
| `pub use learn::{ObsTarget, ObsVerdict, WireSqlObs}` (`crates/blastit-dbproxy/src/lib.rs:109`) | `pub use learn::{ObsTarget, WireSqlObs}` — one line, and necessary for the crate to compile |

`grep -rn "ObsVerdict" crates/` → **no matches** (exit 1).

`crates/blastit-sqlpolicy/src/learn.rs` gained one variant,
`RefuseReason::Reported(String)` — the wire's round-trip carrier for a reason
whose only surviving form is the one-line explanation — with
`Display` = the text unchanged, so re-serialising a record read back is exact.

## 2. Consumers switched (all in the `blastit` crate, as re-scoped)

| file | change |
|---|---|
| `crates/blastit/src/audit_v1.rs` | import → `blastit_sqlpolicy::{…, LearnVerdict, …}`; `:21` doc; `:745` `Query`, `:1531` `Refused`, `:1557` `Query` |
| `crates/blastit/src/dev_learn.rs:373` | `blastit_sqlpolicy::LearnVerdict::Refused`; `format!("{redacted}: {reason}")` unchanged (`RefuseReason: Display`) |
| `crates/blastit/src/check.rs:1232` | test fixture builds `blastit_sqlpolicy::LearnVerdict::Query { normalized }` |

## 3. Wire contract — byte-identical (evidence)

**Before** (`git show e6b1291:crates/blastit-dbproxy/src/learn.rs`):
`#[serde(tag = "kind", rename_all = "snake_case")]`, variants
`Query { normalized }` / `Refused { redacted, reason: String }` →
`{"kind":"query","normalized":…}` / `{"kind":"refused","redacted":…,"reason":…}`.

**After** — pinned by the new test, which asserts the whole record as one exact
string and then re-serialises what it read back:

```
{"target":{"scope":"routed","route":"a"},"proxy":"db","kind":"query","sql":"SELECT …",
 "is_tx":false,"allowed":false,"verdict":{"kind":"refused","redacted":"SELECT …",
 "reason":"the statement inlines a quoted literal; bind the value with a placeholder instead"}}
```

and, for the grantable half:
`…"verdict":{"kind":"query","normalized":"SELECT $?"}}`.

Third piece of evidence, in-tree: `crates/blastit/src/dev_learn.rs:1131-1143`
holds a literal **pre-change** record (`"verdict":{"kind":"refused","redacted":…,"reason":"the literal was inlined"}`)
as a fixture, and its test still passes — the adapter reads the old bytes.

## 4. Tests added / adapted

* **Added** `crates/blastit-dbproxy/src/learn.rs::the_verdict_wire_object_is_byte_identical_in_both_directions`
  — exact bytes for both variants; refused record round-trips its sentence
  verbatim through `RefuseReason::Reported`; `to_string(deserialise(x)) == x`.
* **Adapted** `learn::tests::a_refused_verdict_carries_the_redacted_text_and_the_reason`
  — now asks the policy verdict directly (no `From` impl left to call) and
  asserts `matches!(reason, Some(RefuseReason::InlineLiteral))` plus the sentence.
* **Adapted (the one authorized protected change)**
  `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs:784-792`:
  `reason().is_some_and(|r| r.contains("literal"))` →
  `matches!(reason(), Some(RefuseReason::Reported(r)) if r.contains("inlines a quoted literal"))`.

  Why the carrier and not the node's suggested `InlineLiteral`: this assertion
  inspects an observation **read back off `obs_fd`**, and enum identity is not on
  the wire — `InlineLiteral` cannot survive serialisation. What must hold, and
  does, is the same refusal expressed as the same one-line reason, reached
  through the carrier that exists for exactly this. It is strictly narrower than
  the old `contains("literal")` (it names the sentence) and still fails on a
  grant, a redaction-only verdict, or any other refusal class.

## 5. Verification

`cargo fmt --all` clean; one final serial pass:

```
$ cargo test -p blastit-sqlpolicy -p blastit-dbproxy --locked
test result: ok. 82 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (sqlpolicy lib)
test result: ok. 5 passed;  0 failed                                          (sqlpolicy doc)
test result: ok. 9 passed;  0 failed   (blastit-dbproxy critical_learn_and_redaction)
test result: ok. 24 passed; 0 failed   (blastit-dbproxy postgres)
test result: ok. 258 passed; 0 failed  (blastit-dbproxy lib)
... (all targets ok, 0 failed)

$ cargo test -p blastit --locked --lib
test result: ok. 253 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

$ grep -rn "ObsVerdict" crates/
(no output, exit 1)

$ uv run --no-project tools/critical_guard.py --base e6b1291 --node MB.40.3.1
critical-guard: e6b1291..working tree, N changed file(s)
node MB.40.3.1: paths [...]
  no change to a protected critical test
```

`make fast-check` (guard + critical-guard + fmt + clippy `-D warnings` +
workspace tests + py-lint + plan-validate): **passed**, see the run recorded in
the transcript of this attempt.

## Follow-ups

1. **`crates/blastit-dbproxy/src/lib.rs` is outside this node's `paths`** but had
   to change: the one-line `pub use learn::{…, ObsVerdict, …}` cannot survive the
   enum's deletion. Add the file to a future node touching this crate.
2. **`docs/design/learn-single-pass.md`** (`:206`, `:247`, `:1268`) and its two
   `*.review-*.md` siblings still name `ObsVerdict` as the wire record. They are
   design history, excluded from the public build, and out of `paths` here; a
   docs pass should describe the adapter instead.
3. **Enum identity does not cross the wire** and now provably cannot: a consumer
   of `WireSqlObs.verdict` sees `RefuseReason::Reported(sentence)`. Nothing in
   `crates/` matched on the reason's variant, only on `Query`/`Refused` and on
   the rendered text, so no behaviour changed — but a future consumer must not
   assume `InlineLiteral` reaches it.
4. `MB.40.3.4` owns the `RequestIo` → `ChanIo` seam and the ADR-0016 exemption in
   `crates/blastit/tests/critical_artifacts.rs`; untouched here.
