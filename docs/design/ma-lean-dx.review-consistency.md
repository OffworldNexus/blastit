# Consistency review (pass 2): `ma-lean-dx` + ADR-0001…0006

- **Lens:** `consistency`
- **Reviewer:** fresh-context goldfish critic
- **Status of reviewed docs:** design `In review`; ADR-0001…0006 `Proposed` (index `docs/decisions/README.md` matches every title/node)
- **Normative baseline:** `README.md`, accepted `docs/**`, `docs/VISION.md`, `ARCHITECTURE.md`
- **The design's own claim (line 9):** *"Every flag, TOML key, exit code, stdout/stderr guarantee, and JSON field specified in the user documentation is normative."* This review holds it to that.

**Verdict.** The revision fixed most of pass 1: the precedence ladder is split, `sql.observed` is gone, `--learn` no longer mutates `transaction_control`, one Rust editor owns both learn paths, learn-mode ingress is scoped, and the 9-kind JSON table is complete. Two classes of problem remain. First, the `…` redaction contract is still self-contradictory on three shipped surfaces, and the docs gate was narrowed in a way that hides it. Second, several normative seams still disagree with `ARCHITECTURE.md`, `docs/ci.md`/`docs/local.md`/`docs/frameworks/flask.md`, or contradict another requirement in the same document.

**Counts:** **1 BLOCKER, 8 MAJOR, 14 MINOR.**

---

## BLOCKER

### C2-B1. The `…` redaction contract still ships an ungrantable `sql`, a boot-fatal doc example, and an `add_query` fix that cannot boot

**Evidence**

- Design R24 (`design:127`): *"`Manifest::validate` rejects any `queries` entry containing `…` (`ManifestError::RedactedQueryPlaceholder`)"*, and R24 requires every `blastit check` finding to replace quoted literals with `…`.
- Design R33 (`design:138`): every `missing_grant` carries `"sql": REDACTED_NORM_SQL` and `"fix": {"action": "add_query", "to": "[route.<P>].queries"}`.
- Accepted `docs/ci.md:82-86`: *"`sql` is the normalised statement, exact and complete, and never contains a literal value. A grant finding names its `route` and `proxy`, and `fix.to` is that grant's table in the manifest: `"[route.db].queries"` inside the route and proxy the finding names, so `add_query` appends `sql` there…"*
- Accepted `docs/local.md:114-115`: the logged `sql` is *"with quoted literals replaced by `…`: exactly what a grant has to contain"*, and `local.md:119-129` tells the user to paste that statement into `queries`.
- Accepted `docs/reference/manifest.md:48` still ships `queries = ["SELECT … FROM django_session WHERE session_key = $?"]`.
- The docs gate was narrowed to `` ```toml title="blastit.toml" `` fences *"so partial prose fragments (`manifest.md:48`) do not collide"* (`§9.3 C-B1`) — i.e. the gate now skips exactly the block that would fail.

**Failure mode.** For any app that interpolates client-side (or any hand-written legacy grant), `blastit check --json` emits `missing_grant` whose `sql` contains `…`. An agent applying the documented fix appends that string to `[route.<P>].queries`; the next boot fails with `ManifestError::RedactedQueryPlaceholder`. The docs' *"exactly what a grant has to contain"* is false in precisely the case where the user needs it, and a copy of `manifest.md:48` refuses to boot. That is the drift-fixing loop `docs/VISION.md:26` promises, ending in a dead end — the pressure that produces `unrestricted = true`.

**Suggestion.** Keep the rejection, then make the emitted contract truthful and track the doc edits (MA.5's `paths` forbid `docs/**`; `plan/nodes/MA/MA.5/NODE.md` says to log the conflict and escalate, not to hide it):

1. Do not emit `add_query` for SQL containing `…`. Emit a distinct, documented remediation (e.g. `"action": "parameterize"`, which requires an action-set/docs decision) or `"add_query": null` plus `"redacted": true`; never emit a `fix.to` that produces an unbootable manifest.
2. Fix `docs/reference/manifest.md:48` to a concrete parameterised grant (or mark the block illustrative) and reword `docs/local.md:114` to *"for a parameterised statement this is exactly what a grant has to contain; a redacted `…` can never be granted"*. Record it as a follow-up node or escalation in `§2 Non-Goals`, not only in a review-resolution table.
3. Document the `KNOWN_STATIC_SETUP_SQL` exceptions, which also make `manifest.md:188`'s absolute *"a statement that inlines a literal cannot be granted"* false for `SET TIME ZONE 'UTC'` (shown grantable at `manifest.md:163`) and `set_config('TimeZone', …)`.
4. Add a critical test in `critical_check_schema.rs`: apply every `add_query` emitted by a golden fixture to its manifest and assert `Manifest::from_toml_str` succeeds. That single oracle prevents this class from recurring.

---

## MAJOR

### C2-M1. `ready` gains a `port` field that `ARCHITECTURE.md` forbids and `log-events.md` does not list; `--port 0` is undocumented

**Evidence**

- Design R22 (`design:123`): *"`blastit dev` … emits … `"port": <bound_port>` in `ready`"*.
- `ARCHITECTURE.md:1399`: *"A port is never a log field at all — it is a key of `environ`, not of an event, and it reaches the app and nothing else."*
- `docs/reference/log-events.md:61-62`: `ready` carries *"`stage: "serving"`, the route count, the worker budget, and the **names** of the output variables per proxy"* — no port.
- `docs/reference/cli.md:24` documents `--port N` with no `0`/ephemeral behaviour; design R2 (`design:95`) adds *"`0` binds an OS-assigned ephemeral port"*.

**Failure mode.** An implementer cannot satisfy both documents; a log consumer following `log-events.md` will not find the port, and the "port is never a log field" invariant is silently broken. `§2`'s deferred sync list names `ARCHITECTURE.md` §6 and §8.2 only — this sentence is in §8.1.

**Suggestion.** Choose one: drop `port` (test harnesses can read the pretty banner) or add it to `log-events.md`'s `ready` fields and `ARCHITECTURE.md` §8.1, extend the §2 follow-up list to §8.1, and document `--port 0`. Add a critical assertion that `--port 0` makes `ready.port` equal the actual listening port.

### C2-M2. Route attribution has two competing sentinels (`__learn__` vs `__unrouted__:<METHOD>:<PATH>`) and no defined JSON/plan behaviour

**Evidence**

- Design `design:383` introduces `as_learn_fallback()` (`is_learn_fallback`); `design:387` sends `PUSH __unrouted__:<method>:<path>\n`; `design:416` defines `target: String, // route name or "__unrouted__:<METHOD>:<PATH>"`.
- Design `design:430` still branches on *"when `listener.spec.route == "__learn__"`"*, and `ADR-0003:16` calls it *"a hidden `__learn__` route"*. No `__learn__` route is ever created in the design, and `__learn__` does not match the documented route-name regex `[a-z0-9][a-z0-9_-]*` (`manifest.md:68`).
- The sentinel is parsed by colon (`method`/`path`), but `:` is legal in a path segment; method case differs between `design:387` (`<method>`) and `design:416` (`<METHOD>`).
- R29 keys `ManifestEdits.queries` by `(route_name, proxy_name)` (`design:313`), but no step maps a `__unrouted__:…` target to the R28-inferred route name.
- R33 (`design:138`) emits `missing_grant` with `"route": R` for *every* denied statement; for SQL inside an unrouted request the only target is the sentinel, so `route` is not a route name and `fix.to` becomes `[route.__unrouted__:GET:/x].queries` — not a valid table path and contrary to `ci.md:83-86`.

**Failure mode.** Either unrouted SQL is dropped (then `statements`/`denied` no longer mean what `ci.md:87-89` says) or invalid findings ship. `plan_learn` cannot attach learned queries to the route it proposes, so `learn: N routes, 0 queries` can be written with the queries lost.

**Suggestion.** One sentinel, one grammar: e.g. `PUSH_UNROUTED <method> <percent-encoded-path>` (or length-prefixed), and delete `__learn__`. Define `audit_v1`: SQL under an unrouted target produces only `unrouted_request` (no `missing_grant`), and `plan_learn` maps it through `infer_routes` to the proposed `route_name`. Add critical fixtures for an unrouted-only suite and a path containing `:`.

### C2-M3. R4 and R29 give opposite outcomes for a suite that only hits unrouted paths

**Evidence**

- R4 (`design:97`): exits `1` when `--learn` *"observed `0` routed requests, leaving the manifest untouched with `"wrote": false`"*.
- R29 (`design:132`): writes when *"`exercised_routes > 0` (or `proposed.routes > 0`)"*.

**Failure mode.** A suite that hits only `/robots.txt` has `exercised_routes == 0` and `proposed.routes == 1`. R29 writes the proposed route; R4 says exit `1`, `"wrote": false`. The two normative requirements cannot both be implemented, and the critical test cannot encode both.

**Suggestion.** Use one predicate in R4 and R29: write when `exercised_routes > 0 || proposed.routes > 0`, exit `0` when written (or `--dry-run`), `1` only when nothing was learned or a statement was refused. Add the unrouted-only fixture.

### C2-M4. `blastit-manifest` needs `blastit-sqlpolicy`, but the "exact" dependency matrix omits it

**Evidence**

- Design `design:208`: *"`crates/blastit-manifest/Cargo.toml`: adds `blastit-ingress = { path = "../blastit-ingress" }`."*
- Design `design:198` mermaid: `c_manifest --> c_sql`.
- R24 (`design:127`) and `§4.2 B` (`design:300`) make `Manifest::validate` call `has_quoted_literal` / `KNOWN_STATIC_SETUP_SQL`, both declared in `crates/blastit-sqlpolicy` (`design:323-326`).

**Failure mode.** Leaf 3 cannot compile as specified. A cheap implementer will either add the dependency (making the "exact" matrix wrong) or copy the constants into `blastit-manifest`, creating the second SQL lexer the design's threat model forbids.

**Suggestion.** Add `blastit-sqlpolicy = { path = "../blastit-sqlpolicy" }` to the manifest crate (and confirm the direction stays acyclic), or move `KNOWN_STATIC_SETUP_SQL`/`has_quoted_literal` into a lower crate both use. State the direction in Leaf 1.

### C2-M5. `blastit check`'s credential handoff and socket location are Django-only, contradicting the framework-agnostic promise

**Evidence**

- Design `design:444`: *"real DB env stays in inherited process env only until `_check_pytest.py` finishes `django_db_setup`, at which point `_check_pytest.py` overwrites `os.environ` with `swapped_env`!"*
- `docs/ci.md:118-120`: *"Statements outside a routed request — migrations, fixtures, teardown — are not audited; only what a request sends is. The test database is prepared first with the real credentials, then the suite runs through the proxy."* This is stated for any WSGI framework (`ci.md:3-12`).
- `docs/reference/manifest.md:144-146`: `socket_dir` is *"`/proc/self/cwd`, each view's own socket directory"*, while `check` binds listeners under `<runtime_dir>/pg` (`design:443`). `check_session.json` carries `swapped_env` (`design:444`) without defining it.

**Failure mode.** A Flask/SQLAlchemy/raw-WSGI suite has no `django_db_setup` hook: either the real `DATABASE_URL` stays in `os.environ` for the whole suite (breaking the credential-barrier promise in `docs/security.md:62-67`) or the swap moment is invented and fixtures race it. If `swapped_env` renders the documented `socket_dir`, the app connects to `./.s.PGSQL.5432` and every statement fails to connect.

**Suggestion.** Define the handoff for both cases: keep the real env only until an explicit test-DB setup hook (the pytest-django hook when present, session start otherwise), and specify `swapped_env` as the rendered proxy variables with `socket_dir = <runtime_dir>/pg`. Add a non-Django critical test that asserts no real secret is in `os.environ` during a routed request.

### C2-M6. `SELECT pg_catalog.version()` is a connection-init read the design drops, contradicting `flask.md`

**Evidence**

- Design R31 (`design:136`) and `§4.2 F` (`design:430`): with `active_route_stack` empty, record only `SET`/`SHOW`/`KNOWN_STATIC_SETUP_SQL` (max 8) for replay; everything else is allowed with **zero** observations.
- `docs/frameworks/flask.md:99-100`: *"Its first connection runs `select pg_catalog.version()` before any request statement. `--learn` records it; a `[group.<name>]` is where it belongs."*
- A `SELECT` is not in the replay whitelist, so when the connection is first opened by a session fixture (after the env swap, before any routed request) the statement is neither observed nor replayed.

**Failure mode.** `blastit check` can be green while production denies `pg_catalog.version()` with `42501` on the route's first connection — the exact "CI proves nothing was forgotten" claim in `docs/VISION.md:21` failing. If the connection opens inside a request it works, so the failure is intermittent and fixture-order-dependent.

**Suggestion.** Classify `pg_catalog.version()` (or all idempotent `SELECT`s on `pg_catalog`) as session-init and replay them under the first routed target, as the design already does for `SET`/`SHOW`. Add a critical test with a session-scoped engine fixture that connects before the first request.

### C2-M7. Route insertion only recognises `path = ['.*']`, while the manifest accepts `path = ".*"`

**Evidence**

- Design R28 (`design:131`) and `§4.2 G.4` (`design:489`): proposed routes are *"inserted immediately above the first `path = ['.*']` catch-all route or at EOF"*, and the fixture row (`design:504`) detects `fallback(path=['.*'])`.
- `docs/reference/manifest.md:69`: *"a list of regexes … one regex may be a bare string (`path = ".*"` is `path = ['.*']`)"*.

**Failure mode.** With `path = ".*"` (documented, and the form a hand-written catch-all is likely to use), the proposal is appended at EOF, after the catch-all, so every inferred route is unreachable — the `--learn` pass reports routes that can never be hit, and a later audit reports them `unexercised_route`.

**Suggestion.** Detect the catch-all through the parsed `Manifest`/`RoutingTable` model (end-anchored `.*` after normalisation), not by matching TOML text; insert by route index. Add a 13th `infer_routes` fixture row with the scalar form.

### C2-M8. Transaction-control denials have no representable fix, and `add_query` is the wrong documented fix

**Evidence**

- Design R33 (`design:138`): *"Every observed SQL statement on `(route, proxy)` denied by `Allowlist::check` emits … `"fix": {"action": "add_query", "to": "[route.<P>].queries"}`"*.
- `docs/ci.md:91-93` closes `fix.action` to eight values, none of which enables transaction control; `docs/troubleshooting.md:64`: *"a `SAVEPOINT` refused | the route needs `transaction_control = true`"*; `docs/reference/manifest.md:182` defines the boolean; `manifest.md:255-256` says a savepoint name *"cannot be granted"*.
- R27 (`design:130`) only emits a **stderr** hint to set `transaction_control`; `--learn --json`'s document is fixed to `{"proposed", "wrote"}` and carries no hint.

**Failure mode.** A `SAVEPOINT`/`BEGIN` denial produces `missing_grant` telling the agent to append `SAVEPOINT "s123"` (which contains a literal and is redacted/refused) or `BEGIN` (which the documented grant model does not treat as a query). The agent loops on an inapplicable fix, and the machine contract has no way to say "set `transaction_control = true`" — unlike the human docs.

**Suggestion.** Exclude transaction-control statements from `missing_grant`/`add_query` and give them their own finding/action (needs a docs follow-up), or add `"action": "enable_transaction_control"` as a reserved action and document it. Carry the `--learn` hint in a documented field or document stderr as the channel. Add a SAVEPOINT-only golden fixture.

---

## MINOR

1. **`§4.2 H` does not exist.** `design:70` (*"specified field-for-field in §4.2 H"*), `design:131` (*"deterministic segment rules in §4.2 H"*), and `design:140` (*"See §4.2 H interaction matrix"*) point at subsections that are `§4.2 G.2/G.3/G.4`. Renumber or fix the three references; these are the normative tables.
2. **R35 still contains an open question.** `design:140`: *"(suppressed when `requests == 0` and `no_observations` already fires? See §4.2 H …)"*. Replace with the §4.2 G.3 rule as a declarative sentence; a requirement with a "?" is not implementable unambiguously.
3. **Route-name sanitisation prose disagrees with its own fixture.** R28 says non-`[a-z0-9_-]` runs are *"collapsed to `-` and leading non-`[a-z0-9]` stripped"*; for `/café/1` that yields `caf-`, but the normative row (`design:501`) says `caf`. Add trailing-separator trimming to the rule (or state that the table wins).
4. **Group `dead_grant` still contradicts `ci.md`'s `fix.to` sentence.** `docs/ci.md:83-86` says *"`fix.to` is that grant's table … inside the route and proxy the finding names"*; design `design:480` emits `"group": G` with `"to": "[group.<G>.<P>].queries"`, which is not inside the named route. Add a sentence to `ci.md` (follow-up) or a `"scope": "group"` field so the JSON is self-describing.
5. **The human formatter is underspecified for 3 of 9 kinds.** `§8 Leaf 8` says "human formatter"; the only normative example (`design:50-61`) covers six kinds one-per-route. Wording for `ingress_mismatch`, `raw_ingress`, `no_observations`, plural forms, and two findings on one route is left to the implementer. Pin a 9-row `kind -> line` table and assert the exact bytes of the `ci.md:36-47` example.
6. **`fold`'s exit/refusal contract is still incomplete.** `docs/reference/cli.md:99-100` says exit `1` only *"on an unreadable log or manifest"*; design `§1.1` (`design:30`) adds *"edit verification failure"*. `fold`'s refusals (inline literals, multi-statement, non-DML, unsafe functions) are documented nowhere, and `manifest.md:288` advertises `fold` from `sql.denied` events whose `sql` is now redacted with `…` (so `fold` refuses the very statements). Document both on the page (follow-up) and say a literal-bearing denial must be parameterised first.
7. **Preset/seccomp warning shape is inconsistent.** `ADR-0005:18` says *"plus a `warn` event (`preset_expects_root` / `preset_expects_rootless`)"*, `ADR-0005:22` says *"an unmistakable `WARN` `boot` / `manifest.warning` event"*, `ARCHITECTURE.md:1432-1435` says *"A `warn` line follows the `config` one"*, `log-events.md:37` defines `manifest.warning` as manifest-only and lists no `warn` event, and R18 (`design:119`) says *"`Warning::SeccompDisabled` in `boot` logs"*. Pick one event/field shape and document it; `preset_expects_*` is currently neither an event name nor a field.
8. **`RefuseReason::Unparseable` is unreachable.** `design:343` defines it, but `check_learnable`'s six-step order (`design:359-365`) never constructs it: a `pg_query::parse` failure falls through to `Query`. Fail closed with `Refused { Unparseable }` and test malformed SQL.
9. **`WireSqlObs` can carry the `KNOWN_STATIC_SETUP_SQL` literals.** `design:414` says *"Never carries unredacted SQL literals!"* and `§5.1` (`design:527`) says `redact_literals` scrubs *"before any `SqlDenied` or `WireSqlObs` leaves `dbproxy`"*, but step 2 returns `LearnVerdict::Query { normalized }` for `SET TIME ZONE 'UTC'` / `set_config('TimeZone', …)` and `verdict` is a `WireSqlObs` field. Fixed strings, not secrets — say "no user-derived literals" or redact the verdict too.
10. **`BLASTIT_PRESET` is an exception to a blanket documented rule.** `docs/reference/settings.md:3-5`: *"A set `BLASTIT_*` variable wins over the manifest and over a preset, even when empty"*; `ADR-0002:26` keeps whitespace-only `BLASTIT_PRESET` *unset*, and R2/R3 make `dev`/`check` ignore it. Neither exception is in `settings.md`. Add it (follow-up).
11. **`--learn` exit semantics are absent from both exit tables.** `docs/ci.md:105-110` defines `1` as "findings"; design R4 extends it to *"`--learn` refused a statement / observed 0 requests"*, and R2 gives `blastit dev` exit `1` on a refused statement and `0` on `SIGINT`. `local.md` gives no dev exit codes. Document, or make `--learn` use a distinct code (the docs' table is closed).
12. **Undocumented observation filters.** R32 (`design:137`) excludes `BLASTIT_HEALTH_PATH`; `§4.2 G.3` suppresses `unexercised_route` when `requests == 0`; R34 emits no `dead_grant` on unexercised routes. `docs/ci.md:9-12` says *"every request the suite makes and every statement it sends is checked"* and defines both kinds without these interactions. Document the three filters (they are sensible) so an agent's counts match.
13. **The deploy checklist names the wrong `boot` line.** `docs/deploy/index.md:63-64`: *"The first `boot` event reports the `privilege` you expected and the `preset` you asked for"*; `log-events.md:48-55` and `ARCHITECTURE.md:1425-1432` put `preset` on the **`config`** line and stage0's `preset` is always `null`. `§9.3 C-m9` claims this was "noted as a read-only doc follow-up", but `§2` records no such follow-up. Record it.
14. **`BLASTIT_ENV_STRIP` wording is ambiguous.** R20 (`design:121`) strips *"`BLASTIT_*`, and `BLASTIT_ENV_STRIP` variables"*, which reads as hiding the variable named `BLASTIT_ENV_STRIP`; `docs/reference/settings.md:16` defines it as a list of *"extra variables hidden from the app"*. Say "the names listed in `BLASTIT_ENV_STRIP`" — this is a secret-stripping path.

---

## Pass-1 resolution audit (consistency lens)

| Pass-1 ID | Status in this revision | Note |
|---|---|---|
| `C-B1` (BLOCKER) | **RESIDUAL — BLOCKER** | Rejection kept and `LiteralQuery` added, but `manifest.md:48`, `local.md:114`, and the JSON `add_query` value are unresolved; the gate was narrowed around the conflict. See C2-B1. |
| `C-B2` (BLOCKER) | **RESOLVED at design level** | `fold` back to exit `0`; `check --learn` all-or-nothing with `"wrote": false`. Residual MINOR: `--learn` exit `1` is still outside the docs' exit table (C2 minor 11). |
| `C-M1` precedence split | Resolved | R9 now separates selector from values, matching `settings.md:3-5`/ADR-0005. |
| `C-M2` public `sql.observed` | Resolved | Internal `obs_fd` only; `§4.2 D` (`design:368`) adds no public event kind. |
| `C-M3` silent `transaction_control` | Resolved | R27 writes routes/queries only; stderr hint. |
| `C-M4` dual TOML editors | Resolved | One `blastit-manifest/src/edit.rs`; `fold --write` mirrors the discipline. |
| `C-M5` learn-mode ingress bypass | Resolved | R26/`§4.2 E` keep body cap and declared codecs; only `UndeclaredBody` is bypassed. |
| `C-M6` audit-mode short-circuiting | Resolved | R26 covers audit **and** `--learn`, and no longer names a scheduler inside `check`. |
| `C-M7` double-counting WSGI wrappers | Resolved | `ContextVar` guard in `design:135`, `design:741`. |
| `C-M8` `--learn`/`fold` refusal scope | Resolved/partial | Refusals narrowed; `fold`'s `--allow-dangerous` policy preserved. Residual: refusal classes are still undocumented (`ci.md:149-151`). |
| `C-M9` `allow_raw`/reserved names | Resolved | `allow_raw` on other codecs is a plain `UnknownKey`; `"ingress"`/`"all"`/`"default"` claims removed. |
| `C-M10` `ARCHITECTURE.md` sync | **Partial** | §6/§8.2 recorded, but §8.1 (port, events) is not, and `blastit_live_server`/doc pages are not in the follow-up list. |
| `C-M11` literal-bearing `queries` | Resolved | `ManifestError::LiteralQuery` added. |
| `C-M12` finding JSON drift | **Partial** | 9-kind table complete; group `dead_grant` still contradicts `ci.md:83-86` (C2 minor 4); transaction-control has no fix (C2-M8). |
| `C-m1…m12` | Mostly resolved | Temp-file name, lexical path, zero-obs, missing `pytest`, `--manifest` rejection, `BLASTIT_PRELOAD`, preset scope, `learn:` stream, `remove_route` reservation, `dead_grant` redaction all landed. Residual: `deploy/index.md` boot-line follow-up (C2 minor 13). |

## Cross-checks that passed

| Surface | Result |
|---|---|
| `blastit dev` flags/defaults (`--port`, `--host`, `--learn`) | Match `docs/reference/cli.md:14-31`, `docs/local.md:25-34`, ADR-0001 |
| 3-line banner text and pluralisation | Matches `README.md:38-43`, `docs/getting-started.md:40-45` word for word |
| `BLASTIT_DEV_EXPOSE` two-key guard | Matches `local.md:31-34`, `troubleshooting.md:45-46`, `settings.md:30` |
| `[route.ingress]` codec table and 413/400 split | Matches `manifest.md:76-103`, `security.md:87-93`, ADR-0006 |
| Human `check` findings block, 6-problem summary | Matches `ci.md:36-50` |
| `schema_version: 1`, counts, `ok`, 9 kinds, `fix.action` closed set | Matches `ci.md:58-96`, ADR-0004 |
| `learn: <R> routes, <Q> queries added → <path>` | Matches `local.md:83`, `django.md:119` |
| `blastit check` exit `0/1/2/3`, pytest `5` → `3` | Matches `ci.md:103-110`, `troubleshooting.md:35-37` |
| Preset values (`do-connecting-ip`, `x-forwarded-for`, `*`, `on`, `json`) | Match `deploy/digitalocean-paas.md:11-14`, `deploy/compose-traefik.md:11-14`, `ARCHITECTURE.md` §8.2 |
| `BLASTIT_SECCOMP=0` preset abort / non-preset warning | Matches `settings.md:50`, `troubleshooting.md:11`, ADR-0005 (event shape excepted) |
| Top-level key ordering error text | Matches `manifest.md:30-31` exactly |
| Route-name regex `[a-z0-9][a-z0-9_-]*` | Matches `manifest.md:68`, R28 |
| Six `title="blastit.toml"` fences (count, parseability) | Verified: README, index, getting-started, local, django, flask — all 6 parse under the new schema |
| ADR index titles/statuses/nodes | Match `docs/decisions/README.md` |

## Pre-acceptance edit list

1. Resolve C2-B1 end to end: one truthful remediation for redacted `sql`, fix/track the two doc passages and the two `KNOWN_STATIC_SETUP_SQL` exceptions, and add the "every `add_query` must boot" oracle.
2. Make `ready`/`--port 0` and `ARCHITECTURE.md` §8.1 agree (C2-M1), and extend the deferred sync list.
3. Pick one route-attribution sentinel and define JSON/`plan_learn` behaviour for unrouted SQL (C2-M2); reconcile R4/R29 (C2-M3).
4. Add `blastit-sqlpolicy` to the manifest crate or move the shared literal API (C2-M4).
5. Specify the framework-agnostic env swap/`swapped_env` and the `pg_catalog.version()` replay (C2-M5, C2-M6).
6. Detect the catch-all from the parsed route, and give transaction-control denials a real fix (C2-M7, C2-M8).
