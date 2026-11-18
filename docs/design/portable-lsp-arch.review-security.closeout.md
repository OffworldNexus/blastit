VERDICT: FAIL

# Security close-out — `MB.35.3`, the three claimed resolutions

Scope: only the three claims in the author's close-out request, checked in
`docs/design/blastfile-lsp-zed.md` at `.worktrees/release-0.2.0` (`143afaf` plus the
`MB.35.3` working-tree edits), against
`docs/design/portable-lsp-arch.review-security.final.md` (0 BLOCKER, 3 MAJOR). No new
MINOR hunt beyond the one-line list at the end.

| Claim | Verdict |
|---|---|
| 1. §1.3 row 3 classifies `use`/paths by origin, manifest API allowlisted; `std::fs`/`std::net`/`blastfile::read` are hits | **VERIFIED.** The two final-round evasions are closed. The allowlist omits names the server already imports (MINOR 1), which fails closed and does not reopen the evasion. |
| 2. Every `#[cfg(…)]`/`#[cfg_attr(…)]`/`cfg!(…)` banned except `#[cfg(test)]` | **VERIFIED in §1.3.** Blanket rule, `all(…)`/`any(…)`/`target_env`/`cfg!` are covered. Its propagation to the leaf brief is part of MAJOR 1 below. |
| 3. R73, S12 and §14.4 say "every file under `src/**` except the pinned `main.rs`" | **NOT RESOLVED.** R73 (2955) and S12 (2443) are fixed; §14.4's `MB.35.6` row (3008) still says `lib.rs`/`hover.rs`, and the resolution ledger (3079) asserts it was changed. |

| Grade | Count |
|---|---|
| BLOCKER | 0 |
| MAJOR | 1 |
| MINOR | 2 |

## MAJOR 1 — §14.4's `MB.35.6` hand-off still ships the two-file set (and the pre-`cfg` token list), and the ledger says otherwise

**Quotes.**

- §14.4, line 3008, the `MB.35.6` row's *"This design requires"* cell: "§1.3's row 3
  (the §1.2 list plus `std::net`, `std::fs`, `std::path`, `std::process`, `unsafe` items,
  `extern "C"`, `include!`, **with `lib.rs`/`hover.rs` as the file set** and `main.rs`
  pinned), every dependency table, one twin per token from the guard's own table …".
- Resolution ledger, line 3079: "security final M3 (two-file set vs crate-wide) —
  **accepted → §14.1 R73, §11 S12, §14.4.** All three now say 'every file under `src/**`
  except the pinned `main.rs`'".
- The normative row, line 255: "no line in **any** file under `crates/blastit-lsp/src/**`
  except the pinned `src/main.rs` — i.e. `lib.rs`, `hover.rs`, and any file a later change
  adds".

**Failure mode.** §14.4:2998-3000 is the only text that corrects the leaf briefs ("the
design wins"), and `MB.35.6` is the node that writes `scripts/check_lsp_portable.py`. Its
brief still names two files, so the implementer writes a two-file scan. The final round's
evasion 1 is then restored in the artefact that becomes code: add `mod helper;` to
`lib.rs`, put `use std::fs;` in a clean `src/helper.rs`, and `make lsp-portable` is green
while R73 and S12 claim a crate-wide scan. The ledger row is worse than stale — it tells
the lead the hand-off was corrected, so the epic can close on a false record.

The same cell also stops at the pre-revision token list ("the §1.2 list plus …") and never
carries the new blanket `cfg` rule. Read as the rule, `#[cfg(all(unix, target_arch =
"aarch64"))]` — live on two shipped legs — passes the leaf's guard even though §1.3:255
now bans it. Claim 2 is correct in the normative row and absent from the only text the
leaf is told to follow.

**Exact fix (one cell + one ledger line).** In the `MB.35.6` cell, replace "(the §1.2
list plus … with `lib.rs`/`hover.rs` as the file set and `main.rs` pinned)" with "§1.3's
third row in full — every file under `src/**` except the pinned `main.rs`, the token table
**and** the blanket `#[cfg(…)]`/`#[cfg_attr(…)]`/`cfg!(…)` ban (only `#[cfg(test)]` on a
test module)". Rewrite ledger row 3079 to say §14.4 is corrected by this edit. No test
changes; `MB.35.6`'s twins already have to come from the guard's own table.

## Why claims 1 and 2 pass (checked, not taken on trust)

- **Origin classification is real and borrowed from a real mechanism.** §1.3:255's
  enforcement column says "classifies `use` items and paths by origin, the way
  `scripts/check_manifest_purity.py` does". That script (lines 18-47) resolves
  `use std::{fs, net}`, `use std::process as p`, transitive `std` aliases, raw identifiers
  and `use std::{io, env::{self, var}}`; it refuses glob imports and unclassifiable items
  rather than passing them. So both final-round evasions are hits, and a third module is
  inside the file set.
- **The path-opener is explicitly red.** §1.3:255: "the manifest API is allowlisted …
  so `blastfile::read`/`write` and `blastit_manifest::edit` — which open paths — are a
  red." `blastfile::read` is public at
  `crates/blastit-manifest/src/blastfile/mod.rs:270`, so naming it closes the second
  evasion, not just the `std::fs` spelling.
- **The `cfg` rule is structural, not an enumeration.** §1.3:255 bans "**any
  `#[cfg(…)]`, `#[cfg_attr(…)]` or `cfg!(…)` other than `#[cfg(test)]` on a test module**
  (so `#[cfg(all(unix, target_arch = …))]` is a hit)". `target_env`, `target_family`,
  `any(…)`, `not(…)` and `cfg!` no longer need their own spellings; `#[cfg(test)]` stays
  legal and the pinned `main.rs` shape is checked separately (attribute + one `fn main` +
  one `std::process::exit`), so the `cfg` exemption for `main.rs` does not admit a second
  behaviour.
- **No new BLOCKER surface.** Nothing in the three edits adds a privilege crate, a
  socket, an environment read, or a second protocol; `blastit lsp` still calls
  `hold_none()` before reading a byte, and the final round's honest gap (HTTPS to
  `github.com`, no signature in 0.2.0) is unchanged.

## Leftover MINORs (one line each)

1. **§1.3:255 allowlist omits names the extracted server imports today** —
   `Manifest`, `ManifestWarning`, `UidLayout`, and `blastfile::MAX_SOURCE_BYTES`
   (`crates/blastit/src/lsp/mod.rs:54,79,87`, `:1085`). A literal allowlist reds the
   server on its first run; add them to the sentence before `MB.35.6` writes the guard.
2. **ADR-0013:343** still says the greps forbid those tokens "in `lib.rs`/`hover.rs`";
   the next clause correctly defers to §1.3's third row, but delete the stale pair so the
   decision record cannot be quoted for the two-file set.

## Bottom line

The two code-level rules are now sound. The hand-off row that `MB.35.6` is written from
was not changed, so the crate-wide rule does not reach the implementer — and the
resolution ledger records the opposite. Re-run this close-out after the §14.4 cell and
ledger row are corrected; no source or test change is required.
