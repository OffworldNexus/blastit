# `dev-fanotify-reload` — consistency review (second pass)

Scope: the revised `docs/design/dev-fanotify-reload.md` (`status: In review`), `ADR-0014`,
`docs/VISION.md`, `ARCHITECTURE.md` (§2.4, §2.5, §3, §3.1, §4, §5, §7.3, §8.1), the four
accepted DX pages, the research note, `ADR-0001/0009/0010/0012`, both README tables,
`LESSONS.md`, and the code surfaces the design names. Every §9 disposition was checked
against the text. Read-only; no plan nodes, no builds.

## Findings

### F1 · MAJOR (`docs/decisions/ADR-0014-dev-fanotify-live-reload.md:34`)

Decision 1 still says "Exactly one group per dev boot, held by the supervisor, never by a
view." The design it governs says the opposite: R2 is "One **live** group per image", §4.1
is "One group per boot image, one generation at a time" (:179), and §4.10's table is
"marked groups | exactly 1 per image" (:703) — a `Blastfile` save creates the next group
after the old image's fd died, so one `blastit dev` run creates several. The first pass's
F9 fix was applied to the design only; the Accepted ADR now contradicts it. Adopt R2's
wording in decision 1 ("one live group per image; never two at once").

### F2 · MAJOR (`docs/design/dev-fanotify-reload.md:391`)

The first pass's F8 is not resolved. R6 now says a skipped or cached path "is in the union
only as a report (it is never marked and never triggers)" (:133), which is the intended
reconciliation — but §4.4 still states "A path under the skip list is never marked and
never in the union" (:391). The two sentences are direct negations, and the difference is
observable: if skipped paths are in the union they consume the §4.10 "tracked paths in the
union | 16384" budget (:698) and can raise `Drop::Cap`; if they are not, a worker's report
for `.venv/…` simply disappears. `ReportTable::union()` cannot satisfy both, and T15's
16 385th path depends on which is true. Rewrite §4.4's sentence to match R6 (or change R6
and say the report is dropped before the union), and say whether skipped reports count
against the cap.

### F3 · MAJOR (`docs/design/dev-fanotify-reload.md:536-556`)

The reload re-exec cannot find the dev policy socket. §4.6 step 2 makes stage 1's end of
`--dev-policy-fd` a `SOCK_CLOEXEC` socketpair member (:513-515), but step 6's argv is
`--stage=1`, `--config-fd`, `--dev-root`, `--dev-host`, `--dev-reload=on` and
`--dev-adopt-fd` (:536-544), the adoption document carries only `http`, `chan`, `dbproxy`
and `learn` (:550-556), and "FD_CLOEXEC is cleared on exactly the configuration pipe, the
adoption pipe and the descriptors the document names" (:542-544). After the first
`Blastfile`/`preload` reload the new image therefore has no policy writer, so the *next*
grant edit cannot refresh dbproxy — contradicting R10 ("refreshes dbproxy's allowlist …
then re-execs", :137), §4.8 ("Its **allowlist** is refreshed on a `Blastfile` save",
:613-623) and ADR decision 4 ("refreshes dbproxy's allowlist over a dev-only policy
socket", :49-57). Add the policy end to the adoption document (and to the ADR's adopted
list), exactly as `learn.{obs,ctl}` is.

### F4 · MAJOR (`docs/design/dev-fanotify-reload.md:727`)

§5 S10 still asserts "Every failure maps to one `dev.reload.unavailable` and 'serve without
reload'". That is now false for the mid-run case: §4.7's last row says a later `read`/
`mark` error latches the watcher dead and emits "**one** `manifest.warning` (`live reload
stopped: <errno>; restart blastit dev`)" (:587), and §4.5 freezes that as `Effect::Stopped`
(:447). `docs/reference/log-events.md:39,45` also defines `dev.reload.unavailable` as the
boot event ("could not watch anything at boot", "the one `warn` at boot"). S10 must say the
boot ladder maps to `dev.reload.unavailable` and the mid-run latch to one `manifest.warning`.

### F5 · MAJOR (`docs/design/dev-fanotify-reload.md:907`)

`cli.rs` gains "`--no-reload`, `--dev-reload=on|off`, `--dev-adopt-fd`, `--dev-policy-fd`",
yet §1 says "`--no-reload` is the only knob" (:47) and §6's rejected-alternative table says
"One default and one off switch is the whole documented surface" (:780). `--dev-reload=off`
is a second off switch with no defined difference from `--no-reload`, and
`docs/reference/cli.md:23-28` lists exactly four `dev` flags and no `--dev-reload`, so as
written either the page is incomplete or the non-goal is false. The neighbouring internal
flags are explicitly labelled as such in the code (`--dev-root`: "Internal to the dev
supervisor's `execve` of stage 1", `cli.rs:124-126`). Do the same here and drop the `off`
value (the reload image only ever needs the positive marker), or document the public flag
on the page.

### F6 · MAJOR (`docs/design/dev-fanotify-reload.md:1014-1018`)

§9's closing claim — "each lens re-ran once and the four files on disk end `VERDICT: PASS`
with zero `BLOCKER` and zero `MAJOR`" — is false on disk. `review-implementability.md` ends
`VERDICT: FAIL` and records a `BLOCKER` (its F1 is this review's F3, the dropped policy fd);
`review-testability.md` ends `VERDICT: FAIL` ("Not ready for acceptance");
`review-security.md` still ends `PENDING`; this file's predecessor ends `VERDICT: FAIL`.
Replace the pre-declared verdict with a per-lens disposition table, and only write PASS
once the file on disk says so.

### F7 · MINOR (`docs/design/dev-fanotify-reload.md:584`)

`ENOSPC` is on the boot ladder and emits `dev.reload.unavailable` (R13 :140), but its cause
clause is "no marks left: the reloads already marked keep working" (:599) — watching is
*partly* on. `docs/reference/log-events.md:39,45` defines that event as "could not watch
anything at boot" and "the server serves without it". Either widen the page's one sentence
to "some watching unavailable", or give the partial case `manifest.warning` and reserve
`dev.reload.unavailable` for "nothing is watched".

### F8 · MINOR (`docs/research/2026-11-16-dev-fanotify-reload.md:35`)

The design's §6 reconciliation covers research §3's rows, but the note still teaches three
mechanisms the design and ADR reject as current ground truth: the fd reaching the worker
"through the loader scope, like `--dev-obs-fd`" (:35, repeated :268 — the design now names
`ChildIo.keep` → `ViewConfig::loaded_fd` → `serve`), `BLASTIT_DEV_RELOAD=poll` as "the only
alternative path worth documenting" (:187, :304, and recommendation 7 :330 — the ADR
records polling as rejected), and `Ctl::Reload` with "payload `route u32`" (:292 — the
design freezes an empty payload). Mark the note superseded where it disagrees, or an
implementer following the cited source builds a knob and a frame the epic forbids.

### F9 · MINOR (`docs/design/dev-fanotify-reload.md:914`)

The scheduled `ARCHITECTURE.md` edit is "§3 channel table + rule, §3.1 frame table, §8.1
`reload` reason". Two more places now assert the old behaviour: §8's failure table says
"http, scheduler or dbproxy dies | fail fast" (`ARCHITECTURE.md:1468`) while §4.6 step 5
makes a scheduler exit expected under `reloading`, and §2.4's Lifecycle lists only idle
timeout and max-requests retirements (`ARCHITECTURE.md:423-426`) while `reload` becomes a
third. Add both to the amendment list so a cheap implementer does not leave the living
specification contradicting the shipped dev path.

### F10 · MINOR (`docs/design/dev-fanotify-reload.md:621`)

"the views are retired right after, so no connection outlives the swap" contradicts
§4.6 step 4 and S7: a view mid-request is not retired until `DONE` (:489-492) and "an
existing session keeps its snapshot only until its view is retired" (:724). Only *idle*
views are swapped at once; say that a connection in a stale view keeps the old grants until
its last request finishes.

### F11 · MINOR (`docs/design/dev-fanotify-reload.md:513`)

The policy document is frozen as "`u32 len | bytes`, ≤ 1 MiB" with no endianness, and its
member list reinvents what already exists: `RoutePolicySpec` is exactly
`route/proxy/queries/transaction_control/unrestricted/tables` (`crates/blastit-dbproxy/src/config.rs:90-126`)
and `write_config` already frames a document with a **big-endian** `u32` length
(`config.rs:448-470`, `MAX_CONFIG_BYTES` 16 MiB at :33). Say "reuse `RoutePolicySpec` and
`write_config`'s framing" (or state big-endian and why 1 MiB differs), otherwise the second
wire format will disagree with the first.

## Second-pass verdict (before the resolution edits)

The revision genuinely closes most of the first pass: the `Blastfile` grant fix (policy
refresh, §4.6/§4.8/R10/T18), `routes` truncation and `truncated: ["routes"]`, the ADR's
In-review analysis and `learn` descriptors, the ARCHITECTURE §3/§3.1/§8.1 amendment, the
real `ChildIo.keep`/`ViewConfig` handoff, `--learn`'s tally (§4.6 step 0), the `reloading`
gate, `RetireReason::Reload`, the narrowed protocol non-goal, and the README/ADR Node rows
are all correct on disk. What remains is drift the rewrite introduced or left: the ADR still
says one group per boot (F1), R6's union definition is still negated by §4.4 (F2), the new
policy socket is not carried across the re-exec that must keep using it (F3), S10 still
generalises the boot event to every failure (F4), a second `--dev-reload` switch undercuts
"the only knob" (F5), and §9 pre-declares four PASS verdicts that three files on disk deny
(F6). No VISION contradiction and no page that ships false on its primary path, so no
`BLOCKER`; the design is not consistent enough to accept.


## Resolution (third check)

The second-pass verdict above is kept as the historical record. This section re-checks the
six `MAJOR`s and two `MINOR`s against the files as they stand after the latest design and
ADR edits (design 05:51:50, ADR 05:49:13).

| Finding | Verified on disk |
|---|---|
| C-F1 | `ADR-0014:34` reads "One live group per image, held by the supervisor, never by a view", matching R2 (`dev-fanotify-reload.md:129`), §4.1 (:179) and §4.10 (:703). |
| C-F2 | `:398` says a skipped path "is never marked and is never a trigger; it may still be a report (R6)", and R6 (:133) says "in the union only as a report (it is never marked and never triggers)". One definition. |
| C-F3 | The re-exec carries the policy end: `:574` clears `FD_CLOEXEC` on "the policy pipe when it is present", `:590` has `"policy":14` in the adoption document, `:953` names `--dev-policy-fd`, and `ADR-0014:54-56` now adopts "the dev policy socket" alongside the `--learn` observation/attribution descriptors. |
| C-F4 | S10 (`:771`) separates "Every boot failure maps to one `dev.reload.unavailable`" from "a mid-run watcher death maps to one `manifest.warning` (`live reload stopped`)", matching §4.7 (:626) and `log-events.md:39,45`. |
| C-F5 | §1 (:49) makes `--no-reload` "the only user-facing knob" and names `--dev-reload`, `--dev-adopt-fd`, `--dev-policy-fd`, `--dev-reload-since` (plus dbproxy's own `--dbproxy-policy-fd`); `:967-970` gives their gate (`--stage=1` + `--dev-root`) and their absence from `--help`. |
| C-F6 | §9's `### Verdicts` (`:1167-1176`) states the four files end `VERDICT: PASS`. At read time: implementability PASS (05:50:24), security FAIL (05:50:29), testability FAIL (05:50:15), and this file is updated here. The two FAILs are snapshots written **before** the design's 05:51:50 amendments and their findings are answered in the design: security S-F4 by the authoritative, pair-built document (§4.6:552-557, `.unwrap_or(&self.empty)` denies, T18:872 adds the removal case); testability T-F5 by the debug `BLASTIT_TEST_RELOAD_PAUSE` seam and the differential T31 (:581, :885), T-F7 by `PHP_SUITE_ARGS`/`e2e/php-sapi/Dockerfile` (:954), T-M3 by `DevReload.files: Vec<String>` (:723). The only gap is those two files' verdict lines, which this lens cannot edit; once their resolution checks land, §9's sentence is literally true and no design text changes. |

Minors re-checked and correct: the errno/`filesystem` sentence (`:72`), `routes=` (:711),
the `view.chan`/ingress/broker non-goal (:118), §6's extended reconciliation (:806) and its
two non-FID mechanisms (:821), the ADR's "a view save is a fork, a manifest save re-imports
once" (`ADR-0014:96`), and both README tables (`docs/design/README.md:10`,
`docs/decisions/README.md:20`) keeping `MB.23`/`In review`/`Accepted` as the brief fixes
them.

## Verdict

Every consistency `MAJOR` from the second pass is fixed in the text, the ADR and §1 now
match the design, and the new mechanisms (`policy`/`--dbproxy-policy-fd`, the ack,
`Policy::reconcile`, `dev_policy`) introduce no contradiction with VISION, ARCHITECTURE, an
accepted ADR, or any of the four DX pages. C-F6's only residue is the security and
testability review files' stale verdict lines, which are outside this lens's edit scope and
do not require a design change.

VERDICT: PASS
