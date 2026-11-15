---
status: In review
owner: designer
node: MB.3.51
reviewers: []
---
# `check --learn` and the Single-Pass Promise: a Decision Record

Decision: **(a) restore the promise** — `blastit check --learn` must propose
exact `queries` for a statement whose target resolved to an **existing** route,
behind the refusal/dry-run/`--json` gates it already has. The rejected
alternative is **(b) amend the promise** (hand the single-pass claim to
`dev --learn`). The *rejected middle* — documenting (a) while the code does (b),
which is where 0.2.0 stands after `MB.3.19` — is rejected because a page that
describes a capability the product does not have is the worst defect class we
ship, and `dev --learn` is not a substitute: it needs a served process and a
human clicking, and it proves nothing about the suite.

This document records the decision only. It changes no code and no page outside
`docs/design/`.

---

## 1. Developer Experience Target (Local, CI/CD, Deploy)

| Environment | Command the record serves | What must be true afterwards |
|---|---|---|
| Local | `uv run blastit check --learn` then `uv run blastit check` | one suite run produces a `blastit.toml` diff the next `check` accepts with no findings *that `--learn` can write* (transaction control is the one named exception, §7.4) |
| CI | `uv run blastit check --json` in the workflow ([`ci.md`](../ci.md)) | the same, and `--learn --json` keeps its one-document shape |
| Deploy | the `blastit check` pre-deploy item ([`deploy/index.md`](../deploy/index.md) line 111) | the manifest shipped is the one the suite proved, including grants on routes that already existed |
| Agent | `--learn --json` (writes) → `check --json` (`ok: true`) | the loop terminates: every grantable statement is written in the first pass, and the only remaining edit (`transaction_control`) is named by a `fix` object (R18); no `unrestricted = true`, no second `dev` session |

The promise under test is `docs/VISION.md` §3 (strategist-owned, quoted, not
edited here): *"it records every route hit and outgoing query/call across an
entire test run in one pass"*, and the `MB.3` framing that "the CI story is one
command that fills the manifest".

---

## 2. What the Code Actually Does at `872f523`

Every claim below was re-read in the worktree at `872f523`, not taken from
`MB.3.19`'s inventory. `MB.3.19`'s six bullets are substantially right; four
are oversimplified and one is incomplete in a way that matters.

| # | Claim | Verdict | Evidence |
|---|---|---|---|
| F1 | `check --learn` is a writer | **right** | `check.rs:279-281` → `learn()` (`:957-1012`); writes only through `apply_edits` + `write_atomic` when `plan.would_write` (`audit_v1.rs:1284-1289`: not dry-run, `requests > 0`, nothing refused, something to write) |
| F2 | `plan_learn` only considers unrouted observations | **right, but for a subtler reason than stated** | `audit_v1.rs:1360-1362` (`let ObsTarget::Unrouted { path, .. } = … else { continue }`) drops everything else |
| F3 | `ProposedGrant` + `granted_already` exist and are unreachable for existing routes | **right** | `audit_v1.rs:1398-1417`, `:1428-1436`; they only ever see statements resolved to a *proposed* route |
| F4 | `infer()` has no notion of "add a query to route X" | **right** | `infer_routes.rs:62` returns `RoutePlan { routes, by_path, insert_at }` — routes only |
| F5 | `dev --learn` is the only writer that patches existing routes | **right**, and its `edits()` is the (a) algorithm already | `dev_learn.rs:479-574` groups `ObsTarget::Routed { route }`, filters `effective_grants` + `is_query_covered_by_tables_or_groups` (`:542-557`), de-duplicates (`:558-560`), then `apply_edits` (`:459-471`) |
| F6 | `apply_edits` carries `static_policy`/`php`/`ingress` through the postcondition | **right** | `edit.rs:913-960` (`expected_routes` chains `.with_static/.with_php/.with_ingress`); `MB.3.21`, commit `deb80c8` |

### 2.1 Where `MB.3.19` oversimplified

**O1 — the seam is not an attribution problem, and that is why the fix is
small.** `ObsTarget::Routed { route }` already carries the **route name**
(`blastit-dbproxy/src/learn.rs:84-97`). Before `plan_learn` runs, `check` calls
`resolve_targets` (`check.rs:912-943`), which rewrites every `Unrouted { method,
path }` whose path the `RoutingTable` matches into `Routed { route: <name> }` —
the same name `evaluate` uses to attribute a `missing_grant`
(`audit_v1.rs:637-643`). So the statements `plan_learn` throws away are *already
attributed*; the fix needs no routing logic of its own, only a `match` arm.

**O2 — but the two entry points disagree, and that is a second defect.**
`plan_learn_json` (`audit_v1.rs:1446-1457`, behind `_blastit.plan_learn`) parses
raw JSONL and does **not** resolve targets. Two consequences:
`resolve_targets`' doc comment (`check.rs:906-911`) is true of the gate but the
bridge never inherits it; and the SQL path loop (`audit_v1.rs:1341-1345`)
inserts an unrouted statement's path **without** the `table.route(…).is_none()`
guard the HTTP loop has (`:1336-1340`). An observation stream from the bridge
that names a path the manifest already routes therefore makes `infer` propose a
**duplicate route** (`notes-2`, because `infer` disambiguates against used names,
`infer_routes.rs:96-99`). The CLI cannot reach this; the bridge can. Any fix must
make attribution one function used by both.

**O3 — the transaction-control hint is under-specified, not false.**
`audit_v1.rs:1367-1371` prints `set transaction_control = true on [route.{}]`
filled with `statement.proxy`, i.e. `[route.db]`. That is **correct TOML**: inside
an `[[route]]` block `route` is the current element, so `[route.db]` is that
route's `db` grant table (`docs/reference/manifest.md:239` shows the same header
under one route). `[route.<name>.<proxy>]` — this record's first draft, and the
obvious "fix" — is **wrong**: it would parse as an unknown key on a route named
`<name>`. The real defect is that the hint names no route, so with eight routes
in the file the reader cannot tell which block to edit, and for a statement on an
existing route (new under (a)) the answer is known for free. The route name goes
in the sentence, not in the table header (R11).

**O4 — "the machinery is only filtered out" is true for grants and false for
formatting.** `edit.rs` folds a multi-line `queries` list onto one line by
design and test (`edit_branches.rs:79-107`), and it *refuses* an inline grant
table (`edit.rs:806-813`, `EditError::InlineGrant`). Folding loses an interior
comment silently: `verify` (`edit.rs:976-1000`) compares the **model**, so text
outside the edited field is unverified. Today that path is `dev --learn`'s; (a)
makes it the common one. §5.4 rules on it.

**O5 — the two writers' refusal gates differ and must keep differing.**
`check` writes *nothing* if any statement was refused (`would_write`,
`audit_v1.rs:1287`); `dev` writes everything grantable and exits `1`
(`dev_learn.rs:466-474`). (a) does not change either. `MB.3.19`'s "the refusal
gate" bullet treats them as one rule.

---

## 3. The Decision

**(a) restore the promise.** `plan_learn` gains a second attribution source —
`ObsTarget::Routed { route }` — and proposes exact `queries` for it under
exactly the gates it already applies to a proposed route.

**Rejected — (b) amend the promise.** (b) is honest but it moves a step the
vision explicitly removed onto the developer: *"instead of failing on the first
query per request"* (`VISION.md` §3) becomes "run your suite, then start a dev
server and click everything the suite already clicked". For an AI agent there is
no click-through at all, so (b) leaves hand-editing or `unrestricted = true` —
the two things `VISION.md` §3 forbids by name. If (b) were chosen the amendment
would have to say that `check --learn` fills routes but not grants, and that one
`dev --learn` session per existing route is the documented path; the CI page's
`--learn` block would need the same sentence. That is a larger DX loss than the
~60 lines (a) costs, and it leaves `dev --learn`'s `edits()` (F5) as the only
correct implementation of the rule while a second, wrong one ships beside it.

**Rejected middle — "document (a), implement (b)".** Current state. Named here
so it is not re-proposed as a fix: the pages concede `learn: 0 routes, 0 queries
added` while the audit reports `1 statement not granted`. A concession paragraph
is a defect with a changelog entry, not a design.

**Also rejected:** `check --learn` writes a `tables` or `group` grant wide enough
to cover the statement (widens the trust root to save a line; `R27` and
`ADR-0003` forbid it); `check` spawns `dev --learn` internally (two writers, two
refusal gates, a second manifest read mid-suite); print a paste-ready patch and
write nothing (that is (b) with worse ergonomics).

---

## 4. Requirements (`R1..R21`)

| ID | Testable requirement |
|---|---|
| **R1** | A statement whose observation target is `Routed { route }` for a route the manifest declares becomes a candidate exact `queries` entry on that route's `[route.<proxy>]` table, provided its verdict is `Query { normalized }`, it is not `is_tx`, and `granted_already` is false. |
| **R2** | The grant lands on the **same** route name `evaluate` attributes a `missing_grant` to (`audit_v1.rs:637-643`), so the next `check` on the edited manifest reports neither `missing_grant` nor `dead_grant` for that statement. |
| **R3** | `Routed { route }` naming a route the manifest does not declare is skipped (`learn_route` returns `None`), never turned into a `ProposedGrant` — an undeclared name would fail the whole write with `EditError::MissingRoute` (`edit.rs:570-575`). `Unrouted { method, path }` resolves through `RoutingTable` **inside** `plan_learn`, so the CLI and `_blastit.plan_learn` agree on the same observation bytes. |
| **R4** | Only a path no route matches contributes to `infer`'s input; an `Unrouted` statement whose path the table matches can no longer cause a duplicate `<name>-2` route (O2). |
| **R5** | De-duplication is `granted_already` (`audit_v1.rs:1428-1436`) — `normalize(query) == sql` against `effective_grants`, plus `is_query_covered_by_tables_or_groups` — so a statement covered by `queries`, `tables`, a `group`, or `unrestricted = true` proposes nothing. A route with `unrestricted = true` never reaches `EditError::UnrestrictedGrant`. |
| **R6** | Repeated learning is a fixed point: a second `check --learn` on the unchanged manifest proposes `0` routes and `0` queries, exits `0`, and leaves the file byte-identical (`apply_edits` returns the source when there is nothing to add, `edit.rs:361-374`). |
| **R7** | The write gates are unchanged: nothing written on a nonzero suite, on `--dry-run`, with zero requests, or if **any** statement was refused (`would_write`, `:1284-1289`); the CLI document's `wrote` field reflects the actual write, never the intent (R20 for the bridge, which cannot write). |
| **R8** | The `--learn --json` document keeps its shape: `{"proposed":{"routes":N,"queries":M},"wrote":bool}`, no `schema_version`; `queries` counts grants on existing routes too, `routes` only proposed ones. Its four states are disambiguated by exit code and `proposed`, not by a new field (§5.5 table). The `_blastit.plan_learn` bridge keeps the same shape and says in its rustdoc that it never writes (`wrote` is the decision the CLI would take) and prints the refusal lines (R20). |
| **R9** | `apply_edits`' postcondition still guarantees the edit touched nothing else — top-level keys, proxies, groups, every other route, and the edited route's `[route.static]`/`[route.php]`/`[route.ingress]` blocks (`edit.rs:913-1000`). It compares parsed **models** (`verify`, `:976-1000`) and, per its own doc comment, re-parses without validating; the *acceptance test* additionally proves the result passes `Manifest::parse_and_validate` (the oracle `critical_check_engine.rs:330-334` already uses). |
| **R10** | A multi-line `queries` list is extended by **inserting element lines**. Every existing line is byte-identical except (a) the last element line gains a `,` before its trailing comment when it had none, and (b) when the closing `]` shares its line with an element, a newline is inserted before the `]` and that line's trailing comment moves with it. Each new entry occupies one line; an empty multi-line list gains entries without touching `queries = [`. A one-line list keeps the existing in-place `append_to_array` extension and its trailing comment (`edit_branches.rs:113`). |
| **R11** | The transaction-control hint keeps the correct table (`[route.<proxy>]`, a sub-table of the current `[[route]]`) and now names the route it belongs to, for a proposed and an existing route alike (O3). |
| **R12** | No new privilege, secret, network call, or file read: the writer stays unprivileged, writes only `<manifest>` through `write_atomic`'s `O_CREAT|O_EXCL` + `rename` (`edit.rs:385-441`), and logs no literal value. |
| **R13** | The human `learn:` line and the `--json` `wrote` field agree: "added" only when the file changed, "(dry-run)" when `--dry-run`, "(not written)" for a blocked run. §5.5. |
| **R14** | A run that only printed transaction-control hints still writes the grants it found and still exits `0`; the *following* `check` remains red on that statement until the author adds `transaction_control = true` (the one finding `--learn` cannot clear, §5.6, §7.4). |
| **R15** | A route whose grant is an inline table is refused by name (`EditError::InlineGrant`, whose message names the `[route.<proxy>]` form), and the whole write is refused — never duplicated beside the inline grant. |
| **R16** | On the production path (`dev.is_none()`; `dev` also re-execs into stage 1, so the stage-1 point is gated by the same flag), **every** route declaring `body = "raw"` without `allow_raw = true` is refused — both in Stage 0 and again in Stage 1 after its own `parse_and_validate` (`stage1.rs:254-262` re-reads the manifest from disk, so one check point is a TOCTOU) — with one error naming each offending route and the key. `blastit dev` keeps `ManifestWarning::RawIngress` (one per route, unchanged); `blastit check` keeps the `raw_ingress` finding. Serving is otherwise unchanged (`ADR-0006` amended by `ADR-0011`). §7.2. |
| **R17** | Every non-clean `--learn` run prints a cause and the "(not written)" line to **stderr**, including under `--json`; a zero-observation suite says `learn: nothing observed: …` with the audit's DATABASE_URL hint. The four-state table in §5.5 is the contract (R13). |
| **R18** | A transaction-control denial in `evaluate` carries `Fix::set_transaction_control` with `to = format!("[route.{proxy}].transaction_control")` (the finding's own proxy, never the literal `db`) instead of `Fix::observe`, so the `--json` consumer has a machine-readable remedy and the documented loop never has to reach for `unrestricted = true`. `to` is a table locator relative to the finding's own `route` field, exactly like `add_query`'s `[route.<proxy>].queries`; the finding keeps carrying `route`, and no new key is added to `Fix`. |
| **R19** | Each refusal line names the route and proxy the statement belonged to (`route "notes" [route.db]: <redacted>: <reason>`), or the proxy alone when no route resolves; the SQL text is the broker's `redacted` field (R12), and the label-only `…`-in-`Query` case prints **no** wire text at all. Every component is passed through the control-byte escaper before formatting (route/proxy capped at 64 bytes, reason 256), the SQL is truncated last, and the finished line is ≤ 2048 bytes, so a bridge-supplied proxy name can neither forge a line nor blow the log budget. |
| **R20** | One builder for the `--learn --json` document (`LearnPlan::json(wrote)`); `check::learn` passes the actual write result, and `_blastit.plan_learn` prints the refusal lines and documents that it never writes. |
| **R21** | An `app`/`BLASTIT_APP` that **no route could reach** — every route is `[route.php]` (with or without a `[route.static]` block, which forwards to PHP), or `[route.static]` with `spa = true` — is refused by `blastit` (Stage 0 **and** Stage 1's re-read) and by `blastit check`, from one predicate `no_route_reaches_the_app`; `app` beside a non-`spa` static route stays legal, because that route forwards a miss to it. `Manifest::can_boot_without_app` and the second half of `app_rule` are untouched. §7.1. |

---

## 5. The Precise Proposed Change

All in `crates/blastit/src/audit_v1.rs` except §5.4 (`blastit-manifest`).

### 5.1 Attribution (replaces `audit_v1.rs:1335-1346`)

```rust
    let mut paths: BTreeSet<String> = BTreeSet::new();
    for line in observations.http() {
        if table.route(&line.method, &line.path).is_none() {
            paths.insert(line.path.clone());
        }
    }
    for statement in observations.sql() {
        // The HTTP loop's guard, for the same reason: a path the table routes
        // already has a route, and `infer` would only propose a second one
        // (`notes-2`) for it (`R4`).
        if let ObsTarget::Unrouted { method, path } = &statement.target
            && table.route(method, path).is_none()
        {
            paths.insert(path.clone());
        }
    }
    let plan = infer(paths.iter().map(String::as_str), manifest);
```

### 5.2 The refusal set and the grouping seam (replaces `audit_v1.rs:1348-1385`)

The refusal is the one thing a blocked write tells the reader, so it must name
where it came from. Replaces the current bare-text loop, after `infer` (it needs
`plan`). (The `R24`/`R33`/`R25`/`R29` references inside the pasted comments are
the existing codebase's requirement ids for the audit and ingress designs, not
this record's `R`-numbers.)

```rust
    // Any refused statement in the run makes the whole plan unwritable: the
    // manifest after the edit would still be incomplete (`R25`, `R29`). Each
    // line names the route and proxy the statement belonged to, because this
    // is the only channel a blocked write has (`R17`, `R19`).
    let mut refused: BTreeSet<String> = BTreeSet::new();
    for statement in observations.sql() {
        if let ObsVerdict::Refused { redacted, reason } = &statement.verdict {
            let location = match learn_route(&table, &plan, manifest, &statement.target) {
                Some(route) => format!("route \"{route}\" [route.{}]", statement.proxy),
                None => format!("proxy \"{}\"", statement.proxy),
            };
            refused.insert(format!("{location}: {redacted}: {reason}"));
        }
    }
```

`statement.sql` stays [`redact_literals`]'d at the broker and `redacted` is that
same text, so no literal reaches the line (`R12`). **Every interpolated component
is sanitised before it is formatted** (`R19`): a local `escape_control` in
`audit_v1` — the same rule as `blastit_manifest::error::escape_control`
(`pub(crate)` there, so it is re-stated, not exported): every byte below `0x20`
and `0x7f` becomes `U+FFFD` — applied to the route name, the proxy name and the
reason, and the whole line capped at 2048 bytes with the **SQL tail** truncated
(the location and cause never are). The caps are 64 bytes for route and proxy,
256 for the reason, and the remainder for the SQL. That matters because
`plan_learn_json` deserialises arbitrary JSONL, so `proxy` is attacker-shaped on
the bridge path; `apply_edits`' `UnknownProxy` guard (`edit.rs:551-556`) covers the
write, not the log line. The `…`-in-`Query` safety net below reuses the same
helper for its label. Then the grouping, unchanged except for the attribution:

```rust
    let mut hints: BTreeSet<String> = BTreeSet::new();
    let mut grouped: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for statement in observations.sql() {
        let Some(route) = learn_route(&table, &plan, manifest, &statement.target) else {
            continue;
        };
        if statement.is_tx {
            hints.insert(format!(
                "transaction control is not granted automatically: set \
                 transaction_control = true inside [[route]] name = \"{route}\", in \
                 its [route.{}] table (also granted by the audit's \
                 set_transaction_control fix)",
                statement.proxy
            ));
            continue;
        }
        if let ObsVerdict::Query { normalized } = &statement.verdict {
            if normalized.contains('\u{2026}') {
                // A redaction is never grantable, whatever a broken verdict
                // says (`R24`, `R33`): the plan stays unwritable. The wire text
                // is not echoed at all — a verdict that still carries a `…` may
                // equally carry a literal beside it, and this line must never be
                // the path that prints one (`R12`, `R19`).
                refused.insert(format!(
                    "route \"{route}\" [route.{}]: a redacted statement",
                    statement.proxy
                ));
            } else {
                grouped
                    .entry((route, statement.proxy.clone()))
                    .or_default()
                    .insert(normalized.clone());
            }
        }
    }
```

### 5.3 The helper (`audit_v1.rs`, beside `granted_already`)

```rust
/// The route one statement's grant belongs to (`R1`, `R3`).
///
/// A [`ObsTarget::Routed`] statement already names the route blastit's routing
/// table matched — the same name [`evaluate`] attributes a `missing_grant` to,
/// so the grant lands where the next audit looks for it. An
/// [`ObsTarget::Unrouted`] statement belongs to a route when the table matches
/// it after all (the Python gate cannot route, so [`plan_learn_json`] sees raw
/// targets) or when [`infer`] proposes one for its path.
///
/// A name the manifest does not declare is skipped, never returned: an edit
/// for an undeclared route fails the whole write with
/// [`EditError::MissingRoute`](blastit_manifest::edit::EditError::MissingRoute),
/// which would make one stale observation cost every other grant.
fn learn_route(
    table: &RoutingTable,
    plan: &RoutePlan,
    manifest: &Manifest,
    target: &ObsTarget,
) -> Option<String> {
    match target {
        ObsTarget::Routed { route } => manifest
            .routes()
            .iter()
            .any(|declared| declared.name() == route)
            .then(|| route.clone()),
        ObsTarget::Unrouted { method, path } => match table.route(method, path) {
            Some(matched) => manifest
                .route(matched.route)
                .map(|route| route.name().to_string()),
            None => plan.by_path.get(path).cloned(),
        },
    }
}
```

with `use crate::infer_routes::{RoutePlan, infer};` replacing
`use crate::infer_routes::infer;` (`audit_v1.rs:65`).

The de-duplication block (`:1387-1417`) is **unchanged**: `already` is found by
name, and `granted_already` already covers a named existing route (R5).

### 5.4 The editor extends a list instead of folding it (`crates/blastit-manifest/src/edit.rs`)

`patch_grant` (`:788`) currently folds a multi-line `queries` list onto one line
(`:859-888`, tested by `edit_branches.rs:77`). That is acceptable while only
`dev --learn` writes; under (a) it is the common path, and it breaks the step
`ci.md` asks the reader to take — *"Review `git diff` before committing"*: a list
of forty statements collapses into one 4 KB line, and any comment inside the list
is destroyed silently (`verify`, `:976-1000`, compares the model, so no text
outside the edited field is checked).

**Change:** when the `queries` assignment spans more than one line, insert the
new entries as new lines and touch nothing else. The algorithm is exact:

1. `stop` is already computed (`:861-864`); `stop - 1` is the list's last line.
2. Split that line into text and trailing comment with the existing
   `trailing_comment` helper (the same one the one-line path uses, `:873-880`).
   Let `close` be the **last** `]` in the text — the list's closing bracket is
   the last structural token on the line, so a `]` inside an earlier string is
   always to its left. Everything before `close` is `head`, everything after it
   is `tail`.
3. If `head.trim()` is non-empty (the close shares its line with an element, the
   hand-wrapped `queries = ["a",\n  "b"]` shape): if `head` does not end with
   `,`, append one — appending to `head`, never to the raw line, so a trailing
   comment never receives it. Replace the line with `head` and insert a new line
   `{assignment_indent}]{tail}{comment}` immediately after it. The list now ends
   on a bracket-only line, and its comment travelled with the `]`.
4. Define an **element line** as a line whose first non-space byte is `"` or `'`
   (a `queries` entry is always a TOML string), plus the `assignment` line only
   when its text **before its trailing comment** (`trailing_comment` again) has a
   quote after the first `[`. This excludes `[` alone, `[ # …` (a comment may
   quote SQL — `queries = [  # "SELECT 2" is denied` must not count), `]`, and
   comment-only lines, which are the shapes that would otherwise receive a comma
   in the wrong place. Let `last` be the last element line. If it does not end
   with `,`, insert one at the end of the line's **text**, i.e. before its
   trailing comment and before its `\n` (`lines` come from
   `split_inclusive('\n')`, so appending to the `String` would land after the
   newline). This is the **only** edit to an existing element line, and R10
   states it.
5. If there is **no** element line (an empty list written across lines,
   `queries = [\n]`, or `queries =\n[\n]`, `was_empty == true`), skip step 4 —
   never a comma after `[`.
6. Insert one line per fresh entry, in the order of `patch.grant.fresh`, after
   `last` (or after `assignment` when there is none), using the indent of an
   element line when one exists, else the assignment's indent plus four spaces,
   each rendered `toml_basic_string(query)` + `,\n` — the same quoting
   `toml_string_array` uses.
7. Return. Every other line — entries, their quoting, interior comments, the
   closing bracket and its trailing comment — is byte-identical (plus the
   step-3 split when it applies).

One shape is deliberately **not** handled by this path: an element written as a
multi-line TOML string (`"""…"""` or `'''…'''`) spans lines, so "an element line"
and "insert after it" stop being well defined. Such a list keeps today's behaviour
— the fold — because the fold is safe for it (a comma appended inside a multi-line
string would not be), and leaf 1 pins that this is the **only** list shape that
still folds. A one-line list never reaches here (`append_to_array`).

The one-line path (`append_to_array`, `:1379`) is unchanged and keeps the
author's trailing comment (`edit_branches.rs:113`).

Nothing else in `edit.rs` moves: the inline-grant refusal (O4), the
`unrestricted` refusal, `normalise_queries`' literal refusal, and the
postcondition all stay as they are. An inline grant (`[route] db = { queries = […] }`)
stays a **named refusal**, not a failure the user cannot act on: `EditError::InlineGrant`
already says *"route X's grant on `db` is an inline table; write it as
`[route.db]`"* (`edit.rs:284-290`), so `check --learn` prints an actionable
sentence and writes nothing. R15 pins it.

R10 states the guarantee, and the test is the one `edit_branches.rs:77` becomes.

### 5.5 What does **not** change, and the lines that must

* `learn()`'s gates and exit codes (`CLEAN`/`FINDINGS`/`SUITE_FAILURE`) and
  `would_write` (`audit_v1.rs:1284-1289`).
* `LearnPlan::would_write` and `queries()` (`audit_v1.rs:1279-1299`).
  `queries()` sums every grant's entries, so it counts grants on existing routes
  for free.
* The refusal rule: one refused statement still blocks the whole write. Writing
  the grantable subset would be defensible, but `R29` pins the stricter rule and
  this node does not reopen it (§8 lists it as a follow-up observation).
* `dev_learn.rs`. Its `edits()` and `plan_learn` now implement one rule twice;
  unifying them is a mechanical follow-up, not a prerequisite — the two differ
  in ordering (dev keeps observation order, `plan_learn` sorts) and in the
  refusal gate, so a premature merge would change `dev`'s bytes.

**The document: one builder, two producers.** `check::learn` builds its own copy
of the `--learn --json` document (`check.rs:996-1002`) while `LearnPlan::json`
(`audit_v1.rs:1293-1299`) hard-codes `would_write`. They agree on success and
disagree exactly where it matters: `_blastit.plan_learn` never writes anything,
yet its document says `"wrote": true` for a writable plan, and it drops
`plan.refused` entirely. The fix keeps the **shape** (R8) and gives the one
builder a parameter:

```rust
/// The `--learn --json` document (`§1.3`): the proposal and whether this run
/// wrote it. `wrote` is the caller's fact, never `would_write` (`R13`, `R20`).
pub fn json(&self, wrote: bool) -> String {
    json!({
        "proposed": {"routes": self.routes(), "queries": self.queries()},
        "wrote": wrote,
    })
    .to_string()
}
```

`check::learn` passes the actual `wrote`. `plan_learn_json` — a proposal API that
never writes — passes `plan.would_write(dry_run)` and its rustdoc says so
(`:1438-1445`: "a proposal: nothing is written, and `wrote` is the decision
`blastit check --learn` would take"), and it prints the same `learn: refused …`
lines the CLI prints, so a bridge caller is not blind (`R20`).

Changing `json`'s parameter from `dry_run` to `wrote` keeps the arity, so it
compiles silently and leaves **five** call sites that must change in the same
leaf (not be weakened) — the bridge's own at `audit_v1.rs:1456` and the four
protected ones in `crates/blastit/tests/critical_check_engine.rs` at `:1111`,
`:1141`, `:1169` and `:1183`, all `plan.json(plan.would_write(false))`. Each is
mechanical and each keeps its meaning: `:1111` (a refused plan), `:1141` (an
empty plan) and `:1169` (a zero-request plan) assert `wrote == false`; `:1183`
(a writable plan) asserts `true`. Those five edits are authorised through
`tools/critical_guard.py --node <leaf 3>`, and a reviewer should see exactly four
changed call expressions in the protected file (plus the bridge's one) and no
changed assertion.

**The `learn:` line, because it is false today.** `learn_line` (`check.rs:1015-1033`)
prints `learn: N routes, M queries added` whenever the run is not a dry run —
including a run that wrote nothing because a statement was refused or the plan was
empty. With (a) that is the routine case: `5 queries added` beside `learn:
refused …` for a file that did not move. The line must agree with `wrote`:

| Outcome | Line |
|---|---|
| wrote | `learn: {routes} routes, {queries} queries added → {path}` |
| `--dry-run` | `learn: {routes} routes, {queries} queries proposed (dry-run) → {path}` |
| not clean (refused, zero requests, write failed) | `learn: {routes} routes, {queries} queries proposed (not written) → {path}`, **always printed to stderr — under `--json` too** (`R17`) |

**Every non-clean run must explain itself on stderr.** Today a suite that reaches
nothing produces exit `1`, `{"proposed":{"routes":0,"queries":0},"wrote":false}`
and **empty stderr** (`check.rs:991-1004`: `clean` is false, so `learn_line` is
suppressed; no refusal, no hint, no failure), which is indistinguishable from a
fixed point except by the exit code and has no actionable content. Add one
diagnostic, reusing the audit's own sentence:

* zero requests: `learn: nothing observed: the suite reached no route and no proxy (does DATABASE_URL point at blastit?)`
* any other not-clean run: the existing `learn: refused …` / `learn: <path>: …`
  lines, plus the "(not written)" line above.

The exit-code table then disambiguates the states without a new field (`R20`).
Read the first row that matches, with `clean = plan.requests > 0 &&
plan.refused.is_empty() && failure.is_none()` (`check.rs:991`) as the first key —
that is what makes a blocked `proposed 0,0` run exit `1`:

| `clean` | `--dry-run` | `proposed` | `wrote` | Exit | Meaning |
|---|---|---|---|---|---|
| no | any | any | `false` | `1` | blocked — stderr names the cause: a refusal, a write failure, or a suite that reached nothing (the zero-observation line) |
| yes | yes | any | `false` | `0` | a dry run — nothing was written; re-run without the flag |
| yes | no | `0,0` | `false` | `0` | fixed point — nothing left that `--learn` can write |
| yes | no | any | `true` | `0` | written; run `check` |

`check.rs:984-1004` is the authority: `clean = requests > 0 && refused.is_empty()
&& failure.is_none()`, so a blocked run is `FINDINGS` whatever `proposed` says,
while a dry run with something to propose is `CLEAN` with `wrote: false`.

This is a truthfulness and diagnosis fix, not a change to what is written or to
any exit code.

### 5.6 The pages after this decision (leaf 6 writes these; this node only quotes them)

`docs/ci.md`, in "Bootstrap an existing project", one clause in the sentence
that currently reads *"`--learn` discovers routes and statements"*:

> `--learn` discovers routes and the statements your routes ran — including
> statements on a route you already declared — while `app`, `preload` and the
> proxy's input names are yours to write.

and in the paragraph under the `--learn` console block, replacing *"writes exact
`queries` (never table grants)"*:

> writes exact `queries` into the route that ran the statement (never table
> grants), and prints a hint rather than writing `transaction_control`

one sentence added to the finding table's `missing_grant` row, because it is the
one finding `--learn` never clears:

> A route that ran `BEGIN`, `COMMIT`, `ROLLBACK` or `SAVEPOINT`/`RELEASE` needs
> `transaction_control = true` in its `[route.<proxy>]` table; `--learn` names the
> route and the table on stderr and never writes the flag for you.

and one sentence in the same block for the write gate, because a reader will meet
it the first time a statement inlines a value:

> One refused statement blocks the whole write: parameterise it and run again —
> `--learn` never writes a partial manifest.

Exit codes are `ci.md`'s existing table and do not change; the four states a
`--learn` reader has to tell apart are §5.5's table, and the page gains its last
row (the posted wording — note that a `--dry-run` writes nothing and is still
`0`, so the row must not say "wrote nothing ⇒ `1`"):

> `--learn` exits `0` for a fixed point or a `--dry-run`, and `1` with the reason
> on stderr when it was blocked (a refusal, a write failure, or a suite that
> reached nothing).

`docs/local.md` changes in one place. Its "Fill the manifest in one pass" block
already separates the two writers ("With a test suite, one command records the
whole app … Without a suite, click through the app instead"), and that stays; the
`check --learn` half gets the same transaction sentence as `ci.md`, because its
`# … clean` line at `:87` is unreachable for a psycopg3 default connection:

> A route that opens a transaction also needs `transaction_control = true`;
> `--learn` prints the hint and the `learn:` line says what it wrote.

`docs/reference/cli.md` `--learn`/`--dry-run` rows: the dry-run row currently
promises the proposal itself (`cli.md:49`), while the line prints two counts.
Either the row says "prints the proposal's counts, `wrote: false`, and writes
nothing" (**chosen** — the human dry-run output does not grow a statement list in
this milestone), or a statement list is added later; the record pins the former
so `ci.md`'s "print the proposal" phrase is not left standing.

`docs/reference/manifest.md` gains the `transaction_control` consequence in its
grant section (one sentence) — same text as the finding row above.

---

## 6. Acceptance

### 6.1 Fixture (CLI)

An app with one **existing** route and a partly-filled grant:

```toml
version = 1
app = "app:app"
[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }
[[route]]
name = "notes"
path = ['/notes/.*']
[route.db]
queries = ["SELECT 1"]
```

A suite that exercises the granted statement **and** two ungranted ones on that
route (`SELECT 2`, and `SELECT 3` on `/notes/42/`).

```console
$ uv run blastit check --learn
learn: 0 routes, 2 queries added → /…/blastit.toml
$ uv run blastit check --json | jq -e '.ok and (.findings | length == 0)'
$ uv run python -c 'import sys,tomllib; q=tomllib.load(open("blastit.toml","rb"))["route"][0]["db"]["queries"]; sys.exit(q != ["SELECT 1","SELECT 2","SELECT 3"])'
$ uv run blastit check --learn --dry-run --json
{"proposed": {"routes": 0, "queries": 0}, "wrote": false}
$ git diff --exit-code blastit.toml     # the fixed point (R6)
```

(The commands run through `uv`, as the pages show them; the second one is the
assertion that the list grew by exactly two entries **and kept** `SELECT 1` —
"the diff shows two lines" is not an oracle. With a commented, multi-line list
in the fixture, a third assertion pins the comment line's presence and the
file's line count.)

This block is run twice, in the two places the project already stages a suite:

* **Offline (the gate):** a new case in `python/tests/critical/test_check_contract.py`,
  which stages a stub `pytest` and a mock PostgreSQL server and needs no
  database — `--learn`'s writer and the audit it feeds. The file is protected, so
  the case is *added* under `tools/critical_guard.py --node <leaf>`; nothing in
  it is weakened.
* **Real (the `pytest-plugin` CI job):** the same fixture in
  `python/tests/test_check.py`, which is skipped without `DATABASE_URL` — so it
  is evidence in CI, never a local `make check` dependency.

"No findings" is scoped to what `--learn` writes: a suite that opens a
transaction leaves one `missing_grant` (`fix.action: set_transaction_control`
after R18) until the author applies that fix, which §7.4 rules on and §5.6
documents. The fixture above keeps the connection in autocommit so that the
headline claim is exact; a second fixture asserts the tx case separately (R14),
including the `fix.to` string the following `check --json` carries.

### 6.2 Bring-your-own-suite

The same fixture run through the project's own suite (`python/tests/test_check.py`'s
`run()` helper, which invokes `python -m blastit check` in the project
directory): first run writes, `--json` reports `ok: true` afterwards. The bridge
twin: `_blastit.plan_learn` on the same `http_obs`/`sql_obs` bytes must return
the same `proposed` **and** print the same `learn: refused` line as the CLI, with
`Routed` targets *and* with raw `Unrouted` targets for a path the manifest routes
(R3, R4, R20). Its `wrote` field is compared only where `dry_run` makes it
meaningful.

### 6.3 Plan-level oracles (`crates/blastit/tests/critical_learn_existing_route.rs`, new)

| Requirement | Oracle |
|---|---|
| R1, R2 | routed statements on an existing route → `queries() == 2`, `routes() == 0`; `apply_edits` then `evaluate` → `findings == []`, and `Manifest::parse_and_validate` accepts the edited text for **every** fixture route shape (plain app route, and `[route.static]` + app, which is the shape that can both carry a grant and serve files). A route with `spa = true` cannot reach the app and therefore cannot produce observations, so `--learn` can never add a grant to it — the `spa` + global-`group` boot shape is unreachable from this path, not untested |
| R3 | `Routed { route: "nosuchroute" }` → `queries() == 0` and no `EditError::MissingRoute` when the plan is applied; `Unrouted` for a declared route → the grant lands on that route |
| R4 | `Unrouted` statements whose path the manifest routes → `routes() == 0`, no `notes-2` |
| R5 | a manifest using `tables`, a `group`, and `unrestricted = true` → `queries() == 0` in all three; `apply_edits` is a byte-identical no-op |
| R6 | `plan_learn` on the edited manifest → `would_write(false) == false`, and the CLI run exits `0` (the CLI half lives in §6.1's last two commands) |
| R7 | one refused statement + three routed grantable ones → `would_write == false`, `refused.len() == 1` |
| R8 | the JSON document's key set is exactly `proposed`/`wrote`, `proposed` exactly `routes`/`queries`; `check::learn` and `plan_learn_json` build it from the same function (R20) |
| R10 | a multi-line list with an interior comment and a missing trailing comma → the comment line, the entry lines and their quoting are byte-identical, one new line per entry, and the document parses; a one-line list keeps its trailing comment (`edit_branches.rs:113`) |
| R11 | a routed `is_tx` statement → the hint contains the route's name and `[route.db]`, and contains no `[route.<name>.` |
| R13, R17 | `learn_line` on a dry run says `proposed (dry-run)`; on a refused plan it says `proposed (not written)`; on a write it says `added`; a zero-observation plan prints `learn: nothing observed: …` to stderr **under `--json` too** (the call site in `check.rs`, not `learn_line` alone) |
| R14 | a routed `is_tx` statement plus one grantable one → `would_write == true`, `queries() == 1`, hints `== 1` |
| R18 | `evaluate` on a routed `is_tx` denial → `fix.action == "set_transaction_control"`, `fix.to == "[route.db].transaction_control"`, and the follow-up `check` after applying it is `ok` |
| R19 | a refused statement on a declared route → the refusal line contains `route "notes"` and `[route.db]`; on an unresolvable target it contains `proxy "db"` and no route name |

### 6.4 Existing suites

`critical_check_engine.rs`'s golden R27 case uses `unrouted_sql` only
(`:284-291`), so its counts (`routes == 1`, `queries == 2`) are unchanged;
`critical_learn_executor_blocks.rs` already proves the existing-route write path
through `dev --learn` (`:9-19`) and must keep passing untouched;
`python/tests/test_check.py`'s two `--learn` tests (`:239`, `:253`) are
unaffected (they learn a new route).

Two protected files gain coverage under `tools/critical_guard.py --node <leaf>` —
`python/tests/critical/test_check_contract.py` (the offline fixture) and
`crates/blastit-py/tests/critical_check_schema.rs` **only if** the lead wants the
bridge pinned there (`plan_learn_json` is `pub`, so the new Rust suite can pin it
without touching that file). Two more belong to later mismatch leaves, not to this
feature's write path (§11): `critical_static_resolve.rs` gains an `**` vector
(mismatch 6), and `crates/blastit/tests/critical_dev_and_fail_closed.rs` gains the
boot cases for R16 and R21 (leaves 9 and 8), both add-only. One protected file
gains two named repairs, not weakenings: `critical_check_engine.rs`'s
**four** `plan.json(false)` call expressions (`:1111`, `:1141`, `:1169`, `:1183`)
for R8 — all become `plan.json(plan.would_write(false))`, no assertion changed —
and its savepoint guard (R18). Every other old assertion in those files must stay
byte-identical, and `make rs-test-critical` plus `python/tests/critical` are the
gate.

---

## 7. Mismatch Rulings (`MB.3.19`'s inventory)

| # | Claim | Ruling |
|---|---|---|
| 1 | `php.md` taught a nonexistent `check -- <suite>` and `BLASTIT_URL` | **Docs were wrong, `MB.3.19` fixed them correctly.** Settled — do not reopen. |
| 2 | `check --learn` cannot extend an existing route | **Product gap → decided in §3.** The seam is `audit_v1.rs:1360-1362`; O1-O3 are the corrections to the lead's reading. The one residue `--learn` deliberately cannot clear is transaction control (§7.4). |
| 3 | all-static + `app` is documented as a boot error but the code boots | **Code bug (asymmetry), and the page encodes it.** §7.1. |
| 4 | `allow_raw` is a warning/finding, not a gate | **Docs accurate about today; the gate changes.** `allow_raw` has no boot or request-time reader, so `Production` serves the raw surface with the key absent — a Stage-0 error in `Production` replaces the warning. §7.2. |
| 5 | every symlink under `static.root` is 404 | **Behaviour, documented, correct.** `STATIC_RESOLVE_FLAGS` carries `RESOLVE_NO_SYMLINKS` (`static_asset.rs:70-73`), a symlink is `Hidden` → `404`, and fail-closed traversal is worth more than serving a link. Settled. |
| 6 | `immutable` globs do not cross `/` | **Code gap (DX bug).** §7.3. |
| 7 | three published anchors vanished; `MB.3.19` bridged them with ids | **Docs plumbing, fixed.** Settled; retarget the bridges when `MB.18`/`MB.11` touch those pages. |

### 7.1 Mismatch 3 — "all-static + `app` is a boot error"

The page says ([`manifest.md`](../reference/manifest.md) lines 20-22): *"An
all-static manifest needs no `app` … Setting `app` or `BLASTIT_APP` there is a
boot error, because no route could reach it."*

What the code does:

* `is_all_static()` (`model.rs:717-726`) is `app.is_none() && !routes.is_empty()
  && every route static && no php`. So a manifest that sets `app` is **not**
  all-static by construction, `static_app_refusal` (`stage0.rs:295-302`) never
  fires, and `app_rule` (`:315-328`) accepts it. The manifest key boots.
* `BLASTIT_APP` with no manifest `app` **is** refused, with the message *"every
  route is static, so no route can reach an app"*.
* That message is false whenever a static route can forward a miss: a
  `[route.static]` route without `spa = true` forwards a path it does not answer
  to the app (`manifest.md:135`; `StaticOutcome::Missing`). The two spellings of
  the same runtime therefore disagree, which is exactly the class of lie this
  project forbids.

So the sentence is right about `BLASTIT_APP` and wrong about `app`; the *rule*
is wrong about both when a static route can forward. The scheduler already
encodes the correct predicate when it builds the route list —
`static_forward = policy.is_some() && !policy.spa && forwards`, with
`forwards = has_app || route.php().is_some()` (`scheduler/src/route.rs:76-84`) —
so the responder and the boot disagree today. `MB.3.19`'s "the code boots" is
right and its conclusion ("or the sentence is wrong") is the closer one — but
neither spelling is defensible while they differ.

**Fix (a `blastit` crate bug, not this node's):** replace `is_all_static()` in
`static_app_refusal` with "no route can reach an app":

```rust
/// Whether every route answers without the WSGI app: a `[route.php]` route
/// forwards to PHP, a `[route.static]` route forwards a miss unless `spa = true`,
/// and a route with no executor block is the app's own.
fn no_route_reaches_the_app(manifest: &Manifest) -> bool {
    !manifest.routes().is_empty()
        && manifest
            .routes()
            .iter()
            .all(|route| match (route.static_policy(), route.php()) {
                // A route with no executor block *is* the app's way in.
                (None, None) => false,
                // A static route forwards a path it does not answer to the app,
                // unless `spa = true` answers every miss itself.
                (Some(policy), None) => policy.spa,
                // `[route.php]` is its own executor; a static miss forwards to PHP.
                _ => true,
            })
}
```

and apply it to `config.app` whatever its source, so `app = …` and
`BLASTIT_APP=…` are one rule (both a dead declaration → refused, matching
`StaticDeadDeclaration`'s treatment of `[route.ingress]` on the same route,
`validate.rs:498-545`). (R21.) Precisely, and this is where a careless fix breaks
something: `is_all_static()` stays as it is — it answers "does the manifest
declare an app", which is what `check`'s all-static short-circuit
(`check.rs:173-185`) and the dead-declaration rule need — and `app_rule`'s second
half (`!can_boot_without_app() && app.is_none()`, `stage0.rs:322-325`) is
untouched. Only the **refusal of a declared-but-unreachable app** changes
predicate. It is called from the production boot (`stage0`, where `app_rule`
already is), from `stage1`'s re-read (`stage1.rs:254-262`, the same TOCTOU as
R16), and from `check` (`check.rs:146-150`). A manifest with **no** app and
all-static routes is unaffected: the guard is `config.app.is_some()`. Then the
page's sentence becomes:

> A manifest whose every route answers without the app needs no `app`, and an
> `app`/`BLASTIT_APP` no route could reach is a boot error rather than a silent
> setting. A `[route.static]` route without `spa = true` still forwards a path it
> does not answer, so `app` beside it is meaningful.

**Owner:** a follow-up leaf (`crates/blastit/src/{stage0,stage1,check}.rs`,
`crates/blastit-manifest/src/model.rs` if the predicate lives there; test in
`critical_dev_and_fail_closed.rs` or a new `critical_*.rs`). **Acceptance:** every
route shape × {`app` in the manifest, `BLASTIT_APP` in the env, neither} × {boot,
`check`} gives one answer — five route shapes (pure WSGI, static non-`spa`, static
`spa`, static + `[route.php]`, `[route.php]` only), six cases each. **Not this
milestone's release blocker:** today the failure is a refused-but-valid
configuration (annoying) or an accepted-but-dead one (silent), never an unsound
one. It must not ship as a *docs* change, because the docs would then certify the
asymmetry.

### 7.2 Mismatch 4 — `allow_raw` is a warning, not a gate

Verified behaviour:

* `body = "raw"` produces `ManifestWarning::RawIngress` at boot **whether or not**
  `allow_raw` is set (`validate.rs:811-820`), and a boot prints its warnings as
  `manifest_warning` events (`stage0.rs:1893`, `:2247`).
* `blastit check` fails with the `raw_ingress` finding (severity `error`) when
  `body = "raw"` and `allow_raw` is absent (`audit_v1.rs:1059-1070`).
* **Nothing consults `allow_raw` at request time**: `plan_body`
  (`blastit-ingress/src/lib.rs:284`) streams `BodyCodec::Raw` unchanged, and the
  only `allow_raw` reads outside tests are the warning, the finding, the fix
  suggestion, and the scheduled policy struct that carries it.

Ruling: **the page is accurate about today's behaviour, and today's behaviour is
too weak for production — change it.** `allow_raw` is not a runtime gate, so in
`Production` a `body = "raw"` route whose key is absent serves the raw bytes
anyway: `php2shell`/`node2shell` payloads reach the app's own parser with nothing
between the socket and the runtime. Three facts make the warning-only stance
indefensible rather than merely debatable:

* nothing reads `allow_raw` at boot or per request, so the "acknowledgement" has
  no moment of truth;
* `check`'s own `suggest_fix` sets `allow_raw: true` for the reader
  (`blastit-ingress/src/lib.rs:440`), so the key is a rubber stamp the tool fills
  in, not a decision the author takes; and
* no preset, unit, or image entry point runs `check`, so the documented
  checklist item is the only place the stamp is demanded.

**Decision:** on the **production path** — the boot where `dev.is_none()`. In this
codebase that is the flag distinguishing `app_rule`'s two call sites in
`stage0.rs` (`:1869` for the serve path, `:2142` for `dev`; the rule itself takes
`config`/`manifest` and must gain the flag or a sibling that is only called at
`:1869`), plus `stage1.rs:254-262` where the manifest is re-read from disk — a
route declaring `body = "raw"` without `allow_raw = true` is a **boot error**
naming every offending route and the key. `blastit dev` keeps the warning, and
`blastit check` keeps the `raw_ingress` finding (severity `error`). (R16.) The
stage-1 point is not belt-and-braces: `stage0` validates and `stage1` re-reads the
same path, and `config` carries no digest of what stage 0 approved, so a file
swapped between them would otherwise serve. This amends `ADR-0006`'s decision 3;
the amendment is already recorded in
`docs/decisions/ADR-0011-raw-ingress-boot-refusal.md`, created by `MB.3.53`, which
owns the implementation — this section is the handover, not the ADR.

Why this is safe and cheap: production is the only place the opt-out is live, the
manifest is the trust root, the change cannot surprise a manifest that passed
`check` (which already fails without the key), and it turns "fails closed" into
the truth for the one ingress declaration that has no runtime gate at all. It is
*not* the same as making `unrestricted` an error: that key is a SQL grant whose
breadth is already bounded by the broker's parser and by per-route sockets, while
this one governs whether inbound bytes are transcoded at all.

What would still be missing afterwards (a later milestone, not this one): the
`raw` codec has no body-size cap of its own, so the ack still buys streaming
rather than validation. The honest sentence for `manifest.md` stays "the bytes
unchanged — the loud opt-out, flagged at boot" plus, for production, "refused
without the acknowledgement". And `suggest_fix` (`blastit-ingress/src/lib.rs:426-441`)
still chooses `raw` + `allow_raw = true` for an unknown or truncated sample; that
is a *fix suggestion*, not this gate, and a follow-up should prefer `observe` over
inventing an opt-out (out of this node's scope, recorded in §13).

### 7.3 Mismatch 6 — `immutable` globs and `/`

`ImmutableGlob::matches` calls `glob` (`static_asset.rs:207-226`), where `*` is
the only metacharacter and it stops at `/` (`:218-220`). So
`immutable = ["/assets/*"]` matches `/assets/app.css` and **not**
`/assets/app/app.css`; `**` is two `*` and behaves as one `*`. Django's static
layout is `static/<app>/<file>` and Vite emits `assets/index-<hash>.js` plus
`assets/chunk-<hash>.js` — one is one level, the other may be two. A pattern
that silently matches nothing costs the developer a debugging session and gives
no signal: validation accepts it, boot does not warn, `check` does not report it.

Ruling: **code gap, fix it.** Support `**` in the same matcher as "any run of
bytes including `/`", leaving `*`'s semantics untouched. The matcher must be
**iterative**: the obvious recursive `**` (try every suffix) is exponential in the
number of `**` groups, and this function runs on every static answer — a manifest
pattern is trusted, but a quadratic blow-up on a 4 KiB path is still a
self-inflicted denial of service. A classic two-row DP is O(pattern × path) time,
O(path) space, and is bounded by the ceilings that already exist: 32 patterns per
route (`MAX_PATTERNS`, `validate.rs:52`), 256 bytes per pattern
(`MAX_IMMUTABLE_PATTERN_BYTES`, `static_asset.rs:56`), 4096 bytes per path
(`MAX_STATIC_PATH_BYTES`, `:42`). A worst-case 404 therefore costs at most
32 × 256 × 4096 ≈ 33.5 M byte steps for the whole route — not ~1 M per pattern —
and an early exit on an all-false row makes even that rare. No recursion, no
backtracking:

```rust
/// One pattern element: `*` stops at `/`, `**` does not.
#[derive(Clone, Copy, Debug)]
enum Tok {
    Star,
    StarStar,
    Byte(u8),
}

/// The pattern, compiled once at construction (`MAX_IMMUTABLE_PATTERN_BYTES`).
/// Keeps today's derives: `StaticRoot` is `Debug` and holds these.
#[derive(Clone, Debug)]
pub struct ImmutableGlob {
    tokens: Vec<Tok>,
}

impl ImmutableGlob {
    /// `new` keeps today's checks (non-empty, `/`-anchored, ≤ 256 bytes, no
    /// control byte, no `..` segment) and compiles the pattern once.
    pub fn new(pattern: &str) -> Option<Self> {
        // … the existing checks unchanged …
        Some(Self {
            tokens: Self::tokens(pattern.as_bytes()),
        })
    }

    /// Splits the pattern into tokens; `**` is one token, never two `*`.
    fn tokens(pattern: &[u8]) -> Vec<Tok> {
        let mut out = Vec::with_capacity(pattern.len());
        let mut index = 0;
        while index < pattern.len() {
            match pattern[index] {
                b'*' if pattern.get(index + 1) == Some(&b'*') => {
                    out.push(Tok::StarStar);
                    index += 2;
                }
                b'*' => {
                    out.push(Tok::Star);
                    index += 1;
                }
                byte => {
                    out.push(Tok::Byte(byte));
                    index += 1;
                }
            }
        }
        out
    }

    /// `previous[j]` is "the tokens so far match `path[..j]`". O(tokens × path)
    /// time, two rows of `path.len()+1` bits, no recursion.
    #[allow(clippy::needless_range_loop)] // an index into two rows and the path
    pub fn matches(&self, url_path: &str) -> bool {
        let path = url_path.as_bytes();
        let width = path.len() + 1;
        let mut previous = vec![false; width];
        let mut current = vec![false; width];
        previous[0] = true;
        for token in &self.tokens {
            current.fill(false);
            match token {
                Tok::StarStar => {
                    current[0] = previous[0];
                    for j in 1..width {
                        current[j] = previous[j] || current[j - 1];
                    }
                }
                Tok::Star => {
                    current[0] = previous[0];
                    for j in 1..width {
                        current[j] = previous[j] || (current[j - 1] && path[j - 1] != b'/');
                    }
                }
                Tok::Byte(byte) => {
                    for j in 1..width {
                        current[j] = previous[j - 1] && path[j - 1] == *byte;
                    }
                }
            }
            if !current.iter().any(|&cell| cell) {
                // No live state: no extension can match, so stop early.
                return false;
            }
            std::mem::swap(&mut previous, &mut current);
        }
        previous[path.len()]
    }
}
```

The `Tok::Star` recurrence is the existing rule (`*` may be empty, and it stops
before a `/`), and `Tok::StarStar` is the same recurrence with the `/` test
removed. Tokens are compiled once at the root's construction, the two rows are
allocated once per call — two `path.len()+1` byte vectors, ≈ 8 KiB for a 4 KiB
path (`Vec<bool>` stores one byte per element, not one bit) — and refilled in
place, with an early exit after each row when no cell is set (`current` all
false ⇒ the match is dead), so a six-`**` pathological pattern costs the same as
any other pattern of its length and no per-token allocation happens. The old free
`fn glob` is **deleted** (`ImmutableGlob::matches` was its only caller), its
stale doc comment ("`*` is the only metacharacter") goes, and the struct doc at
`static_asset.rs:172-178` is rewritten to describe both metacharacters.
`ImmutableGlob`'s `pattern: String` field is private and read only by `matches`,
so replacing it with `tokens` touches no other file (`critical_static_resolve.rs`
constructs globs through `ImmutableGlob::new`). Hand-checked cases:
`/assets/*` vs `/assets/a/b` (false), `/assets/**` vs `/assets/a/b` (true),
`/**` vs `/` (true), `/**/x` vs `/a/b/x` (true), `/*/x` vs `/a/b/x` (false),
`/a/**` vs `/a/` (true).

No new manifest key; `**` is accepted by validation already
(`check_static_immutable`, `validate.rs:453`, treats a pattern as an anchored
string). **Owner:** a follow-up leaf in the static work
(`crates/blastit-scheduler/src/static_asset.rs` + a critical test), whose docs
half adds two things to `manifest.md`'s `immutable` row: "`**` crosses `/`" and,
as the honest interim for today's matcher, "a pattern that matches no file is not
reported". **Not a release blocker:** one pattern per depth works today and the
page states `*`'s rule accurately; the defect is ergonomics, not safety — but the
*implementation* must not be the recursive one this record first proposed.

### 7.4 Mismatch 2's residue — the one finding `--learn` never clears

`evaluate` reports a transaction-control denial as
`MissingGrant { severity: Error, fix: observe }`, with the statement shown as the
bare label (`BEGIN`, `SAVEPOINT`) (`audit_v1.rs:675-680`,
`transaction_label`). `plan_learn` turns the same observation into a **hint**,
never a grant (`:1366-1372`), because `transaction_control = true` is a
capability the author did not declare and `R27`/`ADR-0003` forbid inventing one.
So a suite that opens an explicit transaction leaves `check` red after a
successful `--learn` — the single place where "one run → clean" does not hold.

Ruling: **keep the hint** (this node does not grant capabilities), and remove the
three things that make it a trap:

1. **The hint must name the route** (R11) — and say which block, not just the
   table header, because `[route.db]` appended at the end of a file binds to the
   *last* `[[route]]`. The hint now reads `inside [[route]] name = "notes", in its
   [route.db] table`.
2. **The CI page must say it** (§5.6), because the finding table currently maps
   every `missing_grant` to `queries`.
3. **The audit must offer the fix, not `observe`** (R18): a transaction-control
   denial carries `Fix::set_transaction_control` with
   `to: "[route.<proxy>].transaction_control"`, so the documented `check --json`
   loop has a machine-readable remedy instead of prose from a previous process.
   `to` is relative to the finding's own `route` field — the same convention
   `add_query` and `add_group` already use (`docs/ci.md`'s JSON example) — and the
   human hint keeps the block sentence, because a locator string appended at EOF
   binds to the last `[[route]]` in the file. That closes the only place where
   `unrestricted = true` was the shortest green edit.

The `--learn` run itself stays exit `0` (it wrote everything grantable; R14), and
the next `check` names the remaining statement with a `to` the reader can apply.
The loop terminates in one edit that both the hint and the document name.

`Fix::set_transaction_control` is an **additive** `fix.action` value: no key
changes, every existing action keeps its meaning, and the CI page's list grows by
one row. `ADR-0004` owns that contract, so the leaf records the addition there
(reasoning: a consumer that does not know the value falls back to its `observe`
handling, which is what it does today; a `schema_version` bump would break every
consumer for a strictly more informative document).

**Follow-up candidate (not this milestone):** should `--learn` write
`transaction_control = true` for a route the wire showed running `BEGIN`? It is
narrow (it enables transaction statements, not arbitrary SQL), it is observed
rather than guessed, and it would complete the single-pass claim for psycopg3's
default (non-autocommit) mode. It needs its own ADR because it changes what
`--learn` is allowed to write.

---

## 8. What Would Make This Decision Wrong

1. **If `Routed { route }` could name a route the audit does not attribute the
   finding to.** It cannot: both read the same `RoutingTable` and the same
   manifest (`check.rs:912-943`, `audit_v1.rs:637-643`). If a future refactor
   splits them, (a) starts writing grants beside the finding and R2 fails
   loudly in the fixture (§6.1 step 3), which is why that step is the oracle.
2. **If `apply_edits` could not patch an existing route's grant without
   rewriting the route.** It can, and `critical_learn_executor_blocks.rs` proves
   it through `dev --learn` today. If that test were ever weakened, (a) would
   start damaging `[route.static]`/`[route.php]` blocks — `MB.3.21`'s regression.
3. **If a single-pass write could launder a statement the developer should have
   parameterised.** It cannot: `normalise_queries` refuses a redacted or
   literal-bearing statement (`edit.rs:652-684`) and `plan_learn` refuses the
   whole write when any statement was refused (R7). If `--learn` ever wrote a
   literal, the grant would be a credential-shaped leak in a committed file.
4. **If the vision's "one pass" meant "one pass of the whole app including
   routes no suite touches".** It does not: `--learn` has always been
   suite-scoped or dev-scoped, and (a) does not claim coverage a suite does not
   have — `unexercised_route` still fires.
5. **If `dev --learn` is the *intended* single writer** because a served app is
   the only honest observation. It is not: `check` observes the same wire at the
   same brokers (`check.rs` §"Why the gate reports unrouted requests"), and the
   vision's CI sentence is the one that promises the fill.
6. **If mismatch 4's ruling is wrong in the other direction** — i.e. an
   unacknowledged raw route is *deliberately* tolerated in production because the
   deploy checklist is trusted — then the boot error is one step too many for
   someone who removed `allow_raw` on purpose. Nothing in the repository reads
   the key at runtime, so the error adds no capability; the cost is one manifest
   line that `check` already demanded.

---

## 9. Alternatives Considered

| Alternative | Why not |
|---|---|
| (b) amend `VISION.md`, make `dev --learn` the only grant writer | Needs a served server and a human (or a browser-driving agent) per route the suite already exercised; leaves agents with hand-editing or `unrestricted = true`; leaves `dev_learn.rs:479-574` as the only correct implementation of the rule. |
| Document (a) while the code does (b) | The current state. A concession paragraph is not a design. |
| Have `check --learn` emit a paste-ready patch instead of writing | (b) with worse ergonomics: the diff still has to be applied by hand. |
| Have `plan_learn` write a `tables`/`group` grant covering the statement | Widens what the route may run to save one line; `R27`/`ADR-0003` forbid inventing a table grant. |
| Run the suite twice inside `--learn` (once to discover, once to write) | The second run adds nothing the first did not observe; doubles CI time for a write that happens after pytest exits. |
| Keep `check` read-only and add a fourth command (`blastit learn`) | A second spelling of one command; the CLI surface grows to fix a filter. |

---

## 10. Security & CVE Analysis

The change is a **narrowing** of what the tool does to the manifest, not a
widening of the trust boundary:

* **No privilege path is touched.** `check` stays unprivileged, in `Dev`/`Check`
  mode, with the `ADR-0001` simulation delta; the write is one `write_atomic` to
  the manifest path the user named (`edit.rs:385-441`, `O_CREAT|O_EXCL` +
  `rename`, mode preserved, temp removed on every failure path).
* **Grant text only grows by what the wire showed.** Every entry is a
  `LearnVerdict::Query { normalized }` — `$?` placeholders, no literal
  (`redact_literals` at the broker, `blastit-dbproxy/src/learn.rs` §Redaction),
  and `normalise_queries` refuses any quoted literal or `…` that slips through
  (`edit.rs:652-684`). A learnt manifest therefore cannot carry a credential or
  a literal into `blastit.toml`.
* **`unrestricted` is never approached.** `is_query_covered_by_tables_or_groups`
  returns true for an unrestricted grant (`model.rs:929-931`), so nothing is
  proposed and `EditError::UnrestrictedGrant` (`edit.rs:581-586`) stays
  unreachable from this path — a permanent negative test (R5). The one place an
  author could be tempted by `unrestricted` is a route's transaction control, and
  R18 gives the `--json` loop a narrower fix to reach for instead.
* **No new CVE surface from this change.** No byte is parsed differently, no
  proxy decision changes, `evaluate` and the request path are untouched. The one
  security-adjacent ruling is §7.2 (`allow_raw`), which now *closes* a hole
  rather than leaving it: production refuses a raw route whose acknowledgement is
  missing (R16, `ADR-0011`).
* **Fail closed on the unknown.** A `Routed` name the manifest does not declare
  is skipped, not invented (R3); an inline grant table is refused by name rather
  than duplicated, and a redacted statement makes the plan unwritable (R7).

---

## 11. Validation & Critical Tests Plan

| Requirement | Unit / integration | Critical (protected) | Runnable docs / CLI |
|---|---|---|---|
| R1, R2, R9 | `audit_v1` unit test beside `granted_already` | new `crates/blastit/tests/critical_learn_existing_route.rs` | §6.1 console block |
| R3, R4 | same file, `Routed`-unknown and `Unrouted`-matched cases | same new file (the bridge twin calls `audit_v1::plan_learn_json`, which is `pub`) | — |
| R5 | same file, three de-dup shapes | same new file | — |
| R6 | same file, fixed point | same new file | §6.1 `--dry-run` + `git diff --exit-code` |
| R7, R8 | same file, refusal gate + key set | `critical_check_engine.rs:1100-1215` refuses/zero-request cases stay green; its **four** `plan.json(false)` calls (`:1111`, `:1141`, `:1169`, `:1183`) take the new parameter, same assertions (authorised) | — |
| R10, R15 | `crates/blastit-manifest/tests/edit_branches.rs` (unprotected) | — | — |
| R11, R14 | `audit_v1` unit test on the hint text | new case in the new Rust suite | — |
| R12 | `edit.rs`'s existing literal/redaction tests stay green | new suite asserts a refused literal never reaches the write | — |
| R13, R17 | `check.rs`'s `learn_line` unit test plus a zero-observation CLI case | `python/tests/critical/test_check_contract.py` (add-only) | §6.1 |
| R16 | `stage0.rs`'s boot-rule tests | new `crates/blastit/tests/critical_dev_and_fail_closed.rs` case: the production boot and stage 1 both refuse raw-without-ack naming every offending route and the key; `dev` warns; `critical_manifest_011.rs:459` (the warning assertion) stays green unmodified | — |
| R18 | — | `critical_check_engine.rs:505-515` changes the **loop body** at `:509-511`: the guard `if sql.contains('…') \|\| sql == "SAVEPOINT"` splits into two arms — a redacted statement keeps `observe`/`None` (unchanged), `SAVEPOINT` asserts `set_transaction_control` and its `to` (the change). Authorised; the review must see no other change in that file | `docs/ci.md`'s action list |
| R19 | `audit_v1` unit test on the refusal text (route, proxy, control bytes, 2048 cap) | — | — |
| R20 | `audit_v1`'s `plan_learn_json` test via the new Rust suite | same | — |
| R21 | `stage0.rs`'s existing `app_rule` tests | new cases for the five route shapes × source × {boot, `check`} | `docs/reference/manifest.md` sentence (§7.1) |
| Whole-fixture | `python/tests/test_check.py` (`pytest-plugin` CI job, needs `DATABASE_URL`) | `python/tests/critical/test_check_contract.py`, new case (add-only, authorise) | `make docs-test` runs `ci.md`'s and `local.md`'s fences |

`make check` (which reaches `make rs-test-critical`) plus `make docs-check` are
the gate. Five protected files are touched, each under
`tools/critical_guard.py --node <leaf>`: `critical_check_engine.rs` (R8's four
call expressions, R18's one loop body), `python/tests/critical/test_check_contract.py`
(the offline fixture case, add-only), `crates/blastit-scheduler/tests/critical_static_resolve.rs:750`
(the `**` vectors, add-only), `crates/blastit/tests/critical_dev_and_fail_closed.rs`
(the R16 and R21 boot cases, add-only — leaves 9 and 8), and optionally
`crates/blastit-py/tests/critical_check_schema.rs` (the bridge trait, if the lead
pins it there instead of the new Rust suite).

---

## 12. Implementation Plan (ordered leaves; the lead creates them)

| # | Leaf | Paths | Acceptance |
|---|---|---|---|
| 1 | **Extend, don't fold** — `patch_grant` inserts element lines and splits a shared closing bracket (R10) | `crates/blastit-manifest/src/edit.rs`, `crates/blastit-manifest/tests/edit_branches.rs` (the fold test at `:77` becomes the extend test; `:501`, the file that ends without a trailing newline, must stay green) | interior comments and entry quoting byte-identical; the four shapes (trailing comma, none, empty, hand-wrapped close) each extend and re-parse; `critical_manifest_edit.rs` untouched and green |
| 2 | **Attribution** — `learn_route`, the `paths` guard, located refusals, the tx hint, the honest `learn:` line (R1-R4, R11, R13, R17, R19) | `crates/blastit/src/audit_v1.rs` (also the `plan_learn` rustdoc at `:1312-1324` and the module doc at `:34-37`, which say "only `Unrouted`"), `crates/blastit/src/check.rs` (`learn_line` + the not-clean stderr diagnostics) | plan-level tests; `critical_check_engine.rs`'s golden case and its refusal/zero-request cases unchanged |
| 3 | **One document, two producers** — `LearnPlan::json(wrote)`, refusals printed by the bridge (R8, R20) | `crates/blastit/src/audit_v1.rs`, `crates/blastit/src/check.rs` (`learn` calls it), `crates/blastit-py/src/check.rs` if the signature is mirrored, **and the four `plan.json(false)` call expressions in `crates/blastit/tests/critical_check_engine.rs` (`:1111`, `:1141`, `:1169`, `:1183`), all `plan.json(plan.would_write(false))`, under `critical_guard.py --node <leaf 3>`** | raw `Unrouted` bytes for a routed path propose `0` routes and the same `proposed` as the resolved CLI path; the bridge prints the CLI's refusal line; `git diff` on the protected file shows exactly four changed call expressions and no changed assertion |
| 4 | **CLI acceptance** — §6.1 offline case + PG twin | `python/tests/critical/test_check_contract.py` (add-only, `critical_guard.py --node <leaf 4>`), `python/tests/test_check.py` | `uv run pytest python/tests/critical -q`; the PG twin in the `pytest-plugin` job |
| 5 | **The critical suite** — §6.3 oracles (R1-R20) | new `crates/blastit/tests/critical_learn_existing_route.rs` | `make rs-test-critical` |
| 6 | **Docs follow the code** | `docs/ci.md`, `docs/local.md`, `docs/reference/cli.md`, `docs/reference/manifest.md` — the sentences quoted in §5.6, verbatim | `make docs-check`, `make docs-test` |
| 7 *(parallel, not blocking)* | **Mismatch 6** — `**` in `glob`, tokenised DP | `crates/blastit-scheduler/src/static_asset.rs`, a case added to `crates/blastit-scheduler/tests/critical_static_resolve.rs:750` (`immutable_globs_match_the_design_vectors`) under `critical_guard.py --node <leaf 7>` — add-only — or a new `critical_immutable_glob.rs` if the lead prefers a fresh file | `/assets/**` matches two levels, `/assets/*` does not; the six-`**` pattern over a 4 KiB path answers correctly and the suite finishes (no recursion); the struct doc names both metacharacters and the free `glob` is gone |
| 8 *(after leaf 9 / `MB.3.53`; can land in two steps)* | **Mismatch 3** — one app-reachability rule (R21). **Not `validate.rs`**: the rule lives in the boot/`check` path, exactly where `static_app_refusal` lives, because `parse_and_validate` must keep accepting `app` + `spa = true` (the protected `crates/blastit-manifest/tests/critical_static_manifest.rs:524-537`, "the page plus an app boots"). | `crates/blastit/src/{stage0,stage1,check}.rs`, `crates/blastit-manifest/src/model.rs` (the predicate only), `docs/reference/manifest.md`. Deliberate in-scope change: `stage0.rs:5061-5080`, `the_two_halves_of_the_rule_still_hold` — its first two assertions (`with_app` + a view → `Ok`; no app + static → `Ok`) stand, and its third (`with_app` + `static_only` → `Err("BLASTIT_APP: every route is static")`) **moves**: `with_app` + a non-`spa` static route becomes `Ok(())` (it forwards), and a new fixture with `spa = true` asserts the refusal and the corrected message. Writable tests: that same in-crate test plus `crates/blastit/tests/critical_dev_and_fail_closed.rs` (add-only under `critical_guard.py --node <leaf 8>`) | step 1 (message honesty only, no new accept/refuse decision): the `BLASTIT_APP` refusal stops claiming "no route can reach an app" when one can; step 2 (the rule, R21): an `app`/`BLASTIT_APP` no route can reach is refused — no interim *warning* state ships, because it would bless the shape step 2 refuses. Matrix: five route shapes × {manifest `app`, env `BLASTIT_APP`, neither} × {boot, `check`} agree |
| 9 *(already landed as `MB.3.53`)* | **Mismatch 4** — `allow_raw` becomes a production gate (R16). `MB.3.53` has already implemented it, at priority 0, and `docs/decisions/ADR-0011-raw-ingress-boot-refusal.md` already records the amendment — so leaf 9 **creates no ADR**; it is the record's handover and, if anything, a docs pass. | `crates/blastit/src/{stage0,stage1,check}.rs`, `docs/reference/manifest.md`'s raw row; `docs/decisions/ADR-0011-raw-ingress-boot-refusal.md` exists (owned by MB.3.53), `crates/blastit-manifest` untouched (`validate.rs` still warns). Writable tests: `crates/blastit/tests/critical_dev_and_fail_closed.rs` (add-only under `critical_guard.py --node <leaf 9>`) for the two boot points and the `dev` warning; `critical_manifest_011.rs:459` (the warning assertion) stays green unmodified | both boot points refuse raw-without-ack naming every offending route and the key; a manifest swapped between stage 0 and stage 1 is still refused; `dev` warns; `check` still finds `raw_ingress` |
| 10 *(second — before leaf 2; both edit `audit_v1.rs`)* | **The transaction remedy is machine-readable** — `Fix::set_transaction_control` (R18) | `crates/blastit/src/audit_v1.rs`, the loop body in `crates/blastit/tests/critical_check_engine.rs:505-515` under `critical_guard.py --node <leaf 10>` (the redacted arm keeps `observe`), `docs/decisions/ADR-0004-*` (additive action note; lead owns `docs/decisions/`), `docs/ci.md`'s action list | a tx denial's `fix.action` is `set_transaction_control` with `to = "[route.{proxy}].transaction_control"` and the finding's `route`; applying it makes `check` `ok`; the redacted arm still asserts `observe` |

**Execution order: 1 → 10 → 2 → 3 → 4 → 5 → 6, then 7, then 8.** Leaf 1 lands
first because leaf 2 turns the multi-line path into the common one. Leaf 10 lands
before leaf 2 because both edit `audit_v1.rs` and leaf 2's hint text names
`set_transaction_control`, which only leaf 10 creates; leaf 5's R18 oracle needs
it too. Leaf 9 has **already landed** as `MB.3.53` (its raw-ingress gate is
uncommitted in this tree), so leaf 8 must rebase on it and land **after** 9 —
the pair is 9 → 8, not 8 → 9. Other shared files: 6 and 10 both edit `docs/ci.md`
(10 first), and 2/3 share `audit_v1.rs`/`check.rs` (2 then 3). Leaf 7 touches only
the scheduler and may run anywhere after 1.

---

## 13. Review Resolution

Four goldfish lenses over twelve rounds in total (dx ×3, ai-ergonomics ×2,
security ×3, implementability ×4); the review files are beside this one
(`*.review-<lens>.md`, round 2 appended below round 1).

### 13.1 `dx` — `learn-single-pass.review-dx.md`

| Finding | Disposition |
|---|---|
| B1 the `learn:` line claims a write that did not happen | accepted → §5.5/R13, and R17 for the `--json` call site |
| B2 `learn_route` contradicts R3; a stale name blocks the whole write | accepted → §5.3's manifest check, R3 |
| M1 the fold becomes the common path; one comment blocks every grant | accepted → §5.4 extends instead of folding, R10 |
| M2 the all-or-nothing refusal gate is on no page | accepted → §5.6 sentence, verbatim |
| M3 R9 claims a boot guarantee the editor does not implement | accepted → R9 says "model comparison only"; `parse_and_validate` is an §6.3 oracle |
| M4 mismatch 4's safety rests on an unchecked checklist box | accepted → §7.2 production boot error, R16 |
| M5 mismatch 3 leaves two live defects, one silent | accepted → leaf 8 splits interim honesty from the rule; R21 |
| M6 exit codes and dry-run contents unspecified | accepted → §5.5 four-state table, `cli.md` row in §5.6 |
| m1 mismatch 6's silent no-op has no signal | accepted → §7.3's eight-word interim clause + leaf 7 |
| m2 the `learn:` line leads with the count that did not change | rejected — both counts are printed; the pair is the contract (§5.5) |
| m3 R10's error must be 1-based | dissolved — no line-numbered error is introduced; the split and comma edits name no line |
| m4 §1's Local row overpromises "no findings" | accepted → §1 qualified with the §7.4 exception |
| m5 leaf 6's doc list was conditional | accepted → leaf 6 names four pages and quotes the sentences |

### 13.2 `ai-ergonomics` — `learn-single-pass.review-ai-ergonomics.md`

| Finding | Disposition |
|---|---|
| B1 `--learn --json` cannot distinguish blocked from success | rejected for the document (shape pinned by R8) and **accepted as a diagnosis gap** → R17 (a cause on stderr, always) plus the §5.5 four-state table |
| B2 the repaired tx hint is still false TOML | accepted — the record's own error: `[route.<proxy>]` was right, `[route.<name>.<proxy>]` was not. O3 rewritten, R11 corrected, oracle inverted |
| B3 tx statements keep `check` red after `--learn` | accepted → §7.4, R14, and R18 (the audit names the remedy) |
| M1 fold / one comment blocks every grant | accepted → §5.4 |
| M2 refusals are locationless | accepted → §5.2/R19 (route + proxy, redacted text) |
| M3 `local.md` still teaches a second writer | rejected on the quote (`local.md:80-96` already splits "with a suite" from "click through"); **accepted for the tx gap** → §5.6's `local.md` sentence |
| M4 `learn_route` does not implement R3 | accepted → §5.3 |
| M5 §6.1's assertions are prose | accepted → §6.1 now has `tomllib` and `check --json` oracles |
| M6 exit codes/dry-run unspecified | accepted → §5.5 |
| N1 the learn document has no `schema_version` | rejected — the document is pinned without it (`§1.3`, ADR-0004) and the four-state table removes the need; an agent special-cases one command |
| N2 §1's Agent row says "apply" | accepted → §1 rewritten to the two real commands |
| Round 2: zero-request suite is a silent exit `1` | accepted → R17 and the `learn: nothing observed: …` line, oracle at the call site |
| Round 2: R8's disambiguation rule was false | accepted → replaced by the four-state table (§5.5) |
| Round 2: the bridge drops refusals and reports intent as `wrote` | accepted → R20 (`json(wrote)`, bridge prints refusals, rustdoc says it never writes) |
| Round 2: the tx `fix` is only `observe`, `unrestricted` stays shortest | accepted → R18 |
| Round 2: `local.md`'s `clean` line is unreachable for psycopg3 | accepted → §5.6 |

### 13.3 `security` — `learn-single-pass.review-security.md`

| Finding | Disposition |
|---|---|
| `learn_route` grants an undeclared route name | accepted → §5.3 |
| `allow_raw` is not defensible as a warning for 0.2.0 | accepted → §7.2 production boot error, R16, `ADR-0011` |
| the recursive `**` matcher is exponential (measured) | accepted → §7.3 tokenised DP; rows hoisted and precompiled after round 2; worst-case test in leaf 7 |
| stage 1 re-reads the manifest, so a Stage-0 check is a TOCTOU | accepted → R16 checks at both points; same note added to R21 |
| `suggest_fix` authors `allow_raw = true` | deferred → §13.5 (a fix suggestion inventing an opt-out is its own node) |
| DP allocation churn (33 MB/request) | accepted → tokens compiled in `new`, two rows reused |
| `statement.proxy` is the only unvalidated input | noted — `apply_edits` already fails an undeclared proxy with `UnknownProxy` (`edit.rs:551-556`), so the write fails rather than producing an unbootable grant |

### 13.4 `implementability` — `learn-single-pass.review-implementability.md`

| Finding | Disposition |
|---|---|
| B1 `[route.<route>.<proxy>]` is not a manifest key | accepted → O3/R11 corrected |
| B2 `learn_route` never checks the manifest | accepted → §5.3 |
| M1 the comment scan misses `queries = [ # …` | dissolved — there is no scan; the list is extended in place |
| M3 an inline grant turns `--learn` into a hard `InlineGrant` | accepted → R15 + the §5.4 paragraph (named refusal, fix in the message) |
| M4 R7 vs `json()`/`wrote` | accepted → R20 |
| M7 the new predicate contradicts `app_rule`'s second half | accepted → §7.1 pins that `is_all_static`, `can_boot_without_app` and `app_rule`'s second half are untouched; R21 + the check path |
| N1 `was_empty` / empty multi-line list | accepted → §5.4 step 4 |
| N2/N3 comma rule vs "byte-identical"; no predicate for a shared bracket line | accepted → R10 states the permitted changes, and the shared-bracket shape is split at the `]` instead of rebuilt or refused |
| N6 `ExecutionMode::Production` does not exist | accepted → §7.2 names `dev.is_none()` and the real call sites |
| N8-N11 leaf paths/scope | accepted → §12 rewritten (ten leaves, each with its own writable tests) |
| leaf 3 had no writable test path | accepted → `plan_learn_json` is `pub`, so the new Rust suite pins it |
| §6.1 was vacuous without `DATABASE_URL` | accepted → the offline case lives in the protected contract suite; the PG file is the CI twin |

### 13.5 Round 3 (the reshaped sections, re-read)

| Lens | Finding | Disposition |
|---|---|---|
| dx | §5.4's comma rule was comment-blind (`"SELECT 2" # note` got `,` inside the comment) | accepted → the comma is appended to the line's text, before `trailing_comment` (§5.4 step 4) |
| dx | a blocked `--dry-run` and the fixed point shared a row | accepted → the table is read first-row-wins and says `--dry-run` never changes the exit code (§5.5) |
| dx | `QueriesShape` made one hand-wrapped list block every grant | accepted → the shared-bracket shape is **split** at `]` instead of refused; `QueriesShape` is gone (R10) |
| dx | R9's oracle booted only the happy path | accepted → `parse_and_validate` is asserted for every fixture route shape; the `spa` + global-group shape is shown unreachable from this path (§6.3) |
| dx | pasted code cites `R24`/`R33`/`R25`/`R29` that are not this record's | accepted → a note before the block names them as the codebase's ids |
| dx | leaf 8's "no behaviour change" overstates | accepted → "no enforcement change" |
| dx | R21 omitted the static + `[route.php]` shape | accepted → R21 lists it |
| security | R18's `to` is not self-locating | rejected as stated — `to` is relative to the finding's `route`, the convention `add_query`/`add_group` already use; the record now says so and the oracle asserts both fields. The underlying point is kept: the *human* hint names the block (§7.4) |
| security | refusal lines have no length/control-byte discipline | accepted → R19 caps the line at 2048 bytes and escapes control bytes |
| security | the `"b"]` shape produced invalid TOML refused as a bare syntax error | accepted → the split in §5.4 step 3 |
| security | `**` widens what earns a year of cache | accepted as documented risk — the cache directive is a freshness property, not a security boundary; the leaf's docs line says so |
| security | `**`'s doc comment would be stale | accepted → leaf 7 updates `glob`'s comment (`static_asset.rs:205`) |
| ai-ergonomics | round-2 items (silent zero-request, false R8 rule, bridge, tx fix, `local.md`) | accepted in round 2 → R17, §5.5, R20, R18, §5.6; verified against the source lines rather than re-reviewed |
| implementability | `json(dry_run)` → `json(wrote)` leaves silent call sites | accepted → §5.5 names `audit_v1.rs:1456` and the **four** `plan.json(false)` calls in `critical_check_engine.rs` (`:1111`, `:1141`, `:1169`, `:1183`), all `plan.json(plan.would_write(false))`; leaf 3 owns that protected edit |
| implementability | R18 breaks a protected assertion (`observe`/`None`) | accepted → §11 and leaf 10 name the exact assertion (`critical_check_engine.rs:509-511`) and the authorisation; the record no longer claims an untouched file |
| implementability | the comma would land after `\n`; a lone `[` was an "element line" | accepted → §5.4 step 4 inserts before the comment *and* the newline, and defines an element line as a line starting with a quote (or the assignment line when it carries one) |
| implementability | §7.2 went back to a single `app_rule` site | accepted → both sites (`:1869` production, `:2142` dev) are named |
| implementability | `Tok` lacks `Debug`; the old `glob` becomes dead code | accepted → `Debug` derived, the free `fn glob` deleted with its stale comment, and the private `pattern` field's only caller confirmed |
| implementability | leaves 3, 7, 10 had no authorised writable test | accepted → each leaf names its protected file and the guard authorisation; leaf 7 names `critical_static_resolve.rs:750` |
| implementability (round 4) | the assignment-line clause counted a quoted SQL comment as an element | accepted → §5.4 step 4 tests only the text before `trailing_comment` |
| implementability (round 4) | "two `json(wrote)` call sites" was false — there are four | accepted → §5.5 lists `:1111`, `:1141`, `:1169`, `:1183` and says four, same assertions |
| implementability (round 4) | R18's "one assertion" cannot express the change (redacted keeps `observe`) | accepted → §11 and leaf 10 say the guard splits into two arms at `:509-511` |
| security (round 3 item 1) | the `…`-in-`Query` refusal echoed the wire `normalized`; a forged bridge verdict could print a literal | accepted → the line is **label-only** (`route "…" [route.db]: a redacted statement`), R19 says so, and §6.3's R19 oracle covers it |
| security (round 3 item 3) | the sibling shape and `"""` multi-line strings | shared-bracket shape → split (§5.4 step 3, already); `"""`/`'''` element → the one remaining fold, pinned by leaf 1 (§5.4 closing paragraph) |
| security (round 3 item 5) | the budget claim ignored `MAX_PATTERNS = 32` and had no early exit | accepted → §7.3 states 32 × 256 × 4096 ≈ 33.5 M worst case for a whole route, and the DP returns false on an all-false row |
| security (round 3 item 6) | R16's singular "the route" vs several raw routes | accepted → R16 says every offending route is named in one error; `dev`'s warning stays one per route |
| dx (round 3) | line citations had drifted (`static_asset.rs`) | accepted → `:42`, `:56`, `:70-73`, `:172-178`, `:207-226`; `Vec<bool>` is one byte per element (≈ 8 KiB, not 1 KiB) |

### 13.5b Lead reviews (attempts 1-2) — the "would misfire if implemented as written" family

| Finding | Disposition |
|---|---|
| MAJOR 1 — the `ci.md` exit-code row contradicts §5.5 row 2 (`--dry-run` writes nothing and exits `0`) | accepted → §5.6 now carries the replacement wording verbatim: "exits `0` for a fixed point or a `--dry-run`, and `1` … when it was blocked" |
| MAJOR 2 — leaf 8 named `validate.rs`, which would make `parse_and_validate` reject `app` + `spa = true` and break the protected `critical_static_manifest.rs:524-537` | accepted → `validate.rs` dropped; `model.rs` (predicate only) + `stage0.rs:5061-5080` named, with the third assertion (`with_app` + `static_only` → `Err`) moving to `Ok(())` and a `spa = true` fixture taking over the refusal |
| MAJOR 3 — the redacted-statement refusal printed the wire `normalized` | accepted → label-only (§5.2), R19, and the §13.5 row above; this was the security round-3 item that had been graded in the review file but not in §13 |
| MINOR — `:1169` asserts `wrote == false`, not `true` | fixed (§5.5 lists `:1111`, `:1141`, `:1169` → false; `:1183` → true) |
| MINOR — `Vec<bool>` is not bit-packed; ≈ 8 KiB, and the bound ignores `MAX_PATTERNS = 32` | fixed (§7.3) |
| MINOR — `static_asset.rs` citations drifted | fixed (§7.1, §7.3) |
| MINOR — "two rounds each" vs twelve | fixed (§13 preamble) |
| MINOR — §6.1 used bare `python3 -c` | fixed (`uv run python -c`) |
| MINOR — leaves 8 and 9 named no writable test path | fixed (both name `critical_dev_and_fail_closed.rs`, add-only) |
| MINOR — no §13 entry for the `"""`-entry shape | fixed (§5.4 paragraph + §13.5 row) |
| MINOR — ordering: leaf 10 must precede leaf 2 | fixed (§12 states 1 → 10 → 2 → 3 → 4 → 5 → 6, then 7, then 8, which rebases on the already-landed leaf 9 / MB.3.53) |
| **attempt 2, MAJOR** — leaf 3 said "the one `plan.json(false)` call … only the call site changes", contradicting §5.5/§11's four | fixed in all four places (§5.5, §11 row, §11 paragraph, leaf 3) to: the four calls at `:1111`, `:1141`, `:1169`, `:1183`, all `plan.json(plan.would_write(false))`, no assertion changed. The "one call site" acceptance wording is gone |
| **attempt 2, MINOR 1** — the four-state table's key could not separate a blocked `proposed 0,0` run from a fixed point | fixed → the table leads with `--dry-run` and exit `1` is the "no / any / `false`" row, with `check.rs:984-1004` quoted as the authority |
| **attempt 2, MINOR 2** — leaf 9 said it creates `ADR-0011-…-production-gate.md` | fixed → `ADR-0011-raw-ingress-boot-refusal.md` already exists (MB.3.53); leaf 9 creates no ADR |
| **attempt 2, MINOR 3** — §5.2's `format!` interpolated unsanitised components while R19 promised escaping and a cap | fixed → §5.2 names the local `escape_control` (same rule as `blastit_manifest::error::escape_control`), the per-component caps (64/64/256) and the 2048-byte line; R19 states the same |
| **attempt 2, MINOR 4** — "8 then 9" was inverted: MB.3.53's gate (leaf 9) is already in the tree | fixed → §12 says 9 has landed and leaf 8 rebases after it; leaf labels updated |

### 13.6 Recorded, not reshaped (MINORs, deliberately left)

The closing pass stops reshaping on MINORs. These are known and accepted as-is;
an implementer may fix them in passing but must not treat the record as wrong:

* The pasted `json!` block in §5.5 is not `rustfmt`-identical to `audit_v1.rs`'s
  formatting (`json!({ … })` on one line); the function's behaviour is the
  contract, not the whitespace.
* `ObsVerdict` (the wire record) and `LearnVerdict` (the policy enum) are two
  different types in two crates; the record says which it means at each use, and
  an implementer should not try to unify them here.
* §5.2's pasted loop keeps two locals the surrounding code declares; the leaf must
  read the function as it stands, not paste the block into a vacuum.
* §6.1's python one-liner assumes `route[0]` is the `notes` route; the fixture
  declares it first, and a test may index by name instead.

### 13.7 Out of scope, recorded as follow-ups
* `suggest_fix` choosing `raw` + `allow_raw = true` for an unknown or truncated
  sample (`blastit-ingress/src/lib.rs:426-441`): a fix suggestion should not
  invent an opt-out. Its own node.
* `--learn` writing `transaction_control = true` (§7.4): an ADR-sized change to
  what `--learn` may write.
* Writing the grantable subset when one statement is refused (the current gate is
  all-or-nothing): a `R29` change, not this node's.
