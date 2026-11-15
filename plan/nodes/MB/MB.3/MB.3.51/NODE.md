---
id: MB.3.51
title: "Does check --learn fulfil the single-pass promise? Decide (a) restore it, or (b) amend VISION, and rule on MB.3.19's mismatch inventory"
kind: task
status: done
owner: designer
priority: 2
depends_on: []
paths: ["docs/design/**"]
attempts: 4
max_attempts: 4
escalation: none
created: "2026-11-15T07:46"
updated: "2026-11-15T08:26"
---

# Goal

Decide — on evidence, not on pages — whether `blastit check --learn` fulfils the
single-pass wire-learning promise that `VISION.md` and the CI story make, and rule on the
documentation/code mismatches `MB.3.19` could only correct on the page.

# Context

`MB.3.19` restructured `/frameworks/` and, forced to make the pages true, found that
`check --learn` cannot add anything to a route that already matches: `learn: 0 routes, 0
queries added` while the audit simultaneously reports `1 statement not granted`. The pages
now say so. That is correct for a docs node and wrong for the product, because `VISION.md`
promises:

> single-pass wire learning (`--learn` / `--record`) … records every route hit and outgoing
> call across an entire test run in one pass

and the CI story is one command that fills the manifest. If the only way to add a query to
a route you already declared is `dev --learn` (a manual dev-server step), that promise is
materially weaker than the vision.

## Established facts (read in the code at `872f523`; the designer must re-verify, not trust)

1. `check --learn` **is** a writer: `crates/blastit/src/check.rs:279` → `learn()`
   (`:957-1010`) writes once, and only when the suite passed, a request was observed,
   nothing was refused, it is not a dry run, and there is something to write (`R29`).
2. But `audit_v1::plan_learn` (`:1325-1422`) only ever considers **unrouted** observations:
   - the path set is filled from `table.route(method, path).is_none()` (`:1336-1340`) and
     from `ObsTarget::Unrouted` statements (`:1341-1345`) — new ROUTES only;
   - the grant grouping has `let ObsTarget::Unrouted { .. } = &statement.target else {
     continue }` (`:1360-1362`), so a statement whose target is an **existing** route is
     dropped before `grouped` is built.
3. The machinery for extending an existing route already exists and is unreachable from
   this path: `ProposedGrant` + `granted_already()` (`:1400-1422`, `:1425-1436`) filter
   duplicates against `effective_grants` — they only ever see statements that resolved to a
   *proposed new* route.
4. `infer()` (`crates/blastit/src/infer_routes.rs:62`) synthesises new routes from paths and
   has no notion of "add a query to route X".
5. `dev --learn` (`crates/blastit/src/dev_learn.rs`, `LearnRun::finish` → `apply_edits`)
   does patch existing routes; that is the only writer that can.
6. `crates/blastit-manifest/src/edit.rs`'s `apply_edits` already carries `static_policy`,
   `php` and `ingress` through the postcondition (`MB.3.21`), so a `check --learn` write
   would no longer damage a route's other blocks.

**Therefore the gap is narrower and more damning than "only one command writes":**
`check --learn` can create routes, but it can never grant a statement that an existing
route executed — which is the single most common thing a developer adds.

# Questions this node must answer

1. Is the ruling **(a) restore the promise** (make `check --learn` propose grants for
   statements whose target resolved to an existing route, behind the same refusal gates and
   the same dry-run/`--json` shape) or **(b) amend the promise** (`dev --learn` writes,
   `check` audits) — and in (b), what exactly changes in `VISION.md` and the CI page?
2. Reject the middle explicitly: documenting (a) while the code does (b) is where we are
   now, and it is what this node exists to stop.
3. Rule on the mismatch inventory below: for each, say **docs are now accurate about
   unfortunate behaviour** or **the code has a bug** — and if a bug, what the fix is and
   whether it is this milestone's.

## Mismatch inventory from `MB.3.19` (one judgement each)

| # | Claim | Where | Lead's reading |
|---|---|---|---|
| 1 | `php.md` taught `check -- <suite>` and `BLASTIT_URL`; neither exists in the code | docs | **docs fixed, correctly** — a page teaching a nonexistent flag is the worst kind of DX lie |
| 2 | `check --learn` cannot extend an existing route; `dev --learn` can | code | **product gap → this node.** Facts above; the `Unrouted` filter at `:1360-1362` is the seam |
| 3 | `manifest.md` says all-static + `app` is a boot error; the code boots | docs/code | **needs a ruling.** Either the rule is real and the code has a validation gap, or the sentence is wrong and a meaningless `app` on a static route is tolerated. Say which, and why a reader is safe |
| 4 | `allow_raw` is a warning/finding, not a gate | docs/code | **needs a ruling.** Documenting it is honest; but if raw bodies are a CVE surface, a warning may be too weak — that is a security-design call, not a docs call |
| 5 | every symlink under `static.root` is 404 | code | **behaviour, documented** — fail-closed traversal is right; the page says it |
| 6 | `immutable` globs do not cross `/` (Django needs one pattern per depth) | code | **probable DX bug** — a glob that silently does not match is the "whack-a-mole" the vision forbids. Say whether `**` should be supported, and if so file it |
| 7 | three published anchors vanished; `MB.3.19` bridged them with explicit ids | docs | **docs plumbing, fixed** — and retarget the bridges when `MB.18`/`MB.11` touch those pages |

# Acceptance

- A decision record under `docs/design/` (e.g. `learn-single-pass.md`) that: restates the
  facts above with file:line **after verifying them**, gives the (a)/(b) verdict with the
  rejected middle named, and rules on mismatches 3, 4 and 6 (with the fix and its owner).
- If (a): the record specifies the proposed change precisely enough that an implementer
  needs no further decisions — which observations become grants, how they are filtered
  against `effective_grants`, what the postcondition guarantees, what the `--json` document
  looks like, and the fixture-level acceptance: **one `check --learn` run on a fixture app
  produces a manifest diff that a following `check` accepts with no findings**, plus a
  bring-your-own-suite run. Paste the `plan_learn` seam you propose to change.
- If (b): the exact replacement sentences for `VISION.md` (strategist-owned — hand them
  over, do not edit it) and for the CI page, and the reason a reader is better served.
- `make docs-check` green for anything written; `uv run --no-project tools/plan.py validate`
  green. No code changes in this node: it decides, it does not implement.

# Notes

- The lead's reading is **(a)**, because the machinery already exists and is merely
  filtered out at `audit_v1.rs:1360-1362`; the designer must confirm or refute that with
  the code in front of it, and say what breaks if it is wrong.
- The implementer node(s) follow this record; the lead creates them from it.
- `VISION.md` is the strategist's; if (b) is chosen the amendment is escalated, not made
  here.
