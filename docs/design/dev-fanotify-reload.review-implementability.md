# Implementability review — `dev-fanotify-reload` (second pass)

Scope: `docs/design/dev-fanotify-reload.md` (revised §4/§7/§8), ADR-0014, the research
note and the four DX pages. Every code claim was checked against the named files; the
lens is cheap-model buildability: is every type, wire member and argv token frozen, and
can each §8 leaf land inside its listed paths without inventing a contract?

The first pass's B1 (report-channel seam), B2 (stage-1 re-exec), F3 (`ChildIo.keep` →
`ViewConfig::loaded_fd` → `serve(cfg: ViewConfig)` → global; `serve` does take the config
by value, `blastit-view/src/lib.rs:242`), F4 (receiver arms), F5 (teardown gates —
`component_exit:813`, `ctl_closed:491`, `signal_all:910` exist), F6 (`io::Result`),
F7 (boot order, `"unknown"` filesystem) are genuinely resolved in the text. F8 is only
half resolved and F9 is not (F6/F7 below).

## Findings

### F1 · BLOCKER (§4.6 step 6, lines 536–556; §4.8 line 615)

The dev policy socket cannot survive the re-exec. `--dev-policy-fd` is the supervisor's
only write end to dbproxy's allowlist reader, but the frozen adoption document names only
`http`, `chan`, `dbproxy` and `learn`, and says "an unknown key, a missing key or a
descriptor that is not open refuses the boot"; the enumerated argv is "`--stage=1`,
`--config-fd=N`, `--dev-root`, `--dev-host`, `--dev-reload=on`, and one **adoption
document**"; and `FD_CLOEXEC` is cleared on "exactly the configuration pipe, the adoption
pipe and the descriptors the document names". That set excludes the policy fd. dbproxy
keeps its read end (stage 0 is not re-run), so the *new* image has no way to address the
socket: the first `Blastfile` save refreshes grants, every save after the first silently
does not — contradicting R10 "A grant edit applies" and `docs/local.md`'s denial fix.
T18 does one save, so no test catches it. The fix (add it to the adoption document, or a
non-CLOEXEC argv token with the fd-clear rule) changes a frozen document the implementer
must not invent. **BLOCKER** because the walkthrough ends in a contract the design says
cannot exist.

### F2 · MAJOR (frame.rs:150–171, 177–183; ctl.rs:195–221)

"the compiler catches four of them: `TypeCode::from_code` … `TypeCode::is_ctl` …
`Frame::decode`'s ctl arm, and `Ctl::{type_code, write_payload, decode}`" (doc line 686)
is false for three of those arms:

- `from_code` ends `_ => Err(FrameError::UnknownType { ty: code })` (frame.rs:168) — a new
  `0x45` variant compiles clean and every `RELOAD` then fails as `UnknownType`;
- `is_ctl` is `matches!(self, Self::Spawn | … | Self::Drain)` (frame.rs:177) — compiles
  clean, and because `Ctl::decode` starts with `debug_assert!(code.is_ctl(), …)`
  (ctl.rs:195) a debug build **panics** on the first reload while release decodes it;
- `Ctl::decode` ends `_ => Err(FrameError::OutOfRange { … })` (ctl.rs:218).

Only `TypeCode::name`, `Frame::decode`'s ctl arm, `Ctl::type_code` and
`Ctl::write_payload` are compiler-enforced (plus `supervise::on_ctl`, whose second match
is exhaustive — good). A cheap implementer told to "follow the compiler" ships a dead
feature; T13 does catch it, which is why this is not a BLOCKER, but the design's claim
about the code is wrong and must name all seven arms, not "five sites".

### F3 · MAJOR (§8 MB.23.5, lines 913–914; dbproxy state.rs:128/198, learn.rs:268, server.rs:111/174)

The dbproxy leaf lists only `crates/blastit-dbproxy/src/{lib,config}.rs`, but the grants
the refresh must swap are materialised elsewhere: `state.rs:128` clones
`config.route_policies` per connection and `state.rs:198` hands out `&Allowlist`;
`learn.rs:268` builds a second `route_policies: HashMap<(String,String), Allowlist>`;
the runtime is `server.rs`'s tokio `run`/`run_with_listeners`, not `lib.rs`. None of
`state.rs`, `learn.rs`, `server.rs` is in the leaf's `paths` (or in §8's list), so the
leaf cannot land without touching files outside its lane. Also missing: the
`DbproxyConfig` member that carries the read end — it must follow `obs_fd`/`check_ctl_fd`
(`config.rs:153/159`) and the `secrets.pipe` document member that carries it, both of
which the implementer would have to invent; a wrong name is a silent `None` and a
silently dead refresh.

### F4 · MAJOR (§4.6 step 2, lines 512–523; dbproxy config.rs:90–106)

The policy document is not buildable as written without a second design:

- the array is called `"listeners"`, but the objects carry `route`, `proxy`, `queries`,
  `transaction_control`, `unrestricted`, `tables` — exactly `RoutePolicySpec`
  (`config.rs:90–106`). `DbproxyConfig::listeners` is a different type (`ListenerSpec`
  with fd/uid). An implementer greps `listeners` and wires the wrong struct; the "`fd`/
  `uid` members are ignored when present" sentence only deepens the collision.
- it is `SOCK_STREAM` with `u32 len | bytes`, but nothing says whether the socketpair
  carries one document per connection or repeated frames, how a partial read is
  accumulated, what timeout applies, or whether `version != 1` is "unparsable" (refused)
  or ignored. dbproxy is tokio; a blocking read on the runtime thread is a real failure
  mode with no stated `spawn_blocking`/`UnixStream` choice.
- "swaps the grants … atomically" names no store or lock (the per-connection clone in
  `state.rs:128` makes Arc/RwLock placement the whole design), and no rule for removed
  routes/grants or for `route_policies` entries with no listener.

### F5 · MAJOR (R6 line 133 vs §4.4 lines 389–402; `Drop` line 333)

R6 says a skipped or cached path "is in the union only as a report (it is never marked and
never triggers)"; §4.4 says "A path under the skip list is never marked and never in the
union" and "A path under a cache dir is dropped before the lookup". `Drop::Skipped` exists
in the frozen enum and implies not-applied, so `on_datagram`'s return is undecided for
exactly the paths §4.10 caps count. Behaviour (no mark, no trigger) is the same either
way, but `union()` is a frozen API and T15/T32 assert cap behaviour; an implementer
cannot know whether a skip-listed path consumes a `max_paths` slot.

### F6 · MAJOR (§8 lines 869–880, 917–920)

Leaf sizing is still not true. MB.23.4 is one leaf over twelve files in three languages
(the `open` hook and `sys.modules` harvest, the codec, the layout-free `ViewConfig`
change, `stage1.rs`, `php_view.rs`, the C sink, the ABI header, four Rust SAPI files and
two critical tests) — comfortably over 600 lines with no split pre-authorized. MB.23.5
admits 5b is "the bulk" (dev_reload.rs entire + supervise + stage1 adoption + zygote +
child + cli) and pre-authorizes a split only for 5c; the design's own `<=650` target then
cannot hold for 5b. A cheap implementer will either land a half-feature or stop.

### F7 · MAJOR (§8 lines 876–893; php-sapi loader.rs:56/65/229–303)

The new `blastit_php_file_sink_install` is "resolved with `dlsym` in the worker", but
`crates/blastit-php-sapi/src/loader.rs` — the one module that holds the shim handle, the
private `symbol()` helper and `Loaded` — is absent from §8's file list (`lib,request,abi.rs`
only). Worse, the shim is opened `RTLD_LOCAL` (loader.rs:56), so `dlsym(RTLD_DEFAULT, …)`
cannot see it; the lookup must use the shim handle that `Loaded` does not expose. Because
the design makes an absent sink a silent no-op, the likely implementation compiles,
no-ops, and only T24/T26 go red — a missing interface rather than a build error.

### F8 · MINOR (§4.5 lines 431–459)

`Policy::sync_marks(&mut self) -> MarkOutcome` is impossible as frozen: `Policy` holds
`ReportTable`, the window and the clock, but no watcher (line 435), while
`Reload::sync_marks` does have one — the duplicate is harmless once removed, but the
frozen shape invites a stub. Likewise `Reload::boot(cfg, watcher: Box<dyn ReloadWatcher>,
poll: OwnedFd, clock)` requires the caller to produce the `F_DUPFD_CLOEXEC` clone, yet
`ReloadWatcher` has "exactly two operations" (line 221) and no `fd()`; only the concrete
`Watcher::fd()` can make it, and the fake cannot. Finally §4.5 says "`Reload::boot`'s
`Err` is the §4.7 ladder", but `Watcher::new()` runs before `boot` (§4.1 step 2), so the
ladder has two entry points and the design names one.

### F9 · MINOR (§4.6 step 6, §8 CLI line 904)

The `--learn` reload path is not wired: the adoption document carries `learn:{obs,ctl}`,
but stage 1 today reads `args.dev_obs_fd`/`dev_ctl_fd` (cli.rs:778–779) and the re-exec
argv list omits both, so the design must state that the document populates those `Args`
fields (and that `--learn` itself is not re-passed). `--dev-reload=on` likewise needs the
`flag()` vocabulary (on/off) frozen, and `loaded.rs`’s encode/split function surface is
not frozen even though T28 drives it directly.

## Verdict

All second-pass findings are resolved in the current text. The cross-process policy
channel is now frozen end to end: stage 0's `--dbproxy-policy-fd=N` (role=dbproxy,
`--dbproxy-*` family) plus `DbproxyConfig::{policy_fd: Option<i32>, dev_policy: bool}`
reaches dbproxy, and `--dev-policy-fd=N` at boot / `"policy":14` in the adoption document
plus the CLOEXEC clause reaches the stage-1 supervisor before and after the re-exec. The
receiver arms are listed exhaustively and asserted by T13/T21, the dbproxy files and
`Arc<Allowlist>` swap are named, the union/Drop definition is single, the leaves are split,
and the PHP sink goes through the shim handle. Two MINOR documentation nits remain (§4.9's
"none of them is caught by the compiler" overstates — four of the seven are exhaustive
matches; §8 still says "five receiver sites"). Neither blocks a cheap implementer.

## Resolution (third pass, final)

- **I-F1 — resolved.** §4.6 step 6: adoption document member `"policy":14` ("present exactly
  when reload is on") and the CLOEXEC list includes "the policy pipe when it is present".
- **I-F2 — resolved; MINOR nit.** §4.9 lists all seven arms, names the `Ctl::decode`
  `debug_assert!` hazard, and T13/T21 assert each. The blanket "none … is caught by the
  compiler — every one is a `match` with a catch-all" is inaccurate for `TypeCode::name`,
  `Frame::decode`'s ctl arm, `Ctl::type_code` and `Ctl::write_payload` (exhaustive), but the
  actionable list is complete and test-enforced.
- **I-F3/F4 — resolved.** §4.6 step 2 freezes `route_policies` / `RoutePolicySpec` (no
  `ListenerSpec` collision), `Learning::replace_route_policies`, `Arc<Allowlist>`, the
  one-byte ack and the 100 ms `policy_ack` deadline/budget. The read end is now carried by
  `--dbproxy-policy-fd=N` (accepted only with `--role=dbproxy`, distinguished from the
  stage-1 `--dev-*` tokens) with `DbproxyConfig::{policy_fd: Option<i32>, dev_policy: bool}`,
  and a `--dbproxy-policy-fd` without `dev_policy: true` is a configuration error. §8 names
  `dbproxy/src/{learn,state,server,lib,config}.rs`, `stage0.rs` for both tokens, and the
  internal-token paragraph separates the two families. No carrier must be invented.
- **I-F5 — resolved.** R6 and §4.4 agree ("it may still be a report"); `Drop` has no
  `Skipped` and is report-level only, so union membership and `Drop::Cap` counting are
  unambiguous.
- **I-F6 — resolved; MINOR nit.** 4a/4b and 5a/5b-i/5b-ii/5c with `MB.23.4.1`/`MB.23.5.1`
  pre-authorized. The escape hatch names only 5c and 4b, so an over-budget 5b-i or 4a relies
  on the epic's general child pre-authorization; small, but worth one line when the plan
  paths are edited.
- **I-F7 — resolved.** `crates/blastit-php-sapi/src/{lib,request,abi,loader}.rs` is listed
  and the sink is resolved "through the **shim handle** … never `RTLD_DEFAULT`", matching
  `RTLD_LOCAL`.
- **I-F8 — resolved.** `sync_marks` is on `Reload`; `Policy` has `pending_dirs`/
  `stop_marking`; `Reload::boot`'s `poll` is "a `F_DUPFD_CLOEXEC` clone of `watcher.fd()`,
  made by the supervisor before the box goes in".
- **I-F9 — resolved.** The dup producer is fixed, and §8 lists the stage-1 tokens, the
  dbproxy token and the `--stage=1` + `--dev-root` gate; only `--no-reload` is user-facing.
- **MINOR residual:** §8 still says "`0x45` at the five receiver sites" while §4.9 lists
  seven — a count to fix in the plan-path edit, not a contract.

VERDICT: PASS
