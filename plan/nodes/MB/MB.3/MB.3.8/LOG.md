# Log MB.3.8

## 2026-11-14T01:09 [planner]

created (task) Release prep 0.2.1: version bump, dated changelog, ARCHITECTURE sync

## 2026-11-14T03:37 [planner]

edited fields

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.

## 2026-11-14T16:36 [unknown]

edited fields

## 2026-11-14T18:54 [lead]

INPUT from MB.3.25 review (2026-11-14): ARCHITECTURE.md §7.3 is now stale against seccomp.rs — it says every refusal is EPERM with no clone3->ENOSYS exception, and lists neither clone CLONE_NEW* EPERM, setsid/setpgid EPERM, nor SeccompRole::EmbeddedView (execve/execveat/fork/vfork/non-thread clone). Update §7.3 in this node's ARCHITECTURE sync; design R14/§5.1.3 is the truth.

## 2026-11-14T21:04 [lead]

INPUT from MB.3.10 review (2026-11-14): docs/design/static-and-spa.md:41 still says the PHP worker is 0.2.1; the release folded into 0.2.0. static-and-spa.md is in this node`s paths — fix in the 0.2.0 sync.

## 2026-11-14T21:10 [lead]

RELEASE CHECKLIST ITEM (owner 2026-11-14, verbatim): add to the 0.2.0 certification: 'no / under a static root is servable (404, protected ), proven before the 0.2.0 tag.' Certification fails without it. Implemented by MB.3.33; MB.9.2.2 must preserve it.

## 2026-11-14T21:10 [lead]

RELEASE CHECKLIST ITEM (owner 2026-11-14, verbatim): add to the 0.2.0 certification: "no `.php`/`.phar` under a static root is servable (404, protected `critical_static_entry`), proven before the 0.2.0 tag." Certification fails without it. Implemented by MB.3.33; MB.9.2.2 must preserve it. (Reposted because the shell ate the backticked names in the first line.)

## 2026-11-15T08:32 [lead]

TWO REQUIREMENTS CARRIED IN BY THE OWNER FOR THIS NODE (binding, not optional):
(1) The 0.2.0 CHANGELOG entry must cover: static assets, the pooler presets, embedded PHP, the portable standalone binary, and the fail-open fix (MB.3.53: `body = "raw"` without `allow_raw` now refuses at boot in production). It must be honest that PHP needs the host's `libphp` — no bundling, no fallback — and name the package to install per distro.
(2) The VISION/ARCHITECTURE/security sync (m11-R7) is in scope, and `docs/security.md` now has two things it did not have before: (a) `raw` refuses at boot without `allow_raw` (ADR-0011, MB.3.53) and (b) the embedded-PHP threat model — no CGI surface, no request-parameter stream, SCRIPT_FILENAME pinned in C (ADR-0010). If those pages still describe the pre-0.2.0 world, the release ships documentation that contradicts the code, which this milestone has already paid for twice.

## 2026-11-15T09:18 [lead]

status ready -> active

## 2026-11-15T09:18 [lead]

attempt 1/3 started

## 2026-11-15T09:18 [lead]

base 6ba10d3

## 2026-11-15T09:25 [lead]

status active -> review

## 2026-11-15T09:28 [lead]

edited fields

## 2026-11-15T09:28 [lead]

REVIEW FAIL (attempt 1) — the owner has already ruled on how to close it, so follow this exactly:
MAJOR (docs/VISION.md:25): the bullet promises one CI pass over "every ... outgoing query/call", but `audit_v1.rs:1360` drops every `ObsTarget::Routed` statement, so `check --learn` cannot add a grant to an existing route. RESULT.md's "no false VISION sentence" is therefore WRONG. The owner's ruling: DO NOT EDIT docs/VISION.md (strategist-owned) and DO NOT block the release on MB.3.51's leaves. Instead: (i) correct RESULT.md to state the overclaim and record the reviewer's replacement sentence verbatim as the escalation item; (ii) make sure no SHIPPED material repeats it — CHANGELOG/ARCHITECTURE/security must say what is true today (`dev --learn` writes; `check --learn` proposes new routes only; the grant-for-an-existing-route gap is 0.2.x and is designed in docs/design/learn-single-pass.md). RESULT must quote the exact VISION replacement:
"**Single-pass wire learning (`--learn`):** Because Blastit sits on the HTTP ingress and every outgoing wire protocol, it records every route hit and outgoing query/call across an entire test run in one pass — language-agnostically (Python, PHP, Node, Java) — instead of failing on the first query per request; `dev --learn` writes those grants, while `check --learn` proposes new routes only, and a grant for a statement on an existing route is still the 0.2.x gap."
MINOR 1 (now in your paths): `docs/decisions/README.md:16` and `ADR-0010:14,:69` still label embedded PHP `0.2.1` although 0.2.0 ships it — the exact "0.2.1 adds …" class this node rules to 0.2.0. Change the three markers to `0.2.0` only; do not rewrite history in those ADRs beyond the version marker.
MINOR 2: NODE.md's certification item 2 says "proved by a protected test". Only stage 1 is (stage 0 is the `ServeGates` compile proof). A planner is fixing the body; your RESULT must carry the accurate wording: "stage 1 by protected test (`critical_raw_ingress`), stage 0 by the `ServeGates` type — `ServeGates::clear` is the only constructor and IS `raw_ingress_gate`, `bind_view_sockets` demands the token — so deleting or reordering it is a compile error, not a runtime test; stage 0's call site is unwitnessed at runtime."
CERTIFICATION: both items were judged PASS with that limit — keep the citations as they are.
VERIFY again: `make check` exit 0, `make docs-check`, `scripts/check-release-tag.sh v0.2.0`, `scripts/release-notes.sh --require-dated v0.2.0`, `plan.py validate`.

## 2026-11-15T09:28 [lead]

status review -> ready

## 2026-11-15T09:28 [lead]

status ready -> active

## 2026-11-15T09:28 [lead]

attempt 2/3 started

## 2026-11-15T09:31 [lead]

status active -> review

## 2026-11-15T09:31 [lead]

OWNER RULING on the 0.1.1 changelog entry (supersedes attempt 2's in-place rewrite):
1. A changelog is a record of what was claimed at the time. Do NOT rewrite the released `0.1.1` bullet. RESTORE its original text verbatim — `- **Fill the manifest in one pass.** `blastit check --learn` with a test suite, or `blastit dev --learn` while you click through the app, records the routes and statements it sees instead of denying at the first gap, adds them to `blastit.toml`, and prints the path it wrote ([fill the manifest in one pass](…)).` — and append a short dated correction inside the 0.1.1 section, e.g. "(corrected in 0.2.0: `check --learn` proposes new routes; grants for statements on an existing route are written by `dev --learn` and are the 0.2.x gap — MB.3.51.)". The original sentence must stay visible.
2. `ARCHITECTURE.md` is a LIVING document — keep attempt 2's in-place correction there (that was right).
3. The VISION amendment is ALREADY DONE: the strategist committed 5332f07 ("Qualify VISION.md single-pass --learn scope (dev vs check)") and it is HEAD of this branch. Remove the escalation item from RESULT.md and say the amendment landed as 5332f07, qualified with the reviewer's sentence. Do not edit VISION.
4. Release-notes consequence: `scripts/release-notes.sh --require-dated v0.2.0` reads the changelog, so `## 0.2.0 (2026-11-15)` must stay the TOP `## ` heading and the 0.1.1 annotation must live INSIDE the 0.1.1 section, below its heading — not above 0.2.0, not in 0.2.0's section.
Keep: the three `0.2.0` version markers in docs/decisions, the two-proof certification wording, the 0.2.0 entry's five areas and libphp honesty.
VERIFY: `make check` exit 0, `make docs-check`, `scripts/check-release-tag.sh v0.2.0`, `scripts/release-notes.sh --require-dated v0.2.0`, `plan.py validate`, and `grep -n "^## " CHANGELOG.md | head -3` to prove the heading order.

## 2026-11-15T09:35 [lead]

status review -> done
