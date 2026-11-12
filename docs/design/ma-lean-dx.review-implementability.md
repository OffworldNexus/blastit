# Goldfish review — `implementability` (pass 2)

**Lens:** cheap-model buildability — crate seams, exact symbols, dependency ownership/licences, leaf sizing (≤ 600 lines), and whether a fresh implementer can build each task without inventing a data model.
**Documents:** `docs/design/ma-lean-dx.md` (`In review`, node `MA.5`) + ADR-0001 … ADR-0006.
**Method:** read the design/ADRs, then cross-checked every cited crate, type, signature, `Cargo.toml`/`Cargo.lock` entry, `Makefile` target and leaf `paths` against the tree at `develop`. Pass-1 findings are **not** re-raised; the five `I-B*`, eleven `I-M*` and nine `I-m*` resolutions in §9.1 were checked and are genuinely closed. What follows is what the revised text still leaves unbuildable.

| Severity | Count |
|---|---|
| `BLOCKER` | 5 |
| `MAJOR` | 10 |
| `MINOR` | 8 |

---

## 0. Cross-check ledger (verified, so the findings are trustworthy)

**Correct and unchanged as the design assumes:** `RouteInfo::new(name, prefork, uid)` (`scheduler/src/route.rs:27`); `SchedulerConfig` with `Default` and 22 fields (`scheduler/src/config.rs:207-297`); `Manifest::validate(&self, base: u32)` (`manifest/src/model.rs:473`); `ManifestWarning::{UnattachedGroup,UngrantedProxy,UnrestrictedRoute}` (`manifest/src/error.rs:350`); `Route::new` is `pub(crate)` (`model.rs:312`); `ListenerSpec` already carries `route/proxy/uid/queries/transaction_control/unrestricted/tables` (`dbproxy/src/config.rs:158-183`); `DbproxyQuery::authorize` and `allowlist.check` (`dbproxy/src/hooks.rs:200,226`); `credentials_match` (`dbproxy/src/state.rs:378`); `SeccompRole::View { egress }` already enforces exactly the `egress = false/true` split (`sys/src/seccomp.rs:38-68`); `caps::detect()` (`sys/src/caps.rs:517`); `make_private_dir`/`bind_unix_listener` (`sys/src/sock.rs:313,465`); `PRESETS`, `Preset::find`, `Warning::PresetFlagWins/ExpectsRoot/Rootless` (`blastit/src/config.rs:82,108,398`); `Frame::RespHead/RespBody/RespEnd` (`frame/src/frame.rs:442+`); `_blastit.main(argv, executable, dbproxy)` (`python/blastit/__main__.py:35`); `build_from_process` (`env/src/env.rs:319`); `pg_query 6.2.1` in `blastit-sqlpolicy`, `toml 1.1.8` in `blastit-manifest`, `url 2.5.8` + `percent-encoding 2.3.2` in `blastit-env`; licences all Apache-2.0/MIT.

**Symbols the revised design now names correctly but still mis-describes:** `Stream`/`Chan` (an `mpsc::UnboundedReceiver<Frame>`, not a `StreamEvent` receiver), `wait_for_credit` (private in `http/src/conn.rs:558`, not `wait_for_credit_or_response`), `RouteInfo` (no `in_flight`), `cli::parse` (a flat `Args` struct, no `Command` enum), `crates/blastit-manifest` (a `toml::Table` walk, no spans).

---

## BLOCKER

### B1. `evaluate_body` cannot honour R16, and no leaf may give it a budget

R16 promises: *"Passes request body bytes unchanged through bounded `1 MiB` flow-control windows without full-body scheduler buffering."* ADR-0006's trade-off repeats it. But the mandated seam is a whole-body function:

```rust
pub fn evaluate_body(method: &str, policy: Option<IngressPolicy>,
                     content_type: Option<&str>, raw: &[u8])
    -> Result<TranscodedBody, IngressReject>
```

`raw` is the entire body and `TranscodedBody.bytes: Vec<u8>` is a second copy of it. There is no variant that says "pass through, I made no decision about the bytes". The §4.2 A matrix even encodes `BodyCodec::Raw` as `Ok(raw, None)` — a full copy — annotated *"(streamed in scheduler without buffering)"*, which the type cannot do.

Failure mode: a cheap implementer calls `evaluate_body` after accumulating the body (the only thing the signature permits), so every `body = "raw"` upload — the Milestone-C excuse for multipart until CDR lands — is buffered twice in `scheduler` up to `BLASTIT_MAX_BODY_BYTES` (100 MiB default). §4.3's budget table has no row for that allocation, so the design's own resource contract is violated by the design's own API. The `Raw` row then becomes dead code the critical test never exercises with real bytes.

Fix: split the decision from the transcoding, e.g. `evaluate_head(method, policy, content_type) -> Result<BodyPlan, IngressReject>` with `BodyPlan::{PassThrough, Buffer { codec, max: u64 }}`, and let `transcode(codec, raw)` handle only `Json`/`Form`. Then R16 is testable (chunk 1 reaches the view before chunk 2) and §4.3 gains one honest row: "`Buffer` cap = `BLASTIT_MAX_BODY_BYTES`".

### B2. `blastit check` loads the **other** Blastit pytest plugin, which monkeypatches the app under test

`pyproject.toml` registers `[project.entry-points.pytest11] blastit = "blastit.pytest_plugin"`, so the 0.1.0 plugin is active in *every* pytest run of an installed wheel. Its activation switch is an option that projects set once in config (`pytest_plugin.py:671`): `_manifest_path` reads `--blastit-manifest` **or `config.getini("blastit_manifest")`** — and `pytest_configure` (`:2094`) then calls `_patch_handler()` (`:1373`) and `_patch_connect()` (`:1375`), i.e. it replaces `django.core.handlers.base.BaseHandler.get_response` and rewires `psycopg.connect` to a live server.

§4.2 G.1 step 5 spawns `[python_exe, "-m", "pytest", "-q", "-p", "blastit._check_pytest", *pytest_args]` and says nothing about the auto-loaded plugin. So for the exact project layout the docs recommend — `blastit_manifest = "blastit.toml"` in `pyproject.toml` — `blastit check` runs **both** plugins: `BlastitWsgiGate` wraps the WSGI callable while the old plugin patches Django's handler *inside* it and redirects connections away from the `DynamicCheck` listener. Nothing crashes: under `--json` the child's fd 1 is redirected, so the old plugin's report printing is swallowed, and the completeness numbers are simply wrong. That is the worst failure shape for a CI gate.

Fix (state it in §4.2 G.1 and test it): spawn with `-p no:blastit`, reject `--blastit-manifest`/`blastit_manifest` in forwarded `ARGS…` with exit `2`, and add a `test_check_contract.py` case that runs a project whose `pyproject.toml` sets `blastit_manifest` and asserts `_patch_handler` never ran (e.g. `BaseHandler.get_response` is `pytest_plugin`'s own object identity check).

### B3. The declared ingress dependency set cannot produce R15's `400`s

§4.1 pins the new crate to `serde`, `serde_json`, `thiserror`, `url`. R15 requires *"validates `&`-separated percent-encoding … any violation returns `400`"* and §4.2 A's matrix pins `b"a=%00"`, `b"a=%FF"`, `charset=ibm037` to `Err(400)`.

`url::form_urlencoded::parse` is infallible: it never returns an error, passes an invalid escape like `%ZZ` through as literal text, and converts invalid UTF-8 to U+FFFD instead of failing. `url::form_urlencoded::Serializer` is an encoder, not a validator. So the one dependency named for the job cannot reject `%FF`, and the honest implementation needs `percent_encoding::percent_decode_str(..).decode_utf8()?` — `percent-encoding 2.3.2` is already in `Cargo.lock` via `blastit-env`, but it is **not** in the ingress matrix and only Leaf 1 owns `crates/*/Cargo.toml`.

Failure mode: Leaf 2 (paths `crates/blastit-ingress/src/**`, `tests/**`) cannot add the dependency, writes `parse()`-based code, and the `%FF`/`%ZZ` rows fail or — worse — silently produce `a=%EF%BF%BD` and the implementer "fixes" the test to match. Fix: add `percent-encoding = "2.3.2"` to the Leaf 1 matrix and say in §4.2 A that validation is `percent_decode_str` + `std::str::from_utf8`, with `url` used only for the canonical serializer.

### B4. Canonical re-encode leaves the framing headers in place

R14/R15 rewrite only two headers: *"updating `Content-Length` and `Content-Type: application/json` on `ReqHead`"*. `ReqHead.headers` is a public `Vec<(Vec<u8>, Vec<u8>)>` (`frame/src/frame.rs:328`) that the view turns into `environ` wholesale — `partition_headers` (`view/src/environ.rs:202`) maps every non-`CONTENT_*` header to `HTTP_*`, so a chunked upload arrives at the app as:

```
CONTENT_LENGTH=17
HTTP_TRANSFER_ENCODING=chunked
```

with a **de-chunked** body. `CONTENT_ENCODING: gzip` (once Milestone C adds decompression, and today if a client sends it and the codec re-encodes) has the same shape. Any framework that trusts `Transfer-Encoding` (or a proxy in front of a later backend) now reads a body with framing it was told not to expect — the smuggling/differential class §5.3 claims to close. Duplicate `Content-Length` is also possible: the design says "updating", not "replacing every occurrence".

Fix: one normative sentence in §4.2 A/ADR-0006: the transcoder returns a **head edit** that removes `Transfer-Encoding`, `Content-Encoding`, `Expect` and every existing `Content-Length`, then appends exactly one of each header it owns; add a critical test asserting `HTTP_TRANSFER_ENCODING` is absent from `environ` for a chunked `POST` on a `json` route.

### B5. Leaf sizing is still not credible, with concrete counts this time

§8 promises *"~180–500 lines each"* and the lens contract is ≤ 600. Measured against the tree:

- **Leaf 7 `supervisor-dev-mode-and-presets` "~500 lines"** must add subcommands to a flat parser, CWD discovery, `app`/`preload` resolution + empty-var precedence, `BLASTIT_SECCOMP=0` preset abort, per-command `BLASTIT_LOG_FORMAT` defaults, `build`/`port` lifecycle fields, `ExecutionMode::Dev` (ephemeral `0700` roots, per-route `views/<idx>` replacing the hard-coded `/run/blastit/views/{uid}` at `config.rs:1789`, stage-0 secret swap, unprivileged seccomp, no `RLIMIT_NPROC`, banner, `BLASTIT_DEV_EXPOSE`), `dev --learn`, **and** two multi-process critical suites with a `MockPgServer`. The files it names are 3816 (`config.rs`) + 1244 (`stage0.rs`) + 1259 (`stage1.rs`) + 827 (`supervise.rs`) + 493 (`cli.rs`) + 282 (`zygote.rs`) + 280 (`child.rs`) lines today. Realistic delta: **1500–2500 lines**. This is the same leaf pass 1 called B5 and it was not actually split.
- **Leaf 8 `check-audit-and-learn-engine` "~440 lines"** is a 9-kind audit engine + deterministic ordering + human formatter + `plan_learn` + 12-row `infer_routes` + `check::run` (fd-1→fd-2 redirection, `dbproxy` lifecycle, `obs_pipe` reader, `http_obs.jsonl`) + three new `_blastit` exports + two critical suites. The sibling `crates/blastit-py/src/audit.rs` is **3473 lines** for a strictly smaller report. Realistic delta: **1200–1800 lines**.
- **Leaf 3 "~480 lines"** carries `app`/`preload`, the ordering guard, `LiteralQuery`/`RedactedQueryPlaceholder`, two warnings, a comment-preserving TOML editor with re-parse verification + `fsync` + atomic rename, **and** `redact_literals`/`has_quoted_literal`/`check_learnable` over 8 literal forms plus `LearnVerdict`/`RefuseReason`/two const arrays in a 27k-line sqlpolicy crate. The editor alone is 300–500 lines with its tests.

Fix: split along the seams the code already has — `L7a` CLI/config/precedence; `L7b` `ExecutionMode::Dev` stage-0/zygote/seccomp/runtime dirs; `L7c` `dev_learn`; `L8a` `audit_v1::evaluate` + formatter; `L8b` `plan_learn` + `infer_routes`; `L8c` `check::run` + `_blastit` exports; `L3a` manifest schema + ordering; `L3b` sqlpolicy literals; `L3c` `edit.rs` — each with its own scoped `cargo test -p … --test …` acceptance (§7.2 already provides the suite names, so this costs no new tests).

---

## MAJOR

### M1. JSON canonicalisation silently rewrites numbers — the differential §5 says it removes

§4.1 states *(no `arbitrary_precision` feature!)* and R14 mandates re-serialisation through `serde_json` numbers. Without `arbitrary_precision`, `serde_json` cannot hold an integer outside `i64`/`u64`: `{"id": 18446744073709551616}` parses to `f64` and re-encodes as `1.8446744073709552e19`. A body Blastit "validated and re-encoded" therefore reaches Django with a *different* value than the client sent, which is precisely the ingress/app parser disagreement the design claims to eliminate, and it breaks honest payloads (bigint IDs, `NUMERIC` strings, Kafka-style longs). Fix: state the policy — reject any numeric lexeme that does not round-trip through `i64`/`u64`/finite `f64` with `400 InvalidPayload`, or preserve lexemes in the visitor — and add both cases to `critical_ingress.rs`.

### M2. `as_learn_fallback` names state that does not exist

§4.2 E: *"`RouteInfo::as_learn_fallback(mut self) -> Self` (`is_learn_fallback: bool`, enforcing `in_flight == 0` single-concurrency admission on that route …)"*. `RouteInfo` is `{name, prefork, uid}` (`route.rs:13`), `#[derive(Debug, Clone, PartialEq, Eq)]`, cloned in `routes_from_manifest` and stored by value in the scheduler (`scheduler.rs:522`). There is no `in_flight` anywhere; admission is a global `self.slots >= self.cfg.max_workers` (`scheduler.rs:1218`). A cheap implementer will add a `u32` to a `Clone + PartialEq` struct and either fail to mutate it through `&RouteInfo` or make every route's equality depend on a counter. Fix: name the owner (`Scheduler::learn_busy: bool` or `Arc<Semaphore>` per learn route), the second-request behaviour (queue behind the existing `queue_timeout`? `503`?), and keep `RouteInfo` a pure value.

### M3. `PUSH`/`POP` are a synchronous round-trip with no timeout, no error policy, no target encoding

§4.2 E/F require the scheduler to send `PUSH __unrouted__:<method>:<path>\n` and wait for `OK\n` before `ReqHead`, over `learn_ctl_path`; `check` uses `check_ctl_fd`/`ctl.sock`. Unstated: (a) the read timeout — a wedged `dbproxy` blocks the scheduler task forever; (b) what a `ERR …`, EOF, or write failure does to the in-flight request (500? proceed untagged?); (c) the 512-byte line cap vs a long `PATH_INFO` (truncate silently? `ERR`?); (d) a target containing `\n`, spaces or `:`, and a route legitimately named `__unrouted__:x`; (e) why `learn_ctl_path` (a `PathBuf` in `SchedulerConfig`) and `check_ctl_fd` (an inherited fd) are two mechanisms for one protocol. Fix: one subsection with the deadline, the failure mapping, UTF-8/length rules on both ends, and one shared name.

### M4. `RoutePolicySpec` duplicates `ListenerSpec`, and the listener model does not fit `blastit check`

`RoutePolicySpec { route, proxy, queries, transaction_control, unrestricted, tables }` is a byte-for-byte subset of `ListenerSpec` minus `fd`/`uid`. The design now has two authorities for the same grants (`listener.allowlist` in `Enforce`, `route_policies[(target, proxy)]` in `DynamicCheck`) with no rule for which one wins when a manifest changes between them, which is exactly the drift `edit.rs` exists to prevent. It is also needed only because `check` binds **one socket per proxy** (§4.2 G.1 step 3) while the runtime model is one listener per `(route, proxy)` and `DbproxyConfig::validate` rejects a listener with an empty `route` (`config.rs:79-84`). Unstated: what `ListenerSpec.route` is for a shared check listener (`"__learn__"` is named only for `Learn`), whether `validate` must tolerate it, and what `PUSH nosuchroute` returns. Fix: either delete `RoutePolicySpec` and key `HashMap<(route, proxy), Allowlist>` built from `Manifest::effective_grants`, or state that `ListenerSpec` is the runtime authority and `RoutePolicySpec` is the check-only authority — plus the `validate` exception.

### M5. Two completeness engines, still no shared helpers

`crates/blastit-py/src/audit.rs` (3473 lines) already implements manifest-vs-observations diffing, covering-table logic and `unexercised`/`unrouted`, calling `RoutingTable`, `Manifest::effective_grants` and `Allowlist::check`. `audit_v1::evaluate` re-implements the same predicates for 9 kinds on a different input schema. Pass 1's "keep both" decision is fine, but the design never says what `audit_v1` **reuses** — and `dead_grant`'s "skip inline queries covered by `tables` or an attached group" (R34) is precisely `audit.rs`'s `own_queries`/`collect_queries`/`_covering` predicate. A cheap implementer will write a third copy. Fix: list the three calls `audit_v1` must reuse (`RoutingTable::route`, `Manifest::effective_grants`, `Allowlist::check`) and where the "covered" predicate lives (one function in `blastit-manifest`, called by both engines), plus one test asserting both engines agree on a 2-route fixture.

### M6. `Lifecycle` cannot scope `build` to `Boot` and `port` to `Ready`

§4.2 D adds `with_build`/`with_port` to `Lifecycle`, but `EventKind::{Boot,Ready,Shutdown}` all carry the *same* `Lifecycle` (`events/src/lib.rs:1281-1285`), and both encoders emit every set field (`json.rs:220`, `pretty.rs:161`). So `port` will appear on `boot` and `shutdown`, `build` on `ready` and `shutdown`, and `decode_json` will accept them there — while R23 and `docs/reference/log-events.md` say `build` is a `preload` `boot` field and `port` a `ready` field. Fix: either state "both live on `Lifecycle` and therefore on all three lifecycle events, docs updated accordingly" or give `Boot`/`Ready` their own structs; then name which encoder/decoder tests change (`json.rs:1595`, `pretty.rs:462`).

### M7. R8's "ordering" guard is not observable from a `toml::Table`

`crate::parse` (`manifest/src/parse.rs:25-53`) parses `source.parse::<toml::Table>()` and the parser's own doc says *"the order of the sections in the file does not matter"*. In TOML a key written after `[proxy.db]` **is** inside `proxy.db`: `version`, `app`, `preload` and `groups` written there are structurally nested keys, not top-level keys with a position. So `<key> after a [table] is not top-level` can only be produced by a name-based check ("this reserved key appeared inside table X"), never by ordering; and "`groups` must appear above the first `[table]`" is a tautology. The doc (`docs/reference/manifest.md:31`) does promise that exact message, so the message must stay — but the design must define the check as reserved-key-in-table, or an implementer will reach for `toml_edit`/`Spanned`, which **no leaf may add**: only Leaf 1 owns `Cargo.toml` and its matrix has no TOML editor. Also state the `TO_LEVEL_KEYS` delta (`parse.rs:20` adds `app`, `preload`) and that `[route.ingress]` joins `ROUTE_KEYS` as a *table*, not a scalar.

### M8. `HttpObsLine.sample` buffers up to 100 MiB per request inside Python

§4.2 G.2 gives `HttpObsLine.sample: Option<BodySample>` *"computed via `_blastit.probe_ingress`"* and §4.2 G.3 exports `probe_ingress(content_type, raw)`. On `body = "raw"` (multipart until Milestone C) or an undeclared route, `BlastitWsgiGate` must read the entire `wsgi.input` to hand `raw` to Rust — up to `BLASTIT_MAX_BODY_BYTES` (100 MiB) per request, retained in Python while the app also wants to read it. §4.3 has no row for check-mode body retention. Fix: specify a sample cap (e.g. first 64 KiB, with `truncated: bool`) and what `valid_json`/`valid_form` mean for a truncated sample (`None`, so `missing_ingress` still fires but `suggest_fix` falls back to `([Raw], true)`), plus the `wsgi.input` replay contract (the gate must hand the app a stream over the bytes it consumed).

### M9. The early-abort seam still loses or duplicates the response head

§4.2 E: *"`Stream` inspects incoming `StreamEvent`s on its `mpsc::Receiver` … checked via non-blocking `try_peek_response()` before each client socket `read()`"*. Reality: `Stream.incoming` is an `mpsc::UnboundedReceiver<Frame>` (`http/src/chan.rs:297-302`), `Stream::recv()` is the only consumer, and `relay()` also reads that same receiver (`conn.rs:596`). There is no push-back: a `RespHead` consumed by a peek is gone from `relay`, and a `try_recv`-based peek that drops non-head frames loses `Window` credit. Fix: state that `Stream` gains `pending: Option<Frame>` (peek = `try_recv` into `pending`, `recv` drains `pending` first), that the credit wait stays on the real `wait_for_credit`/`more_credit()` (`conn.rs:558`, `chan.rs:376`) — `wait_for_credit_or_response` does not exist — and that forcing close is appending `Connection: close` to `RespHead.headers` (`frame.rs:385-392` has no `close` flag) and pinning `response::Framing`.

### M10. The CLI seam has no `Command` today, and `check` needs the dbproxy path

`crates/blastit/src/cli.rs` is a flat `struct Args { stage, role, preset, seccomp, config_fd, … }` with a hand-rolled flag loop that rejects stray words (`cli.rs:221`), and `main(host: Host, argv: Vec<String>, py: Option<Python<'_>>) -> u8` (`cli.rs:88`). §4.2 G.1 jumps straight to `Command::Check { json, learn, dry_run, pytest_args }` without saying how subcommands coexist with `--stage=1`/`--role=dbproxy` (which must keep working for `execve`), how `-- ARGS…` stops parsing, or what `usage()` becomes (it is the only `--help` text and it lists presets). Step 3's *"spawns `blastit-dbproxy` … identical to `stage0`"* also never says it gets the binary from `Host::dbproxy()` (`host.rs:226`) — the wheel passes it as a `String` into `_blastit.main`. Fix: one paragraph: `Command::{Serve, Dev, Check}`, the exact argv grammar, and `check::run(host, …)` taking `Host`.

---

## MINOR

1. **`ManifestEdits.queries: BTreeMap<(String, String), Vec<String>>`** cannot round-trip through `serde_json` (map keys must be strings) — and ADR-0003 §3 says the editor is *"exposed to Python via `_blastit`"*, while Leaf 8's export list omits it. Either drop the `Serialize`/`Deserialize` derives (Rust-only seam) or use `Vec<ProposedGrant { route, proxy, queries }>`; say which.
2. **`CheckObsV1::schema_version: u32` `#[derive(Default)]` yields `0`**, but the contract is `1`. Require it at construction (`fn new()` or a `const SCHEMA_VERSION: u32 = 1` checked in `evaluate`) so a golden fixture cannot encode `0`.
3. **`WireSqlObs.kind: String`** while `SqlDenied::new(pid, proxy, kind: SqlKind, …)` already has a closed enum with `as_str()` (`events/src/lib.rs`). Use `SqlKind`; otherwise the obs pipe gains a second spelling for the same vocabulary.
4. **`BodySample.mime` is unbounded across `obs_fd`/`http_obs.jsonl`**, while `IngressReject::ContentTypeMismatch.observed` is capped at 64 B (good). Cap `BodySample.mime` the same way, in one shared helper.
5. **Enforcing crates with no owning leaf.** §5.2 names `crates/blastit-frame`, `blastit-sys`, `blastit-env` and `blastit-view` as the enforcers of R13/R19–R22, but no leaf lists `crates/blastit-frame/**`, `crates/blastit-sys/**`, `crates/blastit-env/**` or `crates/blastit-view/**` in its `paths`. Even if the answer is "no change needed", say so, or the first required `RespHead`/`make_private_dir` tweak becomes an out-of-lane edit.
6. **R42's `_blastit` staging recipe is under-specified.** `cargo build -p blastit-py` builds the cdylib **without** the `extension-module` feature (`crates/blastit-py/Cargo.toml` documents why), so it links `libpython` and needs `LD_LIBRARY_PATH`; the artefact is `lib_blastit.so` and must be renamed to `_blastit$(python -c 'import sysconfig;print(sysconfig.get_config_var("EXT_SUFFIX"))')`. Name those two facts and where the stub `pytest` package is staged so it shadows a real install.
7. **`docs/ci.md` is cited as the normative source for `schema_version: 1` and the 9 kinds**, but it contains no such section today (`grep schema_version docs/` finds only the ADRs and this design). `MA.5` owns `docs/design|decisions` only, so state who writes that section before `MA.2` starts, or §4.2 G.3 is the only spec.
8. **`parse_mime(content_type) -> (String, Option<String>)`** has no documented second element and silently drops every parameter except `charset` (a `boundary=` disappears, so `multipart/form-data; boundary=…` yields a bare essence). Document the tuple (`essence`, `Option<charset>`), the lowercasing, the parameter-dropping rule, and the `<= 64` cap at the same place as finding 4.

---

## What would make this buildable

1. **Separate "decide" from "transcode"** (B1) and give the buffering path one budget row in §4.3; add the framing-header scrub to the same contract (B4).
2. **Close the ingress dependency loop** (B3): `percent-encoding` belongs in Leaf 1's matrix, with `url` used only to serialise.
3. **Make `blastit check`'s pytest run hermetic** (B2): `-p no:blastit`, reject the ini option, and test the collision.
4. **Split Leaves 3, 7 and 8** along the existing crate/test seams (B5) — the suite names in §7.2 already make each split independently verifiable.
5. **Nail the four remaining data models** before decomposition: learn-admission state (M2), the ctl line protocol (M3), one grant authority (M4), and `Stream`'s pending frame (M9).
