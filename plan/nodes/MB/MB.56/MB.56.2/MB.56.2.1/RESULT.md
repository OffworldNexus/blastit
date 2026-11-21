# MB.56.2.1 — delete the non-§7 tests in the heavy crates

## What changed

Every test that does not protect a `TESTING_PLAN.md` §7 surface is gone from
`blastit-scheduler`, `blastit-dbproxy`, `blastit-view`, `blastit-view-python`,
`blastit-view-static`, `blastit-view-php` and `crates/blastit/src/**`. Nothing
was `#[ignore]`d and no production behaviour changed: the only `src` edits
besides deleting test modules are doc sentences that named them, one
`#[cfg(any(not(feature = "python"), test))]` narrowed to
`#[cfg(not(feature = "python"))]` (its `test` arm existed only for the deleted
`stage0` unit suite), and one test-only `pub(super)` gate helper (`php_boot`)
whose only callers were that suite. `pub` items that lost their last user keep
their visibility; dead-code pruning is `MB.56.5`.

`crates/blastit-scheduler/tests/static_feature_off.rs` and the shared
`tests/common` harness it drives were **kept** (restored from the base): the
`Makefile`'s `rs-no-default` leg runs
`cargo test -p blastit-scheduler --no-default-features --test static_feature_off`,
and it is the only executable proof of §6's "a `static`-less build refuses a
mount at boot" (deny row + accepted twin) — no E2E suite can re-prove it because
it needs `--no-default-features`.

Test LOC removed (git `--stat`, per importable unit):

| Unit | Removed |
|---|---|
| `blastit-scheduler` | 10 481 (`tests/**`; the §6 `static_feature_off` suite and its `common` harness stay) + 1 367 (inline) |
| `blastit-dbproxy` | 3 296 (`tests/**`) + 2 661 (inline) |
| `blastit-view` | 928 (inline only) |
| `blastit-view-python` | 3 115 (`tests/**`) + 1 929 (inline) |
| `blastit-view-static` | 3 388 (`tests/**`) |
| `blastit-view-php` | 260 (`tests/**`) + 2 058 (inline) |
| `blastit/src/**` | 11 591 (25 inline modules + 4 test-only files) |
| **total** | **≈ 41 074** |

Two source-parity meta-tests in `crates/blastit/tests/` had their *subjects* in
this slice and could not compile once those files were gone; they were removed
with them (`stage1_install_sites.rs` scanned `src/stage1/unit_tests.rs`;
`common_harness_parity.rs` compared the two `dbproxy` `pg_upstream.rs` copies).
Both are source-grep guards the charter bans and are part of `MB.56.3`'s
deletion set. See `## Follow-ups`.

## Inventory — user-visible cases `MB.56.3` must re-prove E2E

Dense, one case per line, with the `TESTING_PLAN.md` story it belongs to. "§7"
lines were §7-shaped but lived in a crate that does not own that surface
(scheduler HTTP framing, dbproxy SQL grants); they are re-proved through §2/§6
E2E instead, and the §7 owner (`blastit-http`, `blastit-sqlpolicy`,
`blastit-manifest`, `blastit-lsp`) keeps its own focused suites in later slices.

### blastit-scheduler — adversarial reachability of a view (`§6` isolation, `§2`)

- one view's request never reaches a sibling view, and a sibling never answers
  another's stream (§6 per-view containment)
- frames for an unknown stream id are dropped, not delivered to a live request (§6)
- a route never gets a second process while its first is alive (§6)
- a reused stream id, or bytes that are not frames on `http.chan`, is fatal to
  the run rather than a half-served request (§6 fail-closed)
- a completed response is answered once; a duplicated `DONE` is ignored; a late
  `DONE` after a timeout still releases the slot (§6 draining)
- an unsolicited `SPAWNED` is retired; a body for an unknown stream is ignored (§6)
- a request body is forwarded frame by frame and counted; a response abort after
  the head aborts the response, before it becomes a `502` (§2 runtimes, §6)
- a view that dies mid-request is a `502` and the next request respawns it (§2
  runtime fault isolation)
- a view that dies before `READY` fails its waiters and backs off (§6 fail-closed)
- too many stuck request threads recycle the view instead of hanging (§6)
- `DRAIN` finishes the in-flight request then returns; it gives up after the
  request timeout; closing `http.chan` ends the run; a stray ctl message fails it (§6)
- lazy spawn then dispatch only after `READY`; prefork and eager modes spawn at
  startup; idle views retire and respawn on demand; a request cap retires a view (§6)
- view cap retires the LRU idle view and *waits* when nothing is idle; a view
  that is not ready, or whose route is waiting, is never the victim (§6)
- a cap of two serves three routes without LRU churn (§6)
- queue: the global worker budget is respected and the queue is FIFO; a full
  queue answers `503`; waiting past the limit answers `503` (§6 fair queuing)
- per-view cap queues even with budget to spare; a slot is only released by
  `DONE` (§6)
- a slow request gets `504` and keeps its slot until its own end (§6)
- a queued request cancelled by the client leaves the queue; a dispatched one
  keeps its slot; a cancel tells the view then releases (§6 draining)
- fair queuing: an expensive client queues behind a cheap one and a cheap client
  is favoured over an expensive one; a paid flow is remembered until the debt TTL
  and forgotten after it (§6 fair queuing, `TESTING_PLAN` §6 "slow views cannot
  starve fast ones")
- an under-reporting view is charged at least until its first frame and cannot
  buy its client priority; a report larger than observed time is clamped (§6)
- a forged `X-Forwarded-For` from an untrusted peer is ignored; behind a trusted
  proxy the forged leftmost entry never names the client; with `*` the leftmost
  entry is not the client; `CF-Connecting-IP` decides on App Platform (§6 trusted
  forwarded headers)
- `503`/`504` answers carry `Retry-After` (§6)
- memory: an idle view is retired above the high watermark, one per check, and a
  busy view never is; an unreadable probe is not a retirement; no watermark means
  no check (§6 sizing)
- the derived view cap replaces an unset maximum, bounds the prefork burst, and an
  explicit cap wins; without `MemTotal` the cap stays unset (§6)
- a spawn above the watermark queues instead of forking, and a view retired for
  memory is not respawned straight away (§6)
- measured cost per view replaces the configured estimate; a machine that
  measures nothing keeps it (§6)
- routing: `/_blastit/health` (and a custom path) is the scheduler's, `GET`/`HEAD`
  only; an unmatched path is its `404` (§2 static/SPA, §6)
- requests route by path, method and rule; `SCRIPT_NAME` is stripped and
  normalised before routing; the event path is the decoded path info (§6)
- control bytes and dot segments in a target are refused; percent decoding keeps
  the view and its target intact; a bad target or method is `400` (§6 hardening)
- the `request` event carries every field; keep-alive streams are independent;
  a silent view holds the request in flight (§5 diagnostics, §6)
- static rewrite (`§2` static / SPA): an exact file is served for every alias and
  never forwarded; a directory answers its `index` and both spellings are one file
- a SPA miss on a route answers `index` and never the near-miss file; a miss
  forwards only with an `app` and keeps method/body; no SPA → `404`, not a forward;
  a refusal is final even with a SPA fallback and an app armed; a lost fallback
  file is `404` and never the app (§2)
- the method gate is `405` with exactly `Allow: GET, HEAD`; a `GET`/`HEAD` that
  declares a body is `413`, and the method gate runs first (§2, §6)
- the status mapping is total over every outcome and every request (§6 fail-closed)
- a static block adds no matching rule and mounts are the files; a mount fallback
  is the only thing that answers a miss after the views (§2)
- a serve reaches the wire with the responder's own bytes and headers; `HEAD`
  answers the `GET` head and no body frame; `If-None-Match` answers `304` with no
  body; a zero-length file is an ordinary `200`; a static refusal is one scheduler
  answer with no store (§2)
- more than `max_workers` static streams complete in waves inside the slot bound;
  a short read aborts the response and frees the slot exactly once; a client that
  stops granting credit is aborted at request timeout (§2, §6)
- a forward hands the slot back and re-enters the view path; a serve outranks a
  codec error a forward stores (§2)
- a buffered forward that ends while queued is still answered (and keeps one
  queue entry); it still expires on queue timeout; it counts against the app-path
  bound (§6)
- a full static queue answers `503` with `Retry-After` (§6)
- a client that goes away mid-body releases its slot once; a cancel while a read
  is parked keeps the slot until the read returns (§6)
- the pump reads the documented chunk; a zero-byte read aborts at once (§2 static)
- flow control: a large upload streams with credit; a queued upload does not delay
  another request; a view exceeding its response credit is retired; a client
  exceeding its upload credit fails the stream; a view whose outbound queue
  overflows is retired; credit arriving before dispatch is not lost (§6, §2)
- mount precedence over the product parser: the longest mount prefix answers a
  file both mounts could serve; a mount miss reaches the view before the fallback;
  the declared fallback answers after the views and only for `GET`/`HEAD`; a
  static answer names the mount and an untouched path names nothing; both mounts
  are the files and in file order (§2)
- `Blastfile` parity at runtime: static precedence outcome table; `body json`
  accepts JSON and refuses a form body; `body form` the inverse; `body none`
  refuses any body; `body raw` is unacknowledged and streams; `raw unvalidated`
  boots with the same warning; `prefork` reaches the route the scheduler spawns;
  `egress any` and its warning arrive together; `entrypoint php` answers every
  view; `HEAD` is implied by `GET` through the matcher (§2, §5, §6)
- ingress enforcement: `raw` streams chunk one to the view before chunk two; a
  chunked `json` body exactly at the cap passes; a valid chunked `json` body
  reaches the view canonically with no stale framing headers; a body over the cap
  is `413` before `REQ_END`; a non-empty body on a none/ingress-less route is `413`
  (from the head when declared) (§2, §6, §7 HTTP framing via E2E)
- an ingress-less request is served only after `REQ_END`; a chunked ingress-less
  head cannot reach the view before it; a mount miss with a body before `REQ_END`
  is `413` without a view; a mount miss reaches the view only after an empty
  `REQ_END` (§2)
- an ingress-less head that never ends is a bounded `503` and holds one queue slot
  and no more; concurrent unrouted `--learn` requests queue behind one slot; a
  full learn fallback queue answers `503` instead of growing (§4 `--learn`, §6)
- `routes_from_manifest` carries the declared ingress (§5)
- charge/label: a bailed `5xx` pays its wall where the same frames returned pay
  the clamp; a deferred `5xx` is charged to its last byte, not its `DONE`; an
  out-of-set `DONE` outcome takes the view down; a scheduler-written `504` is
  charged before its request event and stays out of the bailout bill; a view that
  answers then retires is billed for its answer; a view the init reported gone is
  never given a second request (§6, §5 `1 request` accounting)
- pools: per-process cap one forks per request; a threaded route serves both
  requests from one process; LRU picks the coldest admitting worker; siblings
  never answer each other's requests; a queued request survives the worker it
  queued behind; a bailed worker is never given the request that queued behind it (§6)
- one fork at a time; a second fork waits for the first `READY`; `SPAWNED`
  consumes the slot; a colliding `SPAWNED` retires the held pid; growth counts a
  promise not a process; growth yields to a starved route; two cold routes each
  get their first process (§6)
- reload retires every idle worker of every route; a spawn sent before a reload
  is retired on arrival; a failed spawn backs off growth only; a refused spawn is
  one backoff; a PHP route never threads; a route never exceeds its process
  limit; a burst pool shrinks on idle timeout (§4 live reload, §6)
- ctl channel (`§6`): messages round-trip without descriptors; `SPAWNED` carries a
  real descriptor; a missing/unexpected descriptor or a non-ctl frame is a
  protocol error; an oversized message is refused; a closed peer is an error not
  a panic; a cancelled read keeps the bytes and the descriptor it had
- the `static` feature-off build refuses a manifest with a mount at boot (message
  names the fix, echoes no path) and the same boot without a mount still serves
  (§6 fail-closed) — **kept** as `tests/static_feature_off.rs`, the `rs-no-default`
  leg's executable proof
- head allocations (performance, not a user story): dropped — see `## Dropped`.

### blastit-dbproxy — per-view database rights (`§2`, `§6`, `§7.2` SQL grants via E2E)

- granted statements run over simple and extended protocols and the session is
  plain (no proxy residue) (§2 parameterized read/write)
- an ungranted statement is denied with SQLSTATE `42501` and logged normalised
  with `$?`, never its literals (§2, §6 zero credentials)
- a denied statement never reaches the upstream (§6)
- transaction control (`BEGIN`/`COMMIT`/rollback) is granted only where listed (§2)
- a multi-statement simple query is denied (§7 injection/multi-statement, §2)
- two routes on one proxy have separate allowlists; an unrestricted route runs
  anything and its neighbour still gets `42501` (§6 per-view grant isolation)
- Django-shaped statements (connection setup, session `TimeZone`, `SET NAMES`)
  match their grant; table/column grants run a Django-shaped join and refuse an
  ungranted column; table grants refuse a lock hidden behind a subquery (§2)
- a connection from another uid is refused before the upstream; a root listener
  without the opt-in is rejected (§6 identity isolation)
- the connection cap refuses with `53300` and releases its slot (§6)
- an unreachable upstream is refused without naming it (§6 no host leakage)
- a cancelled query is forwarded upstream; a cancel from another route or uid is
  refused (§6)
- the configuration round-trips and never prints its secrets in `Debug` (§6)
- the local leg refuses TLS `N` then serves plain startup; only documented startup
  parameters are accepted, and an unrestricted route still refuses forbidden ones (§6)
- every connection to a closed port is refused by dbproxy (not silently proxied);
  an upstream auth failure is dbproxy's own message (§6)
- a denied `COPY` is logged as a `COPY`; only `application_name` is forwarded from
  the client (§2, §6)
- `ready` is called once the listeners are adopted (§6 startup phases)
- learning: fixture SQL under an empty target stack produces zero obs lines while
  `SET TIME ZONE 'UTC'` and `select pg_catalog.version()` are recorded and replayed
  exactly once on the first route (§4 `--learn`, §5)
- an unrouted request carries its own target; `set_db` switches the database the
  next session requests (§4)
- `Enforce` still answers `42501` and `'s3cr3t'` appears in no byte of the
  `SqlDenied` event or the obs stream (§6 no secret leak)
- a malformed control line is answered and closed; an oversized one is refused
  without buffering; a stalled one times out instead of hanging (§4, §6)
- policy channel: a document is version one over the crate's member names; a wrong
  version, unknown member, wildcard/duplicate entry is refused whole; the length
  prefix is enforced before a byte is read; a truncated body is a framing error;
  an idle channel waits but a started document is bounded; a closed peer ends
  cleanly with a one-byte ack (§4 live reload of grants, §6)
- startup gate: only documented startup parameters are accepted and every other
  one is refused and named; the gate announces a session only after
  `ReadyForQuery`; it delivers dbproxy's own refusal and swallows upstream failure;
  incomplete frames are not a decision; only an invalid listener is a fatal accept
  error; adopted listener fds are checked before use (§6)
- hooks: a stranger is refused and logged; a root listener is refused without the
  opt-in; the cap refuses the extra session and releases its slot; a refused probe
  costs no slot; the hook uses the proxy's identity and never the client's (§6)
- transaction control granted only when the listener grants it; an enforced denial
  redacts every quoted literal; a learning listener allows what enforce refuses
  and logs nothing; an unrestricted listener allows any statement and logs
  nothing, without widening its neighbours; the function-call protocol is always
  denied; a bind of a statement the proxy never authorised is denied (§2, §6)
- a table grant allows a statement no `queries` list names (§2)
- per-route learning listeners attribute from the listener, not the stack (§4, §6)
- `sslmode`: default is `prefer`; an unknown one is rejected without echoing it;
  `disable` never uses TLS; `prefer` tries TLS unverified and ignores a CA (libpq
  parity); `require` verifies only with a CA; `verify-*` needs a CA; CA bundles are
  written privately and named after the proxy; `standard_conforming_strings` is
  forced (§6)
- config: round-trips through a pipe and a byte sink; `Debug` redacts passwords
  and CAs; the serialised document carries secrets because init must hand them
  over; a bad document never echoes a value from it; too-large/empty documents are
  refused; proxies/listeners are validated; a wildcard listener is learning-only;
  a root listener needs the opt-in; internal descriptors are learning-only; the
  policy channel is refused outside a `dev` boot; a policy member the document
  cannot carry is refused; route policies name a real route on a declared proxy
  once; unknown fields are rejected (§4, §6)
- state: only the listener's own uid is accepted; root is never accepted without
  the opt-in (a root CI container may opt in); a connection without credentials is
  refused; the connection pid comes from `SCM_CREDENTIALS`; the probe timeout is
  bounded; every listener has its own cap and cancel registry; a policy swap
  reaches the enforcing listener and keeps a held policy; the cancel cap is small
  and separate from the session cap (§6)
- learn: a target is the innermost push; the route stack and setup buffer are
  bounded; a repeated setup statement is recorded once; only a declared proxy can
  be selected; a policy document replaces the map whole and a held policy is
  unchanged; a refused document keeps the previous policy; an unknown route
  evaluates against an empty allowlist; an observation never carries a user
  literal; transaction control is recognised by the policy grammar; the protocol
  constant setup statement keeps its literal; the wire kind round-trips; a refused
  verdict carries the redacted text and reason; the verdict object is byte
  identical both ways (§4, §5, §6)
- `postgres` E2E extra: `PgUpstream` harness itself (the two-copy parity guard) —
  dropped, see `## Dropped`.

### blastit-view (shared frame plumbing)

- `path_of_target` cuts at the first `?`, percent decoding handles every case,
  `SCRIPT_NAME` is normalised once and stripped only at a segment boundary,
  control bytes are recognised (§6 hardening)
- the output line cap is the documented 2048 (§4 diagnostics)
- a forker is asked for the protocol and the context is verbatim (§6 identity)
- loaded-file batch codec: every field round-trips, an empty batch round-trips, a
  truncated/other-version/undefined-flag/count-over-cap/len-over-cap/byte-over-cap
  batch is refused, an empty/NUL/non-UTF-8 path is refused on both sides, a
  trailing byte is refused, the encoder refuses a batch over the count cap, chunks
  split at the path and byte caps, a sequence reports a gap and a resync instead of
  swallowing them, a first delta from an unknown source is a gap, a reporter sends
  a boot batch then one delta per seq, a boot larger than one datagram is one boot
  then deltas, a boot batch that does not start the stream is refused, a full peer
  buffer drops the batch instead of blocking, a failed send is dropped and arms a
  resync (§4 live reload of loaded files)
- wire writer: frames arrive in order and intact; a body is split into frames that
  fit the window; an empty body sends nothing and spends nothing; a body beyond the
  window stops at its credit; an oversized frame is a write error; a dead peer is
  an I/O error not a panic (§6, §7 framing via E2E)
- flow: a new flow starts with a full window in both directions; request-body
  credit is spent and granted back; response credit is spent in pieces; a
  non-blocking take says when to read instead; a closed window with credit still
  spends it; a taker blocks until credit arrives; closing the window wakes a
  blocked taker; a grant wakes every taker that fits (§6)

### blastit-view-python — WSGI runtime (`§2` Django/Flask, §4, §6)

- `wsgi.input`: body read in pieces across frames; line iteration; `readlines`
  hint; `read(0)`; a read blocks until the body arrives; read after end returns
  nothing; a cancelled request makes reads raise; `size=None` reads everything;
  `readlines(None|0)`; an absurd size is clamped (§2)
- requests run on their own OS threads: concurrent requests take different
  threads; a parked worker is reused; a sleeping request does not hold up another;
  concurrent responses do not interleave frames (§2 concurrency)
- serve: `READY` is first; `GET` serves head/body/end/done; the write callable
  sends the head immediately; `close` runs before `DONE` and also when the
  iterable raises; a failing app is a `500` before the head and an abort after it;
  a non-bytes chunk is an error; no `start_response` is a `500`; a bad status line
  is refused; `CANCEL` discards the rest and still sends `DONE`; `max_threads`
  refuses with `503`; a large response is split into fitting frames; an unread body
  stalls instead of aborting; a second `REQ_HEAD`/forbidden frame/non-frame bytes
  are protocol errors; EOF waits for request threads; an empty channel closes
  cleanly; a request that ends without reading its body still ends; missing stream
  is a protocol error; a second `start_response` without `exc_info` is an error and
  with `exc_info` replaces an unsent head; an `exc_info` tuple or exception
  instance is re-raised once the head is sent; a raising `close` marks the request
  errored but finishes it; an unservable `SCRIPT_NAME` is refused before `READY`; a
  channel read timeout is retried; a response reports its request cost; a view that
  fails after the head reports nothing and aborts (§2)
- body/flow: a three-MiB upload is read with `read`, in chunks, and iterated by
  line; a response larger than the window waits for credit; the I/O thread keeps
  serving while a request waits; a body never read stops at the window; body bytes
  beyond the window are a protocol error (§2, §6)
- `environ` (PEP 3333): the standard environment; a non-address peer keeps its
  name and no port; `PATH_INFO` is percent-decoded and read as latin-1; the query
  string is the raw bytes after the first `?`; no query is empty; headers become
  `HTTP_*` keys and repeats join; an underscore name and an empty name are dropped;
  content headers are not `HTTP_*`; `SCRIPT_NAME` is stripped on a boundary only;
  the WSGI keys are present; the whole environ is exactly as documented (§2, §6
  header smuggling via E2E)
- `serve` works with the GIL held by its caller (init's post-fork call site) (§2)
- immortalisation: the running interpreter is probed; a 32-bit, big-endian,
  free-threaded or tracing-refcounts build is refused; versions outside 3.12–3.14
  are refused; the version and setting decide the pass; the marker follows the
  version; a skipped pass walks nothing; an immortalised object stops counting
  references while later objects are unaffected; the walk reaches untracked
  objects; the 3.14 wrap is survived by the marker child (§2 supported Python)
- loaded-file tracker: the open hook does no I/O, no import and no path
  resolution; it installs once and reports a module and a template; `pyc` maps to
  `py`; namespace packages report their directory; stdlib/frozen/builtin report
  nothing; an `int` audit path does not crash the hook; an exception in the hook
  never aborts the app (§4 live reload)
- request state: the terminal frame is claimed exactly once; cancelling closes the
  output and the window; every request has its own window; the span measures wall
  and thread CPU time (§6 CPU-time accounting, `TESTING_PLAN` §4 logs)
- `WsgiConfig`: defaults fill in, fields override, a mount point is empty or
  `/`-prefixed and a bad one is refused with a reason (§5, §4)
- `wsgi.input` internals: `read_all` waits for the end; `read(n)` makes progress
  and waits for n or the end; `read(0)` never blocks; `readline` stops at newline
  or end and honours its size; an empty line is a line; sizes map from the Python
  signature; credit is sent in whole batches and the rest at a boundary; a dropped
  sender ends the body; `pump` returns what was queued and credits it back; `pump`
  stops on a cancel (§2, §6)
- `response`: a status line parses into code and reason and a bad one is rejected;
  a response stays quiet when the head was already claimed; one without
  `start_response` refuses to finish; storing a head after it was sent is refused (§2)
- `environ` helpers: the target splits into path and query; percent decoding
  handles every shape; latin-1 round-trips and rejects wider characters; latin-1
  borrows ASCII and owns the rest; header names become `HTTP_*`; content headers
  split out and repeats join once (also with dashed lowercase names); scheme names
  are the WSGI ones; the port comes from the peer address (§2)

### blastit-view-static — static confinement (`§2` standalone/SPA, `§6`)

- the 40-row attack matrix over both `openat2` and `Walk` backends: `..` and
  encoded `..`/`%2e%2e`, absolute paths, NUL, double-encoded separators, backslash,
  overlong segments, symlinks (in the root and out of it), dotfiles (and the
  `.well-known` exemption), missing files, directories without index, unreadable
  leaves, a root that is not a directory, a leaf swapped between probe and read,
  and a caller-supplied segment list cannot leave the root (§2 path traversal,
  §6)
- a `fallback` that is missing is `fallback_missing`; the file ceiling is
  inclusive at `static_max_bytes` and a file that grows past it between probe and
  read is hidden; a root declared through a symlink is followed at open and nowhere
  else (§2, §6)
- `segments` accepts every legal shape and refuses every malformed one; the
  production head refuses what `segments` never sees; `classify` is total; the
  `openat2` record is what was asked for; `same_inode` compares device and inode;
  the immutable-pattern compiler matches one way; the dotfile rule exempts exactly
  `.well-known`; `café.js` is the NFC bytes and nothing else; an unopenable root
  names the root it could not open (§2)
- response table: the header/cache/conditional/`Content-Type`/`HEAD`/`304` rows
  with sha256 bodies and exact `ETag`s over both backends; `ETag` is exact, stable
  and moves with the mtime; `If-None-Match` answers `304` and repeats the head; the
  cache header is keyed on the answering file (an exact file earns the year, its
  shared `index` stays `no-cache`); bare `immutable` caches every exact file and no
  index answer; a regex `immutable` entry is never read as a glob; one quoted
  pattern crosses `/` at any depth; a pattern list is an `or` in either order; a
  mount prefix is restored before the pattern is matched; the content-type table is
  closed; a zero-length file is `200` with length zero; an oversize file is `404`
  and `respond` never promises it; a response moves the one descriptor and never
  duplicates it; `HEAD`/`304` close the descriptor instead of leaking it; the
  ceiling admits the exact size and refuses either half over; statuses come from
  `decide`, not a second mapping; `http_date` inverts to IMF-fixdate (§2, §7 HTTP
  framing via E2E)
- entry-script confinement: `.php`/`.phtml`/`.phar` final segments are `404` before
  the method and body gates, in any case, over both backends; a source file is
  hidden at resolve with no read-open (asserted through the probe hook); an
  unreadable `.php` is `404`, not a read failure; a forged source URL path is never
  streamed; a source `index` never answers the directory or the SPA fallback; a
  normal asset is still served exactly and its `405`/`413` gates unchanged (§2,
  §6 source leak / `php2shell` adjacent)
- **seam for `MB.56.3` (keep it):** `blastit-view-static::StaticRoot::open_with`
  (`#[doc(hidden)] pub`) is how a caller outside the crate selects the backend, so
  every matrix row above is re-proved twice — once over `openat2` and once over the
  `Walk` fallback — from `crates/blastit/tests/`. It is not a test-only hook and
  must not be deleted; `StaticRoot::open` stays the crate default.

### blastit-view-php — PHP runtime (`§2` PHP 8.3–8.6, `§6`)

- two requests are answered through a real `view.chan` on a host with NTS `libphp`
  (the only E2E-shaped case; its harness was `--ignored`) (§2)
- a socketpair request is answered and closed in order; a large response waits for
  window frames instead of deadlocking; a cancelled request aborts but the worker
  serves the next one; a request without a stream is a protocol error; a stray body
  frame names only its kind and never leaks bytes; a configuration that could not
  serve is refused both ways; a non-library is a boot refusal and never a request
  fault; an output line is capped on a character boundary; the PHP environment
  never carries `HTTP_PROXY`; the mount point is stripped only at a segment
  boundary; a contained `exit()`/`die()`/fatal is its own `DONE` outcome; a body
  past the worker budget is refused; a bailed worker retires and `max_requests`
  still applies; a runtime that retires ends the loop after its `DONE`; a body
  before the head, a body past the flow window, and a header burst past the cap are
  protocol errors (§2 fault isolation, `php2shell` immunity by construction)
- boot bindings: a PHP slot gets one binding and a WSGI slot none; a swapped entry
  is refused naming the route, the entry and the fix; an entry rewritten in place
  is refused; a repin records the saved entry and refuses one that left the root; a
  boot pins a symlinked entry and a reload refuses to move the pin onto it; a
  replaced `libphp`/shim is refused by name; a missing record is refused by route;
  a pinned descriptor survives the child sequence (§6 tampered entry file,
  `TESTING_PLAN` §6 "tampered application entry files")
- discovery: a version is read from every distro spelling (`php-config8.4`,
  `php-config84`, `/usr/lib/php84/libphp.so`, `/usr/lib/libphp8.4.so`); a symlinked
  libphp/shim is recorded as its targets; a dangling symlink is refused with the
  package sentence; no `libphp` refuses only when a PHP route exists; a ZTS libphp is
  refused naming the tuple and shim; a non-ELF libphp is refused; the minor comes
  from the file name, the packed name, the Alpine directory or the object bytes; two
  disagreeing minors are refused naming both; a libphp without `php_version` is
  refused naming the symbol (presence is the `.dynsym` name, not the body shape);
  an unsupported minor is refused with the support sentence; the two-embed-package
  Alpine host serves from the newer library; `php_version` is decoded on x86_64 and
  aarch64; the shim is chosen by the tuple and a missing one names it; a boot with no
  workers cap passes any budget; a version-less refusal still names the tuple; a
  relative libphp is refused; the gate reads the ELF and never loads it (§2 PHP
  matrix, §6)
- deployment gate: refuses a pair or tree a route uid could replace; a missing entry
  or static root names the missing path; proceeds on an absent gate; a development
  boot does not apply the production rule (§6)
- app rule: a PHP manifest needs no `app`; a manifest with no executor still
  requires the callable; `entrypoint php` no view reaches needs no app; `raw`
  unacknowledged is refused and acknowledged is not; every other codec and no
  ingress at all pass; every unacknowledged route is named in route order; the
  serve-path proof is withheld from a refused manifest (§5, §6 fail-closed)
- report: the loaded-file report is a boot then one delta of sorted absolute paths;
  a compiled artefact reports the template it was built from; the source markers are
  read from the artefact's text; the compiled-to-source map is memoised per
  artefact and stops at its bound (§4 live reload)

### `crates/blastit/src/**` — CLI, dev, check, boot, host

- CLI: the dev grammar is exactly the documented one; the check grammar likewise;
  unknown arguments and unreadable values exit `2`; a stage-0 preset typo is a usage
  error naming the presets; `--preset` parses in both spellings and belongs to stage
  zero; the usage lists the presets, hides the re-exec grammar and offers
  `--version`/`help`; `help` names every command and refuses an unknown one; the
  dev-only port flag is answered with the two places it belongs (§3, §5 CLI craft)
- dev: the banner names a clickable address, every view and the `--learn` badge;
  static mounts share the columns and a non-ASCII mount prefix keeps them aligned;
  one route pluralises correctly; the learn summary pluralises both counts; a
  non-loopback host is named with the variable to set; the dev root is private and
  cleaned up; a symlinked root is refused; view names/patterns follow the documented
  rules; observations are parsed one per line; an unrouted request proposes a route;
  first declared view and matching proxy keep grants when an unrouted view exists;
  two proposed views do not borrow each other's statements; a scanner probe never
  donates its SQL to a proposed view (`.env`/`.git`/`..`); a learned query keeps the
  file's executor blocks; a manifest swapped for a FIFO cannot block the learn write
  (§4 all four stories)
- live reload (policy): a pre-fork change needs the cycle and a view change does
  not; `modify` is never a trigger; atomic save is one reload; debounce edges and
  the floor; two windows 400 ms apart respect the floor; a burst of twelve files is
  one reload; an unavailable errno maps to its sentence; a mid-run watcher error
  stops once and is never retried; overflow reloads every view once; skipped,
  missing and untracked paths do not fire; a framework cache write never triggers;
  path refusals are counted once; the union cap refuses the last path with one
  warning; `ENOSPC` stops marking and marks keep firing; the mark cap latches and
  reports once; `dirs_to_mark` never restats; registering a source again releases the
  previous end; a closed report end is revoked; a zero-length datagram is not a
  hangup; the skip/cache predicates are exact; a boot batch does not rearm but a
  resync does; a view report marks its parent; a seq gap rebuilds the source set (§4
  live reload)
- supervisor: a wakeup reads only its poll set's sources; a late ctl write to a gone
  scheduler is best effort while every other failure stays fatal; a scheduler that
  closes with data queued resets the channel; `EXITED` names the child's own route;
  retire refuses a sibling route's pid; a route cannot be forked past its ceiling;
  the adoption document round-trips; an abandoned cycle names the route and never
  asks for a kill; a cycle whose views never drain is abandoned without a signal;
  the policy document is the new manifest's pairs and nothing else, the exchange
  writes and reads one ack, a stray answer is reported, a refused document is one
  warning and a swap is silent, a store added or removed is one warning naming the
  restart, a late ack leaves the channel out of step and the next save warns (§4
  live reload, §6 draining)
- host/provenance: the binary re-executes itself for every role; a rootless binary
  plays dbproxy as the capability-free copy; a missing binary is refused by name; a
  capability-free binary another uid could replace is refused; every named program
  carrying capabilities is refused; a real capability on a shared name is refused; a
  relative symlink target is read in the right directory; a shared name that is init
  itself is refused; an unnameable capability is refused; a failed capability check
  is a refusal; a child may not point at a file with capabilities; init's own copy is
  not a refusal; a rootless layout is accepted; `same_file` follows symlinks;
  `has_file_caps` tells absence from ignorance; `protected` accepts root or the
  service uid and nothing writable; `facts_of` reads owner and walks directories;
  search paths are read for both tags and classes; a search entry is resolved only
  when boot can; a writable search path or tree is refused and a protected one is
  not; a capable object is refused (§6 fail-closed startup)
- visibility: a directory everyone can read is not a blocker; a leaf a view could not
  read names itself and its mode; an ancestor a view could not traverse is named, not
  the leaf; the first blocker wins; a program a child could not execute is named and
  explained; a group class that belongs to the uid counts; a directory another uid
  could write is refused; every uid that could write is asked; a writable directory
  above an entry is refused; a sticky directory a view uid owns is a way in and one
  above an entry is not; a probe that does not answer names its path and is killed; a
  probe leaves no child behind; a missing entry under a writable directory is refused;
  a writable zip on `sys.path` is refused; a private home refuses a uv-managed
  interpreter (§6 zero-trust containment)
- boot: the layout collision check knows every process it hands a uid to; a uid
  outside the layout is free; the stage-0/configuration boot event says the mode and
  preset; the detected mode becomes the boot field; a live child that printed is
  still logged; a silent live child does not block the drain; a dead child's output
  becomes `app.output` events; a dbproxy that never reports ready times out and one
  that exits before ready is reported; memory refusal takes its ceiling from config;
  an explicit empty grant still names the proxy for its route; the dev policy channel
  is one non-blocking socketpair; the runtime shape is decided by what the boot loads;
  a static-only boot is allowed; an `app` is refused naming the package; a view
  nothing reaches is refused by name; `--learn` needs the gate and says so; an `app`
  on a static tree keeps the rule; a PHP-only manifest is not refused by the
  capability gate; the shim name matches the ADR literal; a PHP manifest needs no
  `app`; a manifest with no executor still requires the callable; every
  unacknowledged `raw` route is named in route order and the serve proof withholds on
  refusal (§5, §6)
- adoption: a production boot drops each view to its own uid and filters it; a dev
  boot shares the uid and still filters; an explicit off switch installs no filter
  but keeps the identity; the adoption document refuses every shape it cannot honour;
  a stage-1 prefix past eight tokens is refused not truncated; the ingress types are
  importable and speak the manifest spelling; `learn` routing reserves slot zero and
  preserves declared matches; a watcher dying after preload keeps the boot line off (§6)
- config: durations/sizes/percentages/booleans accept the documented spellings; the
  immortalisation setting takes `auto`/booleans and is read from the environment; the
  boot seccomp flag uses the documented spellings; proxy variable names fold to one
  key; the JSON document round-trips; unknown members are refused; a setting the
  document cannot carry names the field; a non-UTF-8 `sys.path` entry survives and one
  outside latin-1 is refused; the working directory travels in the document; the
  privilege mode round-trips; a document without privilege members reads as nothing
  detected; a capability list that is not strings is refused; a wrong-shaped member
  keeps its sentence; privilege fields reach a lifecycle event; a missing field is
  named; the health path must be absolute; `app` must be `module:attribute`;
  `frame_max`, a zero view cap and a zero memory estimate are refused; the fair
  queuing defaults are the scheduler's; the default trusted proxies are the
  resolver's; fair queuing and client-IP settings are read from the environment; a
  bad trusted proxy or header name names its variable; the deprecated
  `trust_forwarded` is translated and warned about; a non-name fair queue entry is
  refused; the knobs are clamped and reported; warnings travel to stage 1; each preset
  supplies the documented defaults and no preset leaves them alone; an explicit
  variable overrides the preset; the flag names the preset and wins; an unknown preset
  is refused listing the known ones; the name is trimmed; the stage-1 document carries
  the preset; a preset whose assumption fails is named but never refused; every preset
  table is well formed; the document key set is the stage-0 contract; `libphp` paths
  must be absolute; the resolved PHP backend round-trips and an older document still
  reads (§3, §4 loopback/exposure, §5 presets, §6)
- check/infer: the root is `/`; a numeric path is a placeholder; catch-alls are found
  in both spellings; a taken name is suffixed; scanner probes are not learnable (§4,
  §5)
- `blastit check` exit table (`docs/reference/cli.md:78-80`,
  `docs/troubleshooting.md:49-51`, `docs/frameworks/python/django.md:101`): clean
  `0`, any finding `1`, could-not-start `2`, suite failed `3` — a failing assertion,
  a pytest usage error, a signalled child and pytest `5` (nothing collected) all map
  to `3`, with stdout empty under `--json` (§5)
- pytest-django handshake (`docs/frameworks/python/django.md:98-103`, §2/§5): the
  handshake carries the settings module, `manage.py` and `test_<database>`; the
  handshake file is owner-only, never reused, and removed however the run ends; a
  Django project with no `postgres` proxy and a store with no input variables are
  exit-`2` start failures naming the block/node (§2, §5)
- `check --learn` / `--dry-run` human line (`docs/local.md:84`,
  `docs/ci.md:8,118,123`): `learn: N routes, M queries added → /abs/Blastfile`, and
  `proposed (dry-run) → …` under `--dry-run` (never `added`, never a relative path),
  on stderr under `--json`; the JSON key is `routes`, never `route` (§4, §5)
- gate root discovery for an installed binary (`docs/reference/cli.md:6`,
  `docs/ci.md`, §3/§5): `blastit check` finds its embedded gate from the
  installed/repacked bundle and never from a cargo tree, prefers the project venv's
  interpreter, and prepends the caller's `PYTHONPATH` (§3, §5)
- logs: a phase line keeps a fixed column; a refusal is prose only on a coloured human
  terminal; colour is asked per stream and plain when the operator says so; an
  unreadable forwarded line is escaped; a non-event line is escaped in the dump; JSON
  lines end with one newline; the level filter drops quiet events; forwarded lines are
  restamped; a request line is forwarded byte for byte; a view output pipe inserts the
  route in key order and rewrites it; an unparseable events line is dropped with a
  warning; a quiet event is dropped whole; pretty lines carry a clock and fields;
  output lines split into `app.output` events; stderr is a warning; the line buffer
  keeps the rest and flushes an overlong line (§4 logs, §5 diagnostics)
- memory: reclaimable is the page cache that is not tmpfs plus reclaimable slab; a
  reading takes that cache out of cgroup usage; without it raw usage stands; without
  cgroups `MemAvailable` is already net of cache; a boot refusal names the setting and
  reading; the advisory is only for a boot above the default (§6)
- mem/unified/coverage/py/forking: coverage plumbing and the unified diff renderer
  (changed line, identical documents, far apart hunks, insertion at end, oversized
  document, missing final newline, empty file, CRLF, quotes/braces); the shim refusal
  document names the shim's sentence; a fork whose protocol disagrees with the image
  is refused and a build without the python feature refuses an interpreter-live fork
  (§3, §5 `fmt --check` diff)

## Dropped — no `TESTING_PLAN.md` home, and why

- `blastit-scheduler/tests/head_allocs.rs` — an allocation-count micro-benchmark of
  `begin_raw`; no user story measures allocations.
- `common_harness_parity` (the two-copy `pg_upstream.rs` guard) and
  `stage1_install_sites` (token/order scan of `src/stage1`) — source-parity
  meta-tests, forbidden by the charter; no user-visible behaviour.
- Three deleted **self-checks** — checks of a suite's own oracle, not of the product,
  so no user story owns them and `MB.56.3` must not recreate them:
  `the_suites_sha256_oracle_is_sha256` (`critical_static_assets.rs`) hashed a fixed
  string to prove the suite's own hash helper; the closed
  `the_documented_limits_and_flags_are_the_ones_asserted`
  (`critical_static_resolve.rs`) compared constants to their own documentation; and
  `the_two_halves_of_the_rule_still_hold` (`stage0/unit_tests.rs`) restated the app
  rule its siblings already assert.
- `unified.rs` diff-rendering cases kept above only for `fmt --check`'s shape; the
  byte-exact hunk mathematics (line ranges, oversized documents, CRLF) is a §7
  `Blastfile` formatting detail re-proved by `critical_*` fmt E2E, so the renderer
  itself needs no unit test.
- `coverage.rs` — build-script plumbing for `make coverage`; no user story.
- everything else is listed above with its story.

## Evidence

```
$ cargo fmt --all -- --check
FMT-OK

$ rg 'cfg\(test\)' crates/blastit-scheduler crates/blastit-dbproxy crates/blastit-view \
    crates/blastit-view-python crates/blastit-view-static crates/blastit-view-php crates/blastit/src
ACCEPTANCE-EMPTY   (no match)

$ cargo test -p blastit-scheduler --locked --no-default-features --test static_feature_off
running 2 tests
test a_manifest_with_a_mount_is_refused_before_any_channel_is_served ... ok
test the_same_boot_without_a_mount_is_not_refused ... ok
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.30s

$ make fast-check
exit=0 — 126 `test result: ok` lines, zero `warning:`/`error` lines
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

## Follow-ups

- `MB.56.3`: `crates/blastit/tests/stage1_install_sites.rs` and
  `crates/blastit/tests/common_harness_parity.rs` were removed here because their
  `include_str!` subjects (`src/stage1/unit_tests.rs`, the second
  `dbproxy/tests/common/pg_upstream.rs`) are deleted by this slice; both were
  source-parity guards and part of this node's deletion set. `critical_php_embed.rs`
  and the other E2E suites still compile and pass.
- `MB.56.5`: `dev_reload.rs` keeps `#[cfg(any(test, debug_assertions))]` test doubles
  (`TestClock`, `FakeWatcher`, `report_pair`, `send_batch`) because
  `crates/blastit/tests/critical_dev_reload.rs` (out of this node's scope) still
  drives them; prune them with that suite in `MB.56.3`/`MB.56.5`.
