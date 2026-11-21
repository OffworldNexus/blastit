# Security review — docs/design/blastfile-dsl.md (final acceptance round)

Lens: **security**. Fresh review of the current design, `ADR-0012`, the two
accepted DX pages (`docs/blastfile.md`, `docs/reference/blastfile.md`) and the
code the design cites. The retired TOML format must not exist at all; nothing
below asks for a second reader, a detector, a converter or a migration path.

Ground truth checked in this worktree: `blastit-manifest/src/{model,validate,uid,routing,error,edit}.rs`,
`blastit-scheduler/src/{route,rewrite,static_asset,static_response,path}.rs`,
`blastit/src/{stage0,stage1,supervise,check,config,dev_learn,visibility}.rs`,
`blastit-sys/src/sock.rs`, `blastit-http/src/head.rs`,
`blastit-py/src/{lib,audit}.rs`, `python/blastit/{fold,pytest_plugin}.py`,
`Makefile`, `Cargo.lock`.

Counts: **1 BLOCKER · 9 MAJOR · 7 MINOR** (the MINORs are listed separately and do
not carry the verdict).

**A revision-integrity problem frames every finding.** §22 gained a new "Round 4
(final acceptance round)" table whose security rows record `static_forward`,
duplicate directives, the fallback dotfile check, `root`'s dotfile rule and the
sandbox/socket change as **accepted** with named sections changed. The body was
not updated: §9.3, §6.7, §9.1, §9.2/§17.3 and §10.3 still read exactly as they
did before that table, and §21's leaf paths do not name several files the table
says a leaf owns. This review quotes the current body; a claimed resolution that
is not in the section it names is graded on the section, not on the table.

---

## S1. The erasure leaf cannot touch the files that hold the retired reader and its TOML synthesis [BLOCKER]

**Where:** §15.2 rows (current text):

> | the retired reader (`parse.rs`) and `Manifest::from_toml_str` (which lives in
> `model.rs`, §610) | `blastit-manifest` | `MB.21.4.5` |
> | the TOML **text synthesis** in `stage1::learn_routing_table`, which builds a
> manifest string and re-parses it to get the learn routing table |
> `crates/blastit/src/stage1.rs` | `MB.21.4.5` — it becomes a model-side
> `RoutingTable::for_learn(&manifest)`, no text |

and §15.2's own promise at :1303: "it is why `crates/blastit-py/**`,
`crates/blastit/src/stage1.rs`, `python/blastit/{fold,pytest_plugin}.py` … are in
the erasure leaf's / `MB.21.5.1`'s paths rather than forgotten."

**Why it is live:** §21's leaf `MB.21.4.5` (:1685) has exactly these paths:

> `crates/blastit-manifest/src/parse.rs` (deleted),
> `crates/blastit-manifest/Cargo.toml`,
> `crates/blastit-py/src/{lib,audit}.rs`,
> `python/blastit/{fold.py,__init__.py}`, `Cargo.toml`/`Cargo.lock`,
> `python/tests/test_fold.py`

No `crates/blastit-manifest/src/model.rs`, no `crates/blastit/src/stage1.rs`, no
`python/blastit/pytest_plugin.py`. The code is exactly where §15.2 says:

- `crates/blastit-manifest/src/model.rs:610` `pub fn from_toml_str` (and
  `edit.rs:364`/`:977` call it);
- `crates/blastit/src/stage1.rs:403` calls `learn_routing_table(&manifest)`;
  `:739–766` builds `version = 1` / `[[route]]` text and re-parses it with
  `Manifest::from_toml_str`; `:771` is the `toml_basic_string` serialiser.

**Failure mode:** AGENTS rule 5 and the plan system are explicit — a leaf may
touch only the globs in its `paths`. The erasure leaf is created with the §21
row; it cannot edit `model.rs` or `stage1.rs`. Either the implementer illegally
widens the leaf (the plan guard fails), or `from_toml_str` and the synthetic TOML
survive; the `--learn` dev boot at `stage1.rs:403` then re-parses the retired
format at runtime. The owner's decision — "delete every reference, delete all the
code" — is not reached. §15.2's statement that `stage1.rs` is in the erasure
leaf's paths is contradicted by §21.

**Fix:** add `crates/blastit-manifest/src/model.rs`,
`crates/blastit/src/stage1.rs`, `python/blastit/pytest_plugin.py` (and the
`edit.rs` call sites to the leaf that owns `edit.rs`) to the owning leaf's
`paths`; replace `learn_routing_table` with the model-side
`RoutingTable::for_learn(&manifest)` (owned by the `routing.rs` leaf); delete
`from_toml_str` from `model.rs`; add a gate/test asserting no `from_toml_str`,
`toml_basic_string` or `version = 1` synthesis remains and that the learn path
builds its table from the model. Reconcile the guard: `ADR-0012` decision 14 still
names `scripts/check_retired_format.py` (gate `make check`), while §15.2/R29 name
the pytest in `python/tests/test_docs_assets.py` (run by `make docs-unit`); the
design must be one artefact, and the ADR updated, or two guards with different
sets will drift.

---

## S2. The Python audit/plan readers are re-typed in prose, but no leaf owns the file or the replacement entry [MAJOR]

**Where:** §15.2:

> | the Python readers: `audit_report(manifest_toml, …)` / `check_audit_report`
> / `plan_learn_report` | `crates/blastit-py`, `python/blastit` |
> `MB.21.4.3` (the parameter becomes the Blastfile source, read by the same
> parser) |
> | `fold.py`'s and `pytest_plugin.py`'s own `tomllib` parsing and verification |
> `python/blastit/{fold,pytest_plugin}.py` | `MB.21.4.5` |

and §13.3: "`MB.21.4.3` (a) adds one PyO3 function,
`blastfile_apply_edits(source, edits_json) -> str`".

**Why it is live:** §21 `MB.21.4.3` (:1683) paths are
`crates/blastit/src/{audit_v1,check,dev_learn}.rs`,
`crates/blastit/tests/critical_check_engine.rs`, added:
`crates/blastit-py/src/lib.rs`, `python/tests/test_check.py`. The function that
parses the manifest is in `crates/blastit-py/src/audit.rs:414`
(`Manifest::from_toml_str(manifest_toml)`), and the real callers are
`fold.py:2125` (`_blastit.audit(new_text, observations)`),
`pytest_plugin.py:2309` (`_blastit.audit(session.manifest, …)`), with the
`tomllib` readers at `fold.py:1558/1812/1972` and `pytest_plugin.py:720/1782/1823`
and the uncapped `read_text` at `pytest_plugin.py:496`. `audit.rs` is in 4.5's
paths but not 4.3's; `pytest_plugin.py` is in neither.

**Failure mode:** 4.3 cannot change the `audit.rs` parameter (out of scope), and
4.5 deletes the reader without the design having named a Blastfile-taking
audit/plan entry — `blastfile_apply_edits` is an editor, not a manifest reader.
`fold`'s planning/verification and `pytest_plugin`'s audit either keep
`tomllib`/`audit(manifest_toml)` (the retired reader, owner-forbidden) or stop
working. The `fold` trust boundary — "Nothing a log says is trusted as manifest
text; fold never creates a route" — has no implementation to enforce it, and a
forged route name from a `sql.denied` line can become a `ProposedView`.

**Fix:** name the Blastfile-taking audit/plan entry (the natural one is the
existing `audit` / `plan_learn` surface with `source: &str`), make 4.3 own
`crates/blastit-py/src/audit.rs` and `python/blastit/{fold,pytest_plugin}.py`,
make 4.5 own `pytest_plugin.py` + its tests, and add a test that no `tomllib`,
`manifest_toml` or uncapped manifest `read_text` remains in `python/`.

---

## S3. `static_forward` is still "no `fallback`", so a fallback shadows every view [MAJOR]

**Where:** §9.3 :764: "`StaticPolicy.spa` is replaced by
`fallback: Option<String>`; `static_forward` becomes **"no `fallback` and
something to forward to"**". This directly contradicts §9.2 step 1 ("`Missing` →
**step 2, not `404`**") and the closing warning ("`Missing` at step 1 must not
become `404` (that would shadow every view under `/`)"). §22's Round 4 table
claims "the flag disappears with per-route static; the view table is always
consulted"; §9.3 still says the opposite.

**Why it is live:** `crates/blastit-scheduler/src/route.rs:78–83` sets
`static_forward = policy.is_some_and(|p| !p.spa && forwards)`;
`scheduler.rs::static_head` passes that as `has_app` to `rewrite::decide`, whose
`StaticOutcome::Missing` arm returns `Reject { 404 }` when `has_app` is false.

**Failure mode:** the accepted page's own shape
`static "/" { root "public"; fallback "index.html" }` plus
`view api "/api/*"` gives `static_forward = false`; `/api/x` is a 404 at step 1
and step 2 never runs. A site with a 404 page silently loses every app route — the
exact mount-shadows-view failure §9.2 and §17.6 depend on.

**Fix:** delete the "no `fallback`" clause; `static_forward` is "this mount has
something to forward to" (the app or the one PHP entry); `fallback` is consulted
only at step 3. Add the root-mount + API-view case to `critical_rewrite.rs`.

---

## S4. Two `body` directives (or two grant blocks) in one view are legal with unspecified meaning [MAJOR]

**Where:** §6.1 :348–352: `view_block → '{' view_item* '}'`,
`view_item → endpoint | view_directive`,
`view_directive → body | use | egress | prefork | store_grant`. §6.7 has no
duplicate-directive row (only "a slot written twice" and "a second `app`/
`entrypoint`"). §10.1 says "The spelling, and only one spelling"; §10.3 uses
`ingress = endpoint.body.or(view.body)` (singular); §13.1 says the grant "goes into
`Route.grants[ProxyName("analytics")]`". Round 4 claims "§6.7 (three refusal
rows)"; they are not in §6.7.

**Failure mode:** `view upload { body json; body raw unvalidated; GET "/u" }`
parses; whether the effective codec is `json` or `Raw(allow_raw = true)` is
unspecified (first-wins, last-wins, or lower's iteration), so a reviewer can read
`body json` and approve a file whose production policy is raw pass-through — a
policy differential on the one directive whose production boot refusal is a
security gate. Two `postgres { … }` blocks are equally unstated: replace (a grant
silently disappears) or merge (a grant silently widens).

**Fix:** one refusal row — "a directive is written twice in one view; `use` is the
only directive that may repeat (it is deduplicated)" — or an explicit
first-wins/merge rule per directive. Add the `body json` + `body raw unvalidated`
case to `critical_blastfile_parse.rs`/`critical_blastfile_body.rs`.

---

## S5. The request-time dotfile check for `index`/`fallback` is claimed, not implemented [MAJOR]

**Where:** §9.2 step 3: "A `fallback` that is missing, a dotfile, a symlink or a
`.php`/`.phtml`/`.phar` file is `404` final". §17.3: "`index` and `fallback` …
carry the shape rules in §9.1 **and** the same `Hidden` classification at request
time". §22 S1: "the resolved file still passes `Hidden` at request time (§9.2
step 3)". Round 4 claims "§9.3 adds `is_dotfile`/`is_php_source` on the resolved
fallback candidate"; §9.3 contains no such call.

**Why it is live:** in `crates/blastit-scheduler/src/static_asset.rs` the dotfile
rule runs **only on the request path**: `head_segments` calls
`is_dotfile(&parts)` (:644). A candidate answer is probed without it:
`resolve` calls `probe(&candidate)` and `missed` calls `probe(&self.index)`;
`probe` checks only `rel.rsplit('/').next().is_some_and(is_php_source)` (:477)
plus the regular-file classification. `openat2` happily opens `.env`; `classify`
accepts a regular file.

**Failure mode:** validation refuses a dotfile `index`/`fallback`, so the normal
boot is safe — but the design's own §4.4 `from_blastfile_str` (parse+lower, **no
validate**) and the `docs_fences` statement class build `Manifest`s without
`validate`. A `fallback ".env"` built that way under `root "."` is served at
every miss, and the second line of defence the S1 remediation relies on does not
exist. An untested second layer described as existing is how the first one gets
deleted later.

**Fix:** extend `probe`/`missed` with the same `is_dotfile` check on the
candidate (one function call), and add `fallback ".env"` to the §9.2 assertions;
or say plainly that dotfiles are refused by validation only and delete the
request-time claim from §9.2/§17.3.

---

## S6. `root`'s dotfile rule is not today's rule, is defeated by a symlinked root, and its boot audit was not re-pointed [MAJOR]

**Where:** §9.1 :702: "`root` **keeps every rule it has today**: required,
relative, no leading `/`, no `..` segment, no `\`, no control byte, **no dotfile
segment**; and it is audited at boot and by `check` exactly as today (§9.6)."
The accepted page's `root` row (`docs/reference/blastfile.md:380`) lists no
dotfile rule; §22 S13 says "stated as a **new** hardening rule with a test"; Round
4 claims "§9.1 states it as a **new** rule"; §9.1 still says "today".

**Why it is live:** `validate.rs::check_static_root` (:392–414) has only
`RELATIVE_PATH`, `NO_BACKSLASH`, `NO_PARENT_SEGMENT`, `NO_CONTROL_BYTE` — no
dotfile branch; the dotfile branch is in `check_static_index` (:436, `segment.
starts_with('.')`). `StaticRoot::open` follows a root symlink — "the only place a
symlink is followed, and only on the operator's own declaration". §9.6 names only
`check_static_roots` (readability); the production writability refusal lives in
`stage0::php_trees` (:1379–1388, built from `route.static_policy()`, removed by
§4.3) and `stage0::deployment_boot_check` → `Provenance::check_trees` (test
`a_deployment_boot_refuses_a_tree_a_route_uid_could_write`, :5900+).

**Failure mode:** four distinct ones.

1. If the implementer follows "today" and preserves `check_static_root`, no rule
   is written: `static "/cfg" { root ".ssh" }` serves `/cfg/id_rsa` (the request
   path has no dot; the per-request dotfile rule cannot help).
2. If the rule is copied from `check_static_index` (`starts_with('.')`), it
   refuses `root "."` — the design's own S1 example (`fallback ".env"` with
   `root "."`) and a shape `check_php_entry` explicitly supports — plus
   `root "./public"` and the real build outputs `.output/public` (Nuxt) and
   `.vercel/output/static`.
3. A symlinked root `config -> .ssh` bypasses the spelling rule entirely: `root
   "config"` is clean, the boot follows the link, and `/id_rsa` is served. The
   rule must apply to the resolved root, not just the manifest text.
4. If `php_trees`' static arm is deleted with `Route::static_policy()`, the
   writable-root refusal silently disappears for a PHP app's mount: a route uid
   that can write the mount root serves attacker-modified `app.js`/`index.html`
   from the app's origin. The design's "exactly as today" has no named function
   or test for this.

**Fix:** state the rule as **new** (drop "today"); define "dotfile segment" as a
leading-dot segment other than exactly `.` (decide explicitly about known build
dirs, and refuse `.ssh`/`.git`/`.env` dirs); resolve the root at boot and refuse
a dotfile component in the resolved path; name `stage0::php_trees`/
`deployment_boot_check` in §9.6/§21 and re-point the root claim to
`Manifest::static_roots`; add validation/boot tests for `root "."`,
`root ".output/public"`, `root ".ssh"`, a symlinked root to `.ssh`, and the
writable mount. Update the accepted page's `root` row in `MB.21.5.1`.

---

## S7. The sandbox/uid change still leaves socket binding and worker spawn keyed by route index [MAJOR]

**Where:** §10.3 :872: "`UidLayout` numbers routes by index today; it is changed to
number by `sandbox`, so the two `cart` routes run in **one** uid … A test asserts
both routes report the same uid, and one `view` still yields exactly one spawned
worker." Round 4 claims "§10.3 (one uid **and** one socket directory per
`sandbox`)"; §10.3 names no socket directory.

**Why it is live:** `crates/blastit/src/stage0.rs::bind_view_sockets`
(:3363–3373) loops `manifest.routes().iter().enumerate()` and binds
`layout.view(index)` per route; `make_private_dir(view_dir(uid))` runs per route.
`crates/blastit-sys/src/sock.rs` (:385–400) `unlinkat`s an existing socket at the
path. `crates/blastit/src/supervise.rs:237` keeps `route_pids:
HashMap<usize, i32>` and `spawn_now` (:558–566) chooses
`dev.view_dir(index)`/`view_dir(info.uid)`; `dev_learn.rs:128` names the dev dir
by route index; `visibility.rs::DroppedTo::of(layout, routes)` takes a route
count.

**Failure mode:** with one view lowering to two `Route`s sharing one `sandbox`,
the second route re-enters `bind_view_sockets` for the same uid: the second
`bind_unix_listener` removes the first endpoint's socket from disk and rebinds,
leaving the scheduler holding a `ListenerSpec` whose path now names the other
listener. `SPAWN` is still per route index, so the supervisor can fork a second
worker for one uid; in dev the two endpoints get different `views/<index>`
directories, so "one sandbox for several methods" is false there. A mixed
`layout.view(sandbox)` / `layout.view(route_index)` change can map two sandboxes
to one uid, putting two views' sockets and grants in one `views/<uid>` directory
(last bind wins) while `check_uids` only checks the upper bound — a cross-view
grant mix-up, not just a wasted fd. The only named test
(`uid.rs::two_endpoints_share_one_uid`) tests the layout, not the socket
directory, the listener set or the worker count.

**Fix:** iterate **sandboxes** in `bind_view_sockets`/`dev_view_sockets`; send
SPAWN once per sandbox (or key `route_pids`/`pending_spawns` by uid); change every
`layout.view(...)` call site (stage0, stage1, supervise, visibility, route.rs)
together; add a test that a two-endpoint view yields one uid, one `views/<uid>`
directory, one socket set and one worker, and that a following view's uid is
distinct.

---

## S8. `TRACE` reaches a method-less view; the design's boundary guarantee is false [MAJOR]

**Where:** §12.2 :963: "a *declared* `TRACE`/`CONNECT` is refused for every view
at lower time (§8.4), and **a request carrying either is refused at the boundary
(`405` from `RoutingTable::lookup`)**". §9.2 step 4 defines the 405 as "when a
view's pattern matched but no declared method did"; §8.4 refuses only a
**declared** `CONNECT`/`TRACE`.

**Why it is live:** `crates/blastit-manifest/src/routing.rs:122–137` skips a
route only when `Some(methods)` does not contain the method; `None` (a view with
no method list) matches **any** method. `crates/blastit-http/src/head.rs:264–265`
special-cases only `CONNECT` (501); `TRACE` is not refused anywhere.
`validate.rs::check_php_declarations` (:668–675) today refuses only a declared
`TRACE`/`CONNECT`, and the design deletes that rule for the one global PHP entry.
The accepted page says the opposite of safety: "A view with no method answers
every method, including ones you did not intend" (line 169).

**Failure mode:** `view app "/*"` (or the patterns-only header, which the design
adds) matches `TRACE /x`; the PHP worker or WSGI app receives
`REQUEST_METHOD=TRACE` and can echo the request headers (Cross-Site Tracing),
while the design's boundary sentence and the dropped PHP `TRACE` rule say this
case is already refused. A reviewer relying on §12.2 removes a defence that is
not there.

**Fix:** refuse `TRACE`/`CONNECT` in `RoutingTable::lookup` (or at the
`blastit-http` boundary) for every view regardless of its method list, or keep
the per-view PHP declaration rule and document the method-less case explicitly.
Add `TRACE` on a method-less view to `critical_rewrite.rs` and
`critical_php_manifest.rs` (expected `405`/`501`, never a worker).

---

## S9. The 1 MiB cap is bypassed by every boot reader except the one the design names [MAJOR]

**Where:** §4.5 :236: "**Every entry point** goes through `blastfile::read(path)`,
which stats first, so the cap costs one `stat` and never a multi-megabyte
allocation." R3a; §17.1 property 2: "a malformed file cannot exhaust the boot …
refused by construction."

**Why it is live:** the manifest is read with `std::fs::read_to_string` at
`crates/blastit/src/stage0.rs:2552` (production boot), `stage0.rs:2844` (`dev`),
`stage1.rs:255` (stage-1 re-read), `config.rs:2405` (`manifest_wiring`),
`dev_learn.rs:442`; only `check.rs:296` is the `read` caller the design names.
The Python legs bypass it too: `fold.py:1816` and `pytest_plugin.py:496` (plus
the `tomllib` reads at `pytest_plugin.py:1784/1823`). The R3a critical test
exercises `read` in isolation, not a boot.

**Failure mode:** a 2 GiB `./Blastfile` or `BLASTIT_MANIFEST` is allocated before
`parse_blastfile` can check `len()`; a FIFO named `./Blastfile` (or a growing
file) blocks/streams forever because `read_to_string` has no size or type check.
The privileged stage-0 boot — the one the cap exists to protect — is the least
protected path. The claim "every entry point" is contradicted by the tree.

**Fix:** make `blastfile::read` the only manifest reader in `stage0`, `stage1`,
`config`, `dev_learn` and the Python legs (or give the Python binding a
`read`-based entry); name those call sites in §4.5/§21; add a boot test with an
over-cap regular file and a FIFO (both a clear refusal, no allocation/block) and a
source gate for manifest `read_to_string`/`read_text`.

---

## S10. `InvalidApp`/`PhpEntryInvalid` echo a pasted value, contradicting R26/§18 [MAJOR]

**Where:** §18 :1503: "no value that could be a credential ever quoted (§17.1)";
R26: "never quotes a value where the value could be a secret". §5.3 fixes only the
`postgres "…"` slot ("the literal is **never** echoed").

**Why it is live:** `crates/blastit-manifest/src/error.rs:133` renders
`InvalidApp` as `app {quoted(.app)}: must be module:attribute`, and `:499`
renders `PhpEntryInvalid` as `{location}: php entry {quoted(.entry)}: {rule}`.
`validate.rs::check_app` (:92) pushes the whole `entrypoint python` string;
`check_php_entry` (:603) pushes the whole `entrypoint php` string. `quoted` cuts
at 120 chars and escapes control bytes; it does not redact a DSN or token.

**Failure mode:** `entrypoint php "postgres://app:hunter2@db/shop"` (a plausible
paste into the string-valued entrypoint slot) fails `MUST_END_IN_PHP` and the
boot/`check` prints the DSN — password included — into a CI log. `entrypoint
python "sk-live-…"` (no colon) fails `InvalidApp` the same way. The design's own
reason for not echoing `postgres "…"` (a pasted DSN can be a credential) applies
unchanged; R26 claims the whole error surface.

**Fix:** redact `InvalidApp` and `PhpEntryInvalid` (name the slot and the rule,
not the value), state R26 as "never a value in a value position (a `$IDENT`
literal, a `query`/app/entry literal)", and add an R26 test with a DSN/token in
each of `postgres`, `entrypoint python`, `entrypoint php`.

---

## Checked and closed (fairness)

- The `postgres "…"` refusal no longer echoes the literal (§5.3), and
  `MB.21.5.1` is assigned the accepted page's `$IDENT` row correction.
- Mount prefixes match on a segment boundary, the longest prefix answers, and
  `Missing` at step 1 falls through to views (§9.2).
- Step 3's fallback is `GET`/`HEAD` only (a declared body is `413`; other methods
  reach step 4), and `Hidden`/`Refused` are final (§9.2).
- `blastfile::read` refuses a non-regular file and reads `take(MAX + 1)`; the
  `set_len`/mode-`000` oracle is the right observable (R3a).
- `raw_ingress`'s `fix.to` is a node locator; the payload carries no
  `unvalidated` (§13.3).
- `..`/`//` are scoped before mounts and views (§9.2/§17.3); the missing named
  test is a MINOR, not a re-open.
- `MAX_TRIVIA`, the `Dot` token and `pg_catalog.` handling are specified.
- The erasure **intent** is correct: no compatibility path is requested below.
  Every finding above is about the replacement seam or a claim/leaf mismatch,
  not about keeping TOML.

## Rejected with owner decision (recorded, not graded)

- No second reader, detector, `--format` flag, converter or migration sentence.
  S1/S2 ask for the files and the Blastfile-taking audit/plan seam, not the old
  format.
- `body raw unvalidated`, the `workers` removal and the `static` mount model are
  owner decisions and are not re-opened.
- `--learn` never writes a codec (`ProposedView` has no `body` field) is the
  correct structural rule; §13.3's node-locator `fix.to` is not a pasteable
  acknowledgement.

## MINOR (unchanged, not verdict-bearing)

1. §18 "a name or a number, never a literal" still contradicts §8.3's quoted
   regex/pattern and §6.7's quoted names; word it "never a value in a value
   position" (the `InvalidApp` finding is the MAJOR form).
2. The pre-routing `..`/`//` refusal has no named critical test (`critical_rewrite.rs`).
3. §9.4 `url_path = prefix + "/" + relative` double-slashes a `"/"` mount and
   loses `immutable` for the page's own root-mount example; pin the empty-relative
   collapse.
4. `Str`/`RawStr` still admit NUL/C0 (other than the allowed `\n`/`\t`) into
   `query`/`regex`; refuse at lex time.
5. The non-regular `read` refusal still has only the size sentence; add
   "`<path>: not a regular file; Blastfile must be a file, not a pipe or a device`".
6. `--learn`'s `ProposedView.name`/`patterns` derivation from an observed path is
   still unpinned (P4 fails closed, so MINOR).
7. §10.2's boot `body raw` sentence still spells the exact acknowledgement for
   the operator, the channel `ADR-0011` was written about; name the two legitimate
   uses or record the deliberate exception.

VERDICT: FAIL
