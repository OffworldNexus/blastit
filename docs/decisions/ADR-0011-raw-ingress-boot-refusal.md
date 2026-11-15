# ADR-0011: `body = "raw"` Without `allow_raw = true` Refuses the Production Boot

- **Status:** Accepted
- **Date:** 2026-11-15
- **Node:** MB.3.53
- **Owner:** implementer
- **Supersedes:** `ADR-0006` decision 3, second bullet (`body = "raw"`: emits
  `ManifestWarning::RawIngress` at boot and accepts optional `allow_raw = true`)
- **Analysis:** `docs/design/learn-single-pass.md` §7.2 (mismatch 4), `R16`

## Context

`[route.ingress] body = "raw"` is the one ingress declaration with no runtime
gate. `plan_body` returns `BodyPlan::StreamPassthrough` for `BodyCodec::Raw`
whatever the manifest says (`crates/blastit-ingress/src/lib.rs`), so the bytes
reach the application's own parser unchanged. `ADR-0006` decision 3 made that
opt-out loud instead of gated: a boot warning plus an optional
`allow_raw = true`, with `blastit check` reporting `raw_ingress` when the key was
missing.

Three facts made the warning-only stance indefensible rather than merely
debatable (`learn-single-pass.md` §7.2, which this ADR implements and does not
re-derive):

1. **Nothing read the key at the moment of truth.** Outside tests, `allow_raw`
   was read by the boot warning (which fired whether or not the key was set), by
   the `check` finding, by the fix suggestion, and by the scheduled policy struct
   that carried it — never by a boot gate or a request path. The
   "acknowledgement" therefore had no consequence: an unacknowledged
   `body = "raw"` route in a production manifest booted, served, and streamed
   unvalidated bodies to the app.
2. **`check`'s own `suggest_fix` writes the key.** An agent or developer
   applying the suggested fix got `body = "raw"` *and* `allow_raw = true` in one
   edit, so the opt-out was a rubber stamp the tool filled in rather than a
   decision the author took.
3. **No preset, unit or image entry point runs `check`.** The documented CI
   checklist item was the only place the stamp was demanded, so a deployment
   that skipped CI shipped the hole.

That contradicts the product's central claim — every inbound byte decoded,
validated, re-encoded — for exactly the codec whose bytes are not. It is the
same class of defect as a capability that reports success while doing nothing.

## Decision

1. **On the production path, an unacknowledged `raw` route is a boot refusal.**
   A route declaring `body = "raw"` without `allow_raw = true` refuses the boot,
   naming the route and the key, **before any listener is bound**:
   - **stage 0** (`blastit`, the serve path — the branch `dev` does not take),
     after the `BLASTIT_APP` rule and before the static-root audit and
     `bind_view_sockets`;
   - **stage 1**, immediately after its own `parse_and_validate` re-read of the
     manifest. This is not belt-and-braces: the configuration document carries
     the manifest's **path** and no digest of what stage 0 approved, so a file
     swapped between the two stages would otherwise serve a raw route stage 0
     never saw (`learn-single-pass.md` §7.2). The stage-1 point is gated by the
     same `dev.is_none()` predicate as stage 0's.
2. **The policy has one home.** `Manifest::raw_ingress_refusals`
   (`crates/blastit-manifest/src/validate.rs`) returns one sentence per
   offending route, in route order, and is the only thing both stages consult —
   so the two checkpoints cannot drift about what is refused. An empty result is
   the gate being open; the callers refuse on `!is_empty()`.
3. **The other two environments keep their halves, and each is deliberate:**
   - **`blastit dev` warns and serves.** `ManifestWarning::RawIngress` is
     emitted at boot exactly as before, acknowledged or not. `dev` is the
     explicit single-UID local simulation; refusing there would make the local
     workflow unable to run the route it is developing.
   - **`blastit check` reports.** The `raw_ingress` finding (severity `error`)
     is unchanged, so CI fails before anyone deploys. It is the same rule,
     reported one environment earlier.
4. **Serving is otherwise unchanged.** Acknowledging the route restores exactly
   today's behaviour: `plan_body` streams the bytes unchanged, byte for byte.
   The change is a gate, not a new codec, not a body-size cap (the `raw` codec
   still has none: the acknowledgement buys streaming, not validation), and not
   a change to `suggest_fix`, which still proposes `raw` + `allow_raw = true`
   for an unknown or truncated sample — a separate follow-up recorded in
   `learn-single-pass.md` §13.

## Consequences

- **Positive:** "fails closed" is true for the one ingress declaration that had
  no runtime gate at all — `php2shell`/`node2shell`-class payloads in a `raw`
  body reach the application's parser only where the manifest says so in a line
  that no tool writes for the author. A manifest that passed `check` cannot be
  surprised by the new refusal: `check` already failed without the key.
- **Positive:** the local loop is untouched — `blastit dev` on the same file
  keeps running with the same warning.
- **Trade-off:** a deployment that removed `allow_raw` on purpose (or that
  predates the key and never ran `check`) now refuses to boot until the line is
  added. That is one manifest line for a route that was already declaring the
  codec, and it is the cost named in `learn-single-pass.md` §8.6.
- **Not changed:** the `raw` codec's missing body-size cap; `suggest_fix`
  authoring `allow_raw = true`; the fact that `raw` bodies are unvalidated once
  acknowledged. All three are recorded as follow-ups.

## What Would Make This Decision Wrong

1. **If an unacknowledged `raw` route were deliberately tolerated in production
   because the deploy checklist is trusted** (`learn-single-pass.md` §8.6). Then
   the boot refusal is one step too many for someone who removed the key on
   purpose. Rejected: nothing reads the key at run time, so the refusal adds no
   capability — it only makes the manifest's own opt-out the thing that decides.
2. **If the refusal could surprise a manifest that passed `check`.** It cannot:
   `raw_ingress` is an `error`-severity finding on exactly this shape, so the
   key was already mandatory for a green audit.
3. **If the stage-1 repeat were the only enforcement** — i.e. if stage 0's
   gate were removed, leaving only the re-read. The window between stage 0's
   bind of the view sockets and stage 1's read would then have approved
   listeners for a refused manifest, and the refusal would arrive after part of
   the tree was up. Both points are required together.
4. **If `dev`'s warning were the local equivalent of the gate** — e.g. if a future
   change made `dev` refuse too, the local workflow could no longer run a `raw`
   upload, and developers would remove the codec instead of acknowledging it.
5. **If the predicate answered per-`Route` rather than per-`Manifest`.** A
   manifest edited by hand or by an agent often has more than one offender;
   naming all of them, in route order, is what keeps the reader out of the
   one-denied-query-at-a-time loop. If a future refactor returned only the first,
   the boot message would send readers round that loop.
6. **If `blastit check` were also made to boot-refuse**, CI and the local loop
   would lose the report-then-fix path that lets an author see *which* routes are
   unacknowledged before running the app.

## References

- `docs/design/learn-single-pass.md` §7.2 (the ruling and its analysis), `R16`,
  §8.6, §13.
- `ADR-0006` decision 3 (superseded here), `ADR-0001` (dev simulation),
  `ADR-0004` (the `check` contract).
- `crates/blastit-manifest/src/validate.rs` (`raw_ingress_refusals`),
  `crates/blastit/src/stage0.rs`, `crates/blastit/src/stage1.rs`,
  `crates/blastit/tests/critical_raw_ingress.rs`.
