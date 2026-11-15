# Security review — `docs/design/learn-single-pass.md` (MB.3.51)

**Lens:** `security`
**Reviewed:** `docs/design/learn-single-pass.md` at worktree `872f523`; `docs/VISION.md`; `crates/blastit-ingress/src/lib.rs`; `crates/blastit-manifest/src/{validate,parse,edit,model}.rs`; `crates/blastit/src/{audit_v1,check,stage0,infer_routes}.rs`; `crates/blastit-dbproxy/src/learn.rs`; `crates/blastit-sqlpolicy/src/{learn,normalize}.rs`; `crates/blastit-scheduler/src/{route,static_asset,static_response}.rs`; `docs/decisions/ADR-0006-route-ingress-body-gate.md`; `ARCHITECTURE.md`; `docs/reference/manifest.md`; `docs/deploy/index.md`.
**Verdict:** **FAIL** — 0 `BLOCKER`, 5 `MAJOR`, 4 `MINOR`.

The narrow question first, since it is the decision's whole trust story: a learnt grant **cannot** widen a route beyond the observed wire *for a route the manifest declares*. Every entry is the exact `LearnVerdict::Query { normalized }` text (`sqlpolicy/src/learn.rs:254-296` refuses `…`, quoted literals, multi-statement text, `COPY … PROGRAM`, dangerous functions), re-refused by `normalise_queries` (`edit.rs:652-684`), and the write is a single `write_atomic` (`edit.rs:385-441`) with the unprivileged path unchanged. The five holes below are in the *attribution* and the *rulings*, not in the grant text.

---

## MAJORs

### M1 — §5.3's `Routed` arm never checks the manifest, so R3 and §10's "skipped, not invented" are false and the new critical oracle would fail

**Quote (lines 228-237):**
```rust
    match target {
        ObsTarget::Routed { route } => Some(route.clone()),
```
**Quote (R3, line 140):** "`Routed { route }` naming a route the manifest does not declare is skipped, never invented"; **§10 (line 569):** "A `Routed` name the manifest does not declare is skipped, not invented (R3)"; **§6.3 oracle R3 (line 335):** "`Routed { route: "nosuchroute" }` → skipped, no `MissingRoute` error".

**Failure mode:** the code in §5.3 returns the raw observation string. `plan_learn` groups a grant under it (`audit_v1.rs:1398-1417`), so `proposed.queries` counts it, `would_write` is true, and then `Plan::build` returns `EditError::MissingRoute` (`edit.rs:570-575`). `learn()` sets `failure` (`check.rs:979-988`) but still prints `"proposed": {"queries": N}, "wrote": false` (`check.rs:992-1001`); exit is `FINDINGS`. Three of the document's own claims (R3, §10, the §6.3 oracle) are therefore wrong for the code it proposes. The `Unrouted` arm has the guard; the `Routed` arm — the one the decision adds — does not. Reachability: not from the CLI (`resolve_targets` produces only `manifest.route(..).name()`, `check.rs:912-932`), but it is exactly what `_blastit.plan_learn` accepts (`audit_v1.rs:1446-1457` parses arbitrary JSONL), and §6.2's "bridge twin" test feeds `Routed` targets directly.

**Suggestion:** `ObsTarget::Routed { route } => manifest.routes().iter().find(|r| r.name() == route).map(|r| r.name().to_string())`, and make the R3 test assert `queries() == 0`, `routes() == 0`, `apply_edits` byte-identical, no `EditError`. §8.1's argument ("both read the same `RoutingTable`") is true of *attribution* and does not cover an undeclared name.

### M2 — R11's replacement hint names a TOML shape the manifest does not have; the `manifest.md:239` citation is wrong

**Quote (O3, lines 80-82):** "The key belongs in `[route.<route-name>.<proxy>]` (`docs/reference/manifest.md:239`)". **Quote (R11, line 148):** "the hint names `[route.<route>.<proxy>]` for both a proposed and an existing route".

**Verified:** routes are `[[route]]` array elements; a route's grants are keyed by **proxy name only** — `[route.<proxy>]` (`parse.rs:185-246`, `parse.rs:600-610`, `manifest.md:145`: "may not declare `[route.ingress]`, `prefork = true`, a `[route.<proxy>]`"). `manifest.md:239` is the line "Grant keys for this protocol: `queries`, `tables`, `functions`, `transaction_control`, `unrestricted`", not a table path. `[route.notes.db]` parses as route key `notes` → `resolve_grant(.., "notes", ..)` (`parse.rs:636-663`) → undeclared proxy → `UnresolvedGrant`, which `validate.rs:128` reports as an error; if a proxy `notes` did exist, `db` is an unknown grant key.

**Failure mode:** the document opens by calling the current hint a "false instruction" that makes an agent write "a manifest that fails to boot" (lines 82-84). Its fix has the same property, and R11's oracle only asserts the substring `[route.<name>.<proxy>]` appears — a string that can never be valid — so the test certifies the defect. The editor itself uses the correct header (`edit.rs:795`: `format!("route.{}", toml_key(proxy))`), so the hint is the only artefact that will disagree with the writer.

**Suggestion:** the hint cannot encode a route in a table path; use prose — `set transaction_control = true in route "notes"'s [route.db] grant table` — and assert the route name and `[route.db]` (no route name between the dots) are present. Then re-check §5.2's line 191, which also embeds the invented shape.

### M3 — §7.2's ruling is not defensible for 0.2.0: `allow_raw` has zero boot/runtime enforcement, so production serves `body = "raw"` to a PHP/Node parser without the declared acknowledgement

**Quote (§7.2, lines 441-448):** "`allow_raw` is not a runtime gate by design: its job is to make the opt-out a visible, reviewable line in the manifest diff and a CI failure when it is missing"; "a warning is enough *only while* every shipping path runs `check`".

**Verified (all of the doc's code claims are right):** the warning fires regardless of `allow_raw` (`validate.rs:810-820`); boot prints it as an event and continues (`stage0.rs:1893-1898`, `:2244-2251`); `check` reports `raw_ingress` at `Severity::Error` (`audit_v1.rs:1059-1070`); `plan_body` streams raw unchanged and never reads `allow_raw` (`blastit-ingress/src/lib.rs:276-286`); no non-test source reads `allow_raw` except the warning, the finding, the fix suggestion and the carried struct (`grep -rn allow_raw crates/*/src/`). **Consequence:** `blastit` boots and serves raw bodies with `allow_raw` absent. `ARCHITECTURE.md:960` ("`raw` passes untouched and also needs `allow_raw = true`") is false at runtime; the only barrier is a manual checklist item (`docs/deploy/index.md:111`) plus whoever remembers to run `check` after the last manifest edit.

**Why the ruling fails now, not "pending facts":** the fact the document asks for is answerable in-repo — *nothing* at boot enforces it, and a manifest edited after `check` (including one written by an agent, or by `--learn` in the same session) is served unguarded. 0.2.0 ships embedded PHP; `body = "raw"` on a PHP route is exactly the unparsed-byte php2shell surface the vision promises to close even on an unpatched runtime (`VISION.md` §Why Blastit Exists item 1). A warning event in a log stream is not fail-closed. Second defect inside the same ruling: the "visible, reviewable opt-out" is produced automatically — `suggest_fix` sets `allow_raw = true` whenever an observed codec is `raw` (`blastit-ingress/src/lib.rs:426-441` → `audit_v1.rs:1025-1030`, `Fix::declare_ingress(codecs, allow_raw)`), so an agent that applies `missing_ingress`'s suggested fix never sees the opt-out as a decision.

**Suggestion:** make `ManifestWarning::RawIngress` with `allow_raw == false` a **Stage-0 error in `Production` only** (leave `dev`'s warning and `check`'s finding), per the document's own §8.6 fallback — it adds no step because `check` already demands the key, and it makes `ARCHITECTURE.md:960` true. Also stop `suggest_fix` from setting `allow_raw = true`; emit the `body = "raw"` line and a second finding/hint for the acknowledgement. If this is deferred, the ruling must say plainly: *production 0.2.0 boots a raw route without `allow_raw`; the key is decorative.*

### M4 — §7.3's `**` matcher is exponential, not "pattern length × path length"; it runs per static answer on the request path

**Quote (lines 491-493):** "The recursion is bounded by pattern length (≤ 256 bytes, `:55`) × path length (≤ 4096, `:41`) with no allocation".

**Verified:** the code in §7.3 recurses `(0..=path.len()).any(|start| glob(&pattern[2..], &path[start..]))`; each `**` multiplies the search by every suffix, and there is no memoisation. I ported it verbatim and counted calls: 8 `**` groups / 20-byte path → 2,536; 6 groups / 40 bytes → 164,715; 10 groups / 61 bytes did not finish in 60 s. Validation admits `MAX_PATTERN_BYTES = 256` and `MAX_PATTERNS = 32` (`validate.rs:52-55`), and paths reach 4096 (`static_asset.rs:41`), so a two-group pattern such as `/**/assets/**` on a non-matching 4 KiB path is already ~10⁶ calls, and each static answer runs the whole list (`cache_control`, `static_response.rs:184-193`). A 404 flood on a static route becomes a CPU amplifier against the process that fronts every request.

**Security ruling on the *semantics*:** `**` crossing `/` is safe here — the glob only picks the year-long `Cache-Control` for `AnswerKind::Exact` answers (`static_response.rs:179-193`); it does not affect resolution, `openat2`/`RESOLVE_NO_SYMLINKS` (`static_asset.rs:65-78`), or any authorization decision. So the *feature* is defensible; the *algorithm* in the design is not.

**Suggestion:** memoise on `(pattern_index, path_index)` — a `vec![Option<bool>; (p+1)*(n+1)]` is ≤ 1 MiB at the declared ceilings and O(p·n) — or match segment runs with a two-pointer/backtrack stack. Add a worst-case test (32 × `/**/**/**/**/**/…`, 4096-byte non-matching path) asserting a step or time cap, not just `/assets/**` matching two levels.

### M5 — §1's "next `check` accepts with no findings" and §5.5's "exit codes unchanged" are false for transaction-control and connection-setup statements; `--learn` exits CLEAN without ever running `evaluate`

**Quote (§1, line 28):** "one suite run produces a `blastit.toml` diff the next `check` accepts with **no findings**".

**Verified:** `learn()` never calls `evaluate`; `clean = plan.requests > 0 && plan.refused.is_empty() && failure.is_none()` (`check.rs:976-1011`) — it is a *writer* success flag, not an audit result. Two statement classes the §6.1 fixture does not contain leave findings behind:

- `is_tx`: §5.2 skips the grant (`continue`) and emits a hint; `evaluate` still produces `MissingGrant { severity: Error, fix: Fix::observe() }` for it (`audit_v1.rs:674-681`). `BEGIN`/`SAVEPOINT` therefore always leaves the manifest unclean after `--learn`.
- connection setup (`SET TIME ZONE 'UTC'`, `SELECT pg_catalog.version()`, …): `check_learnable` returns `Query` for `KNOWN_STATIC_SETUP_SQL` before parsing (`sqlpolicy/src/learn.rs:264-266`), so §5.2 writes it into **one** route's `queries`. `evaluate` says the opposite: "A connection-setup statement is not a route's statement … a per-route grant would leave the others denying it … the documented fix is one `connection` group attached to every route" (`audit_v1.rs:685-693`). On a two-route app, route B keeps a `missing_grant`. This is pre-existing, but (a) makes it the everyday path and the design asserts the loop terminates.

**Why security, not just DX:** §7.2's acceptance rests on "`check` is the gate", and §3's alternative analysis rests on "the loop terminates in one pass, no `unrestricted = true`". A success signal that does not include the audit leaves an agent with a manifest that is still wrong after exit 0 — the pressure to hand-edit (or widen grants) the decision exists to remove.

**Suggestion:** either run `evaluate` inside `learn()` and keep exit `FINDINGS` when findings remain (wording can stay), or state in §1/§5.5 that `--learn`'s CLEAN means "the proposal was written", and add `SET TIME ZONE 'UTC'` plus `SAVEPOINT` on two routes to the §6.1 fixture so the promise is tested rather than assumed. Emit a connection-setup hint in the shape of the tx hint.

---

## MINORs

### m1 — `learn_line` says "N queries added" even when the write failed, and §5.4 makes that path common

`check.rs:1006` prints `learn_line` unconditionally; `learn_line` (lines 1015-1032) prints "added" whenever `dry_run` is false, regardless of `wrote`. With `EditError::CommentedQueries` (§5.4) reachable on the new common path, a human sees `learn: 2 queries added → /…/blastit.toml` followed by `learn: … has a comment on line 13 …`. §5.5's "the message becomes truthful without a wording change" holds only for `queries()` arithmetic, not for `wrote`. Print "proposed" unless `wrote`.

### m2 — §5.2 echoes an unvalidated `normalized` string to stderr

`refused.insert(format!("{}: a redacted statement", normalized))` (§5.2, lines 198-200). On the CLI path it is unreachable (`check_learnable` refuses `…` before returning `Query`, `sqlpolicy/src/learn.rs:261-262`), but `plan_learn_json` accepts arbitrary JSONL (`audit_v1.rs:1446-1457`), so a forged `ObsVerdict::Query` carrying a quoted literal is printed to stderr — the security skill's "never log SQL parameter values". Format `redact_literals(&normalized)` instead, as the `Refused` path already does.

### m3 — §7.3's `**` silently year-caches whole subtrees, not just hashed assets

`immutable = ["/static/**"]` now marks every exact answer beneath `/static` `public, max-age=31536000, immutable` (`static_response.rs:31`, `:179-193`), including unhashed files. Not a trust-boundary issue, but a stale-deploy hazard the manifest row should state next to "`**` crosses `/`". Also update the now-false doc comments: "`*` … is the only metacharacter" (`static_asset.rs:178-179`, `:215-216`).

### m4 — a single refusal blocks every grant on every route and names neither route nor proxy

`refused` entries are `"{redacted}: {reason}"` only (`audit_v1.rs:1350-1355`, `:1374-1377`); `would_write` requires `refused.is_empty()` (`audit_v1.rs:1284-1289`). With (a) applied to existing routes, one unparameterised query anywhere yields zero learning across a 50-route app with no localisation — the friction that pushes an agent toward hand-editing or `unrestricted`. Keep R7's strict semantics, but include `route` and `proxy` in the refusal line (both are known at that point in §5.2).

---

## Verified as claimed (no action)

- **Write path / invariants (R12, §10).** `write_atomic` (`edit.rs:385-441`): sibling `.<name>.<pid>.tmp` with `O_CREAT|O_EXCL`, `fsync`, author mode copied, `rename`, directory `fsync`, `TempFileGuard` removes the temp on every failure. No `setuid`/capability path; `check` runs under the caller's uid (`check.rs:14`). Only `<manifest>` is replaced.
- **No literal or secret reaches the committed grant.** `check_learnable` (`sqlpolicy/src/learn.rs:254-296`) refuses `…`, inline literals, multi-statement text, `COPY … PROGRAM`, dangerous functions; `normalise_queries` (`edit.rs:652-684`) refuses `…`, empty, and quoted literals again before the splice. `KNOWN_STATIC_SETUP_SQL` is the only literal-bearing allowance and is static.
- **`unrestricted` is unreachable.** `is_query_covered_by_tables_or_groups` returns `true` for an unrestricted route/group grant (`model.rs:929-931`), so nothing is proposed; `EditError::UnrestrictedGrant` (`edit.rs:580-586`) is the backstop, and `patch_grant` only ever writes `queries = …`.
- **Attribution for declared routes (R2).** `resolve_targets` (`check.rs:912-932`) and `evaluate` (`audit_v1.rs:637-643`) resolve names through the same `RoutingTable` and manifest; the grant lands where `missing_grant` is reported.
- **O2's duplicate-route defect and the §5.1 guard** are real as described (`audit_v1.rs:1341-1345` lacks the HTTP loop's `table.route(..).is_none()` guard at `:1336-1340`; `infer` disambiguates `-2` at `infer_routes.rs:96-99`).
- **§7.2's code claims** (warning regardless of `allow_raw`, boot emits it as an event, `check` reports it at `Error`, `plan_body` streams raw, no request-time read) are accurate — the ruling is the defect, not the evidence.

---

# Round 2 — security (after the reshape)

**Re-read:** `docs/design/learn-single-pass.md` (861 lines, §5.3 `learn_route`, §7.2/R16, §7.3 DP); `crates/blastit/src/audit_v1.rs:630-1465`; `crates/blastit/src/check.rs:955-1032`; `crates/blastit/src/stage0.rs:1820-1935`, `stage1.rs:225-340`; `crates/blastit-manifest/src/{parse,validate,edit}.rs`; `crates/blastit-ingress/src/lib.rs:415-445`; `crates/blastit-scheduler/src/{static_asset,static_response,route}.rs`.
**Verdict:** **FAIL** — 0 `BLOCKER`, 1 `MAJOR`, 9 `MINOR`.

## Round-1 status

| # | Status | Evidence in the reshape |
|---|---|---|
| M1 | **RESOLVED** | §5.3 `learn_route`'s `Routed` arm is `manifest.routes().iter().any(|declared| declared.name() == route).then(|| route.clone())` — the undeclared name now returns `None`, so `MissingRoute` is unreachable from it. Closes R3, §10, §6.3 R3. |
| M2 | **RESOLVED** | O3 (lines 83-88) now calls `[route.<name>.<proxy>]` *wrong*; §5.2's hint is prose + `[route.{}]`; R11 and the §6.3 oracle assert the route name **and** the absence of `[route.<name>.`. Matches `edit.rs:795`. |
| M3 | **RESOLVED** for the hole (see N1 for the residual) | R16 + §7.2 make `body = "raw"` without `allow_raw` a `Production` Stage-0 error, `dev` keeps the warning, `check` keeps `raw_ingress`, leaf 9 adds `ADR-0011`. `parse.rs:845-861` proves the key is a plain bool on `raw`, so the gate is implementable. **The placement is wrong — N1.** |
| M4 | **RESOLVED** for the algorithm | §7.3 replaces the recursive `**` with a two-row DP. Hand-verified correct (below); the exponential blast-up is gone. Bounds/allocation/test residue — N4. |
| M5 | **NOT RESOLVED as stated** (live part → N5) | The tx half is fixed: §1 scopes the claim to "the one named exception", §7.4 + R14 + a second fixture pin it. The connection-setup half **does not reproduce**: `evaluate` keys `missing` on `(index, proxy, sql)` where `index` is the *observation's* route (`audit_v1.rs:635-700`), and §5.2 groups by the same `(route, proxy)` — so a setup statement learned per route clears that route's finding. The `add_group` text is a **fix suggestion**, not a finding. Round 1 read a rationale comment as an oracle. Residue: the suggestion disagrees with what `--learn` writes (N5). |
| m1 | **RESOLVED** | §5.5's three-row table + R13 + §6.3 R13 cover refused/empty/write-failed, matching `wrote` (`check.rs:975-1001`). |
| m2 | **NOT RESOLVED** | §5.2 line 207 is byte-identical to the old code: `refused.insert(format!("{}: a redacted statement", normalized))` still echoes an unvalidated `normalized` (a forged JSONL line can carry `…` *and* a quoted literal) to stderr. `redact_literals` is still the right fix. |
| m3 | **NOT RESOLVED** | §7.3's docs half adds "`**` crosses `/`" and "a pattern that matches no file is not reported", but neither the year-cache-scope warning nor the now-false `ImmutableGlob` doc comments (`static_asset.rs:174-178` "`*` … is the only metacharacter", `:213-215`) are in leaf 7's acceptance. `/**` is now writable and year-caches every exact answer. |
| m4 | **NOT RESOLVED** | §5.2 still inserts `"{redacted}: {reason}"` with `route` and `proxy` in scope but absent from the line; R7 keeps the all-or-nothing gate, so with (a) one refusal on a 50-route app still writes nothing and localises nothing. |

## MAJOR

### N1 — R16 is enforced in the stage that *validates* the manifest, but stage 1 **re-reads and re-parses it from disk** and its parse is what builds the route list — so a raw route without `allow_raw` still serves in `Production`

**Quote (§7.2, lines 583-584):** "in `ExecutionMode::Production`, a route declaring `body = "raw"` without `allow_raw = true` is a **Stage-0 boot error**"; **R16 (line 156):** "… is a Stage-0 **boot error** …; Serving is otherwise unchanged"; **§12 leaf 9:** paths `crates/blastit/src/{stage0,check}.rs` only.

**Verified:** `stage1.rs:254-262` does `std::fs::read_to_string(&config.manifest)` then `Manifest::parse_and_validate(&source, …)`, with the comment *"Warnings are stage 0's: it reads and validates the manifest first, and printing them twice would be noise (§8.1)"* — and `_warnings` is discarded. `stage1.rs:337` then calls `routes_from_manifest(&manifest, &layout)` on **that** parse; `RouteInfo.ingress` (`scheduler/src/route.rs:91-95`) is what `plan_body` streams (`blastit-ingress/src/lib.rs:284`). `config` carries no manifest digest (`grep digest|sha256|blake3` in `config.rs`/`stage0.rs`/`stage1.rs` → nothing). So the process that decides "raw or not" never sees the errored model; it re-reads the file across `execve`.

**Failure mode (two, both fail-open):**
1. **TOCTOU on the trust root.** Anything that can write `blastit.toml` between stage 0's read and stage 1's read flips `body = "json"` → `"raw"` after the gate passed. In a rootless deployment or a bind-mounted manifest owned by a uid the app can influence, that is app-chosen disables-ingress-validation — precisely the mutation §7.2 exists to refuse. (The re-read is pre-existing; R16 is the first *security* decision that depends on the file not changing, so it inherits and amplifies it.)
2. **Plain bypass.** `blastit --stage=1 --config-fd=N` is an ordinary argv path (`cli.rs:468-490`); a `BLASTIT_MANIFEST`/file swapped between the two parses is enough. No privilege is gained, but the claim "production refuses raw without the acknowledgement" is false for the process that serves.

**Suggestion:** enforce in **both** stages, and make the second one authoritative: either stage 1 consumes stage 0's parsed model (no re-read) or `config` carries a digest of the validated bytes and stage 1 refuses on mismatch. Put the rule in one function (`fn raw_without_ack(manifest) -> Option<String>`) called by `run`, `run_dev` (warning) and `stage1::run` (error). Add the oracle to a `critical_*.rs` that `execve`s a real stage 1 against a manifest edited after stage 0 — the single-stage unit test §11 proposes cannot see this.

## MINOR

**N2 — the gate no longer has a rubber stamp, but `suggest_fix` still authors `allow_raw = true`.** §7.2 lists it as one of the three facts that make warning-only indefensible ("the key is a rubber stamp the tool fills in"), then leaves it: leaf 9's paths exclude `blastit-ingress` and `audit_v1`. `suggest_fix` sets `allow_raw = true` for **any** sample that is neither JSON nor form — including a truncated one (`blastit-ingress/src/lib.rs:426-441`) — and feeds `Fix::declare_ingress`/`Fix::fix_ingress` (`audit_v1.rs:1025-1053`). R16 therefore fails closed only against manifests the tool did *not* fix; the documented agent loop (`check --json` → apply fix) enables raw on a PHP route without ever presenting it as a decision. Emit `body = "raw"` alone plus a separate `raw_ingress`-shaped finding for the acknowledgement.

**N3 — `ExecutionMode::Production` is not a type, and the insertion point is unstated.** `config::Mode` is `Root`/`Capabilities`; "Production" is the `stage0::run` vs `run_dev` split. Leaf 9 must be told to insert between `parse_and_validate` (`stage0.rs:1827`) and `bind_view_sockets`, or an implementer will invent the enum or gate `run_dev` too. Also: §11's oracle is singular ("refuses raw-without-ack naming route + key") while `ManifestWarning::RawIngress` carries **only** `route` (`validate.rs:817`) — say whether N raw routes are all named in one boot (the codebase style: `errors.to_string()`) or the operator loops one reboot per route.

**N4 — §7.3's DP is correct but its ceilings are understated and its worst case is untested.** Hand-checked: `/assets/**` vs `/assets/app/app.css` → true; `/assets/*` vs the same → false (stops at `path[11]='/'`); `*` vs `/` → false; `**` vs `` (empty) → true; `/a/**` vs `/a/` (trailing `/`) → true; `/*/x` vs `/a/x` → true. The `Star` recurrence is the existing rule (`static_asset.rs:218-220`: "`*` may be empty, and it never crosses a `/`"), so no semantic drift. Two residues: (a) the budget is **per pattern** — `MAX_PATTERNS = 32` (`validate.rs:52`) × 256 tokens × 4096 bytes is ~33.5 M inner steps and, worse, one `vec![false; width]` **per token** (`:663`), i.e. ~33 MB of zeroing per request on a max-length 404, not "~1 M byte comparisons"; reuse two buffers + `fill(false)` and add `if !current.iter().any(..) { return false }`. (b) Leaf 7's acceptance is still "`/assets/**` matches two levels; `/assets/*` does not" — the worst-case test M4 asked for (32 × `/**/**…`, 4096-byte non-match) was dropped. Fix (b) in the same leaf; it is the only thing that keeps this true.

**N5 — `--learn` writes per-route `queries` where `check`'s own fix for the same statement is a `connection` group.** `evaluate` emits `Fix::add_group("[group.connection.<proxy>].queries")` for `is_connection_setup` (`audit_v1.rs:685-695`); §5.2 groups it into `(route, proxy)` `queries`, and `KNOWN_STATIC_SETUP_SQL` lets the literal-bearing ones through `normalise_queries`. No finding survives, but the two artefacts disagree, so a human who applies `check`'s advice after `--learn` churns the diff. Either write the group or change the advice for `--learn`-written setup statements.

**N6 — R8's rationale is false: `wrote == false` with exit `0` is the *normal* fixed point, not "only a dry run".** `clean = plan.requests > 0 && plan.refused.is_empty() && failure.is_none()` (`check.rs:983`) and `would_write` is false when there is nothing to add, so the second `check --learn` of R6 exits `0` with `wrote:false` and `dry_run == false`. R8 uses the false premise to justify not extending the document.

**N7 — R10's "byte-identical" is unsatisfiable by §5.4, so the §6.3 R10 oracle cannot pass.** Step 2 appends a comma to the last existing element line when it lacks one, and the "closing bracket shares a line with an element" fallback rebuilds the list (dropping interior comments — the exact defect §5.4 exists to prevent). The oracle asserts "the comment line, the entry lines and their quoting are byte-identical" *and* "a missing trailing comma" in the same case. Pick one: either prefix new lines with `,` (legal TOML: `val` then `ws-comment-newline array-sep`) and keep every existing line untouched, or state the one modified line in R10 and assert it explicitly.

**N8 / N9 / N10** carry round-1's m2, m4, m3 respectively (see the table): unredacted `normalized` on stderr; refusal line without `route`/`proxy`; `**`'s year-cache scope and the stale "only metacharacter" doc comments.

## Write path — does any grant exceed the wire? (R1-R16)

**No new widening of the grant text.** Every entry is still `ObsVerdict::Query { normalized }` re-refused by `normalise_queries` (`edit.rs:648-690`: empty, `…`, quoted literal outside `KNOWN_STATIC_SETUP_SQL`); the learning listener answers `Allow` to every message and "records what it saw" (`dbproxy/src/learn.rs:1-8`), so the wire *is* the grant source and the refusal rules are the only filter — unchanged by (a). Two caveats worth a sentence in §10, neither a hole:

- `statement.proxy` is trusted. In `plan_learn_json` (app-supplied JSONL) a statement can be attached to an **undeclared** proxy; `patch_grant` then `append_grant_table`s `[route.<proxy>]` (`edit.rs:826-828`) and the write succeeds — the next boot refuses it with `UnresolvedGrant` (fail closed, but R15's "refused by name" story does not cover it). A **declared, same-protocol** second proxy can receive a forged grant. Pre-existing for proposed routes; (a) extends it to existing ones. Cross-check `statement.proxy` against `manifest.proxies()` + the statement's `kind` in `learn_route`.
- §5.4 is the right mitigation and should be argued that way: appending into an existing list is a one-line diff inside a route the reviewer already trusts, i.e. *less* visible than a whole new route block — which is why the non-folding editor and the refusal gate, not the diff alone, are the review control.

**Verified as claimed (no action):** `learn_route`'s `Unrouted` arm resolves through `table.route` → `manifest.route` → `plan.by_path`, all three manifest-derived, so R3/R4 fail closed; §5.1's SQL `paths` guard is the same predicate as the HTTP loop (`audit_v1.rs:1335-1345`); the de-dup lookup is `manifest.routes().position(..)` (`:1394-1396`), so an existing route's `granted_already`/R5 works unchanged and R6's fixed point holds; `toml_basic_string` (`edit.rs:1446-1465`) escapes `\\ " \n \r \t` and control bytes, so the new insertion path cannot inject TOML; presets are config-only (`config.rs:51-165`), so they cannot inject `body = "raw"`; and `stage1`'s re-parse is the one placement problem, not a second manifest reader for grants.

---

# Round 3 — verdict: **FAIL** (0 `BLOCKER`, 1 `MAJOR`, 6 `MINOR`)

**Re-read:** `learn-single-pass.md` (1115 lines; §5.2, §5.3, §5.4, §5.5, §7.1-R21, §7.2-R16, §7.3, §7.4-R18); `crates/blastit-manifest/src/edit.rs:788-1000`; `crates/blastit/src/{check,audit_v1}.rs`; `crates/blastit-sqlpolicy/src/{learn,literals}.rs`; `crates/blastit-dbproxy/src/learn.rs`.

## Resolved since round 2 (not re-litigated)

`m4`/`N9` (refusal lines now `route "x" [route.db]` / `proxy "db"`, §5.2 + R19) · `N1` **MAJOR** (R16 now refuses at stage 0 *and* stage 1's re-read, `stage1.rs` in leaf 9) · `N4`'s allocation churn (tokens compiled in `new`, two rows reused) and its worst-case test (leaf 7) · `N3`'s `ExecutionMode::Production` fiction.

## MAJOR

**N11 — R18's `to` is not self-locating: `[route.db].transaction_control` binds to the *last* `[[route]]`, and `Fix` has no route field, so the documented agent loop can enable an undeclared capability on the wrong route.** R18 says the fix carries the `to` **and the route**; `Fix` (`audit_v1.rs:242-257`) is `{action, to, codecs, allow_raw, unsafe}` — no route. So either the shape grows a field (contradicting §7.4's "additive `fix.action` value: no key changes" and ADR-0004's contract) or the route is silently dropped. §7.4 item 1 already calls the identical `[route.db]`-at-EOF binding "the trap" for the hint, and §6.3's oracle asserts only `action` + `to` (`"fix.to == \"[route.db].transaction_control\""`), i.e. it certifies the ambiguous string — the same defect class round 1 M2 found in R11's oracle. Effect: an agent that appends `to` writes `transaction_control = true` on the last route, which §7.4 itself calls inventing a capability the author did not declare. **Suggestion:** either add `route` to `Fix` (and say so in ADR-0004) and assert it in the oracle, or make `to` the full, unambiguous sentence the hint uses and assert the route name in it.

## NEW / surviving items

1. **`MINOR` — N8 unresolved.** §5.2's `…` branch is still `format!("route \"{route}\" [route.{}]: {normalized}: a redacted statement", statement.proxy)`; `plan_learn_json` deserialises arbitrary JSONL, so a forged `Query { normalized }` with a quoted literal is echoed to stderr. Use `redact_literals(&normalized)`.
2. **`MINOR` — the located refusal line has no length or newline discipline.** `learn: refused {refusal}` (`check.rs:972-973`) interpolates `{redacted}` (bounded only by PostgreSQL `MAX_MESSAGE_LENGTH`; `redact_literals` keeps every non-literal byte, `literals.rs:82-96`) and, on the bridge path, `statement.proxy` verbatim — no cap, no control-char strip, no `toml_key`. A long `SELECT` exceeds the skill's 2048-byte line budget; a crafted proxy name forges stderr lines. Add a truncation rule to §5.2/R19 and validate `proxy` against `manifest.proxies()` in `learn_route`.
3. **`MINOR` — `QueriesShape` itself is fail-closed, but its sibling shape is not refused.** `apply_edits` returns the text before any write (`check.rs:981-989`), so `QueriesShape` → `failure` → no write: correct. However `queries = ["a"` + newline + `]` passes step 2 (`]` stands alone), has no *element line* for step 3, and step 4 inserts after `assignment` → `["a"` newline `"b",` → invalid TOML → `EditError::Verification { section: "the document syntax" }` (`edit.rs:975-979`), a refusal that names no route and not the shape. Same for an author's `"""` multi-line string counted as an element line (a `,` appended inside it). Step 2 must also refuse a `queries = [` line that carries a value, and the element-line test must be string-aware.
4. **`MINOR` — N10 unresolved.** §7.3's docs half still omits the year-cache-scope warning (`/**` marks *every* exact answer beneath it `immutable`, `static_response.rs:179-193`) and leaf 7's acceptance still does not carry the now-false "`*` … is the only metacharacter" doc comments (`static_asset.rs:174-178`, `:213-215`).
5. **`MINOR` — N4's budget claim is still per pattern.** "at most ~1 M byte comparisons" (§7.3) ignores `MAX_PATTERNS = 32` (`validate.rs:52`): a max-length 404 over the full list is ~33.5 M inner steps with no `if !current.iter().any(..) { return false }` early exit, and leaf 7's oracle only asserts "answers correctly and the suite finishes". Add the early exit and state the list-level bound.
6. **`MINOR` — N3 residue and N2 deferral.** R16 and §11 still say "naming the route and the key" (singular) while `ManifestWarning::RawIngress` carries one route (`validate.rs:817`): say whether N raw routes are all named in one boot. §13.5 now tracks `suggest_fix`'s `allow_raw = true` (`blastit-ingress/src/lib.rs:426-441`) — still live, so the documented fix loop keeps enabling raw on a PHP route without presenting it as a decision.

## Checked and clean this round

- **No literal, secret, or header leaks from the added text.** `ObsVerdict::Refused { redacted }` and `WireSqlObs::sql` are both `redact_literals` output (`dbproxy/src/learn.rs:159-166`, `:604`); the only unredacted allowance is `KNOWN_STATIC_SETUP_SQL`, which is static constants (`literals.rs:83-85`). Refusal `reason` is sanitised on the CLI path (`sanitize_parser_message`); no header value or secret appears in any added sentence.
- **The zero-observation diagnostic reveals nothing.** `learn: nothing observed: … (does DATABASE_URL point at blastit?)` names an env var, never a value, and only repeats the run's own observation count. Misleading for an HTTP-only suite (cosmetic only).
- **R21's predicate is sound.** Every route shape maps correctly, `_ => true` covers php-only and php+static, and the `!routes.is_empty()` guard leaves the no-app all-static case untouched.
