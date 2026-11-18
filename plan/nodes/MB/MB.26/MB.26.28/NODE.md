---
id: MB.26.28
title: "Close the blastit-view loaded.rs survivors: exact-cap boundaries, chunk counts, rebuilds and the two Display sentences"
kind: task
status: proposed
owner: implementer
priority: 3
depends_on: []
paths: ["crates/blastit-view/src/loaded.rs", "crates/blastit-view/tests/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-18T18:27"
updated: "2026-11-18T18:27"
---

# Goal

Kill the twelve `blastit-view/src/loaded.rs` mutants that survived `make mutants-rust
PKG=blastit-view` (`AUDIT-MB.36.md` §4.1/§4.5), each with the assertion that names the value it
protects:

- the exact-cap boundaries: `decode` at exactly `MAX_BATCH_BYTES` (`>` vs `>=`), `check_paths`'
  byte accounting (`bytes += 2 + len`, not `2 * len`) and its exact-cap refusal, `chunks`' byte
  boundary (`>`, not `>=`);
- `chunks(&[])` is empty: after a non-empty loop `start` can never equal `len`, so `start < len`
  → `<=` changes nothing there and only the empty input gains a spurious run — assert the empty
  list (an exact multiple does **not** kill `492:14`);
- `Step::rebuilds` is **false** for a steady-state delta (boot/resync/gap all false) — only the
  three true cases are asserted today;
- the two `Display` impls (`EncodeError`, `DecodeError`) render the sentence they document, and
  `DecodeError::Trailing(n)` carries `have - at`, not a ratio.

# Context

The campaign measured 191/222 (86.0 %); these twelve are the whole of `loaded.rs`'s `missed.txt`,
classified in `AUDIT-MB.36.md` §4.5:

| Survivor | Class | Action here |
|---|---|---|
| `64:39` `FLAGS_KNOWN = FLAG_BOOT \| FLAG_RESYNC` `\| → ^` | equivalent | none: `0b01` and `0b10` are disjoint, so `\|` and `^` are the same value |
| `326:20` `bytes.len() > MAX_BATCH_BYTES` `> → >=` | gap | an exactly-at-cap datagram is accepted (that is what `>` means); assert it |
| `384:47` `Trailing(have - at)` `- → /` | gap | assert the byte count in the refusal, not only the variant |
| `413:9` `Step::rebuilds -> true` | gap | assert `!rebuilds()` for a steady-state delta |
| `483:22` `chunks`' `2 + path.len()` `+ → *` | gap | a chunk-boundary case whose expected split depends on the real size |
| `484:63` `bytes + size > MAX` `> → >=` | equivalent | splits earlier, still fits: the wire contract is "each chunk fits", asserted already (`size <= MAX`) |
| `485:18` `index > start` `> → >=` | equivalent | `full` is false at `index == start` for any path `check_paths` accepts, so the empty-run branch is unreachable |
| `492:14` `start < paths.len()` `< → <=` | gap | only `chunks(&[])` differs (one empty run); a non-empty loop's `start` is always `< len` |
| `517:20` `check_paths`' `bytes += 2 + len` `+ → *` | gap | a list that fits under the real size and not under `2 * len` (or the reverse) |
| `519:14` `bytes > MAX_BATCH_BYTES` `> → >=` | gap | the exact-cap acceptance/refusal pair |
| `209:9` / `288:9` `Display for EncodeError`/`DecodeError` | gap | one case each: the sentence is the documented one, one line |

`the_loaded_file_report_is_exact` (in `blastit-view-python`) proves what the tracker reports, not
these wire-format edges, so the fix belongs beside the codec's own unit module (which already has
`chunks_split_at_the_byte_cap` and `a_trailing_byte_is_refused`).

# Acceptance

```console
$ cargo test -p blastit-view --locked --lib -- --list
$ make mutants-rust PKG=blastit-view
```
- `mutants.out/missed.txt` no longer names these twelve (or names only the rows this node
  reclassifies as equivalent, with the argument recorded in `AUDIT-MB.36.md` §4.2);
- every new assertion has a hand-computed expected value, not a read-back;
- `make check` green.

# Notes

The unit module is `#[cfg(test)]` inside `crates/blastit-view/src/loaded.rs`; a new
`tests/critical_*.rs` would also be legal but would duplicate the fixtures the module already has.
