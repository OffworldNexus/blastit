# MB.40.3 — deduplicate forks & seams (partial slice)

**Status: `needs_split`.** Five of the node's seven targets are done and green;
four (items 1, 4c, 5, 7) are each a vertical slice of their own, and two of them
cannot be finished without editing protected critical tests, which this brief
forbids. The reasons and the exact next step for each are in §Follow-ups.

## 2. `escape_control` fork deleted (`blastit-env`)

**Change** `crates/blastit-env/src/error.rs` no longer has its own copy of the
control-character escaper: it imports `blastit_manifest::escape_control`, the
function §18 already names as the one renderer for a quoted value. The local
`std::fmt::Write` import went with it.

**Proof** `cargo test -p blastit-env --locked` → `50 passed` (lib), including
`invalid_name_errors_escape_control_characters` and
`quoted_escapes_control_characters_and_cuts_long_values`, untouched.

## 3. Python no longer re-derives the SQL literal rules (`fold.py` ↔ `_blastit`)

**Change**
* `crates/blastit-py/src/lib.rs` exposes two pure functions, `has_quoted_literal`
  and `redact_literals`, delegating to `blastit_sqlpolicy`. Nothing is
  re-implemented: the differential is gone because there is one implementation.
* `python/blastit/fold.py`'s `_has_quoted_literal` and `_redact_literal` are now
  one-line calls into `blastit._blastit` (the `_KNOWN_STATIC_SETUP_SQL`
  exception lives in Rust, so the Python branch is gone). They keep their names
  and signatures because the **protected** critical suite
  `python/tests/critical/test_fold_literals_and_atomic.py` calls them directly;
  keeping them as delegates satisfies the node's goal (one rule, in Rust)
  without touching a protected test.
* `_unsafe_functions` is deleted; its one call site reads the `analyze` report
  inline with the shared `_text_list` helper. No test referenced it.

**Proof**
```
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_fold.py -q
102 passed in 1.50s
$ uv run --no-project python -m unittest tests.critical.test_fold_literals_and_atomic
Ran 22 tests … OK      # protected suite, unmodified
```

## 4a/4b/4d. Single-impl mock traits and the cross-request static removed

**4a `Acceptor` (`crates/blastit-http/src/server.rs`)** — the trait is gone.
`TcpAcceptor` exposes an inherent `async fn accept`, `Server::run` and
`serve_with` take it concretely, and the in-file test that queued duplex halves
through a `FakeAcceptor` is replaced by `serve_with_serves_a_live_loopback_connection`,
which binds `127.0.0.1:0`, connects a real `TcpStream` client, and drives the
same request/response frames over a real `http.chan` duplex. `blastit-http`'s
public exports and module docs drop `Acceptor`.

**4b `Editor` (`crates/blastit-manifest/src/edit.rs`)** — the crate-private
trait is gone; `BlastfileEditor::apply` is an inherent method on the one
concrete editor, and `blastfile/mod.rs` stops importing the trait.

**4d `static PORTS` (`crates/blastit-view-python/src/environ.rs`)** — the
process-wide `Mutex<Vec<(u16, Py<PyString>)>>` cache, its `PORT_CACHE` bound and
the `blastit_view::sync::lock` import are deleted. `port_str` formats a port into
a five-byte stack buffer, so the request path carries no lock and nothing a peer
chose (`REMOTE_PORT`) can make the process retain memory.

**Proof**
```
$ cargo test -p blastit-http --locked
test result: ok. 80 passed …         # lib, incl. the live-loopback test
# + 13/10/5/10/11/7/24/28/2 passed in the integration suites
$ cargo test -p blastit-manifest --locked
test result: ok. 141 passed …        # lib, + the 8 integration suites
$ cargo test -p blastit-view-python --locked
test result: ok. 59 passed; 1 ignored …
```

## 6. One `ProxySpec` builder; the two memory readings documented as layers

**Change**
* New `crates/blastit/src/dbproxy_wiring.rs` owns
  `proxy_specs(&Prepared, max_connections: impl Fn(&str) -> u32)`. The three
  copies (`stage0.rs` dev fork, `stage0.rs` production fork, `check.rs`) become
  one call each — `|name| boot.config.max_connections_for(name)`,
  `|name| config.max_connections_for(name)`, `|_| 4` — so a field added to
  `ProxySpec` is added once. Behaviour is byte-identical (same field mapping,
  same per-proxy cap).
* `blastit_sys::mem::Sample::used` now states it is the **raw** figure and names
  `blastit`'s `mem_usage` as the reclaimable-aware reading built on it;
  `mem_usage`'s module doc and `sample` doc state the layering (one
  `mem::Reader::sample`, then `memory.stat`). There was no second parser to
  remove — `mem_usage::sample` already called `mem::Reader` — so the
  reconciliation is that the division is now written where each half is read.

**Proof** `cargo test -p blastit --locked --lib` → `253 passed`, including the
`mem_usage` fixture tests and the `check`/`stage0` unit suites.

## Evidence — the two acceptance commands

```
$ make fast-check
…
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
67 files already formatted
uv run --no-project tools/plan.py validate
plan OK
```
(guard, critical-guard, rs-fmt, rs-lint and the full `rs-test` workspace run —
unit, integration, critical and doc tests — all green to that point.)

```
$ make rs-no-python
…
default build: 0 pyo3 dependencies (no libpython linked)
$ make rs-no-default
no-default build: 0 blastit-view-php dependencies
no-default build: 0 blastit-view-static dependencies
no-default build: 0 pyo3 dependencies
test result: ok. 2 passed …          # static_feature_off
```

## Tests added / changed

* `blastit-http::server::tests::serve_with_serves_a_live_loopback_connection`
  (replaces the fake-acceptor unit test: real listener, real client socket).
* `blastit-http::server::tests::serve_with_reports_an_accept_failure` (below).
* `blastit-py`: `has_quoted_literal` / `redact_literals` pyfunctions (no new
  Python test; covered by the 102 fold tests and the 22 protected literal tests).

No protected critical test was modified, deleted or skipped.

## Review round — three MINORs

* **`python/blastit/logs.py`** — `sql_regions` and `SQL_CODE` (with both `__all__`
  entries) are deleted now that `fold.py` asks `_blastit`; the live helpers
  (`_construct_at`, `_skip_*`, `_dollar_quote_end`, `_code_token_end`) stay,
  because `parseable_sql` walks with them. `ruff check` / `ruff format --check`
  clean, and `pytest python/tests/*.py` → `290 passed, 69 skipped, 19 subtests`.
* **`crates/blastit-http/src/server.rs`** —
  `serve_with_reports_an_accept_failure` pins `Err(HttpError::Accept(_))` again:
  it binds a real `TcpListener`, shuts it down (`SHUT_RDWR`, so the next `accept`
  fails `EINVAL`), and asserts the loop reports it. `rustix` is a new
  `[dev-dependencies]` entry (`std` has no listener `shutdown`, and the crate is
  `#![forbid(unsafe_code)]`); `time` joins its features because rustix 1.1.5's
  linux_raw `net` backend references its `timespec` module. `Cargo.lock` gains
  only `rustix 1.1.5` under `blastit-http`. No fake acceptor.
* **`crates/blastit-view-python/src/environ.rs:35-38`** — the duplicated doc
  fragment is collapsed to one sentence: "…the scheme names, `HTTP/1.1`) and a
  port is formatted into a five-byte stack buffer, so no intermediate `String` is
  minted for it." (`PyString::new` still mints the `PyString`; no zero-alloc
  claim.) ≤100 columns.

## Follow-ups

1. **Item 1 — `ObsVerdict` → `blastit_sqlpolicy::LearnVerdict` (own slice).**
   Two things remain beyond a mechanical rename: (a) `LearnVerdict`/`RefuseReason`
   carry no `serde`, while `WireSqlObs.verdict` is the JSON contract
   (`{"kind":"query"|"refused",…}`) read by `check`/`audit` and Python, so the
   slice must add a `#[serde(with = …)]` adapter in `blastit-dbproxy` (plus a
   `RefuseReason::Reported(String)`-style variant so a refused wire record
   round-trips its one-line reason exactly); (b) consumers `dev_learn.rs:373`,
   `audit_v1.rs:745,1531,1557` and `check.rs:1247` destructure `ObsVerdict`, and
   the **protected** `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs:782-789`
   calls `verdict.reason().contains("literal")`, which becomes
   `Option<&RefuseReason>` — a one-line, assertion-preserving edit that needs the
   node to authorize touching that file.
2. **Item 4c — `RequestIo` (own slice).** It is the public seam of
   `PhpEmbedRuntime::serve` and six in-file cases use scripted `FakeIo`s. The
   slice should make `ChanIo` the concrete request transport and rewrite those
   cases onto real `UnixStream::pair()` + `view.chan` frames, exactly as `4a`
   did for the acceptor.
3. **Item 5 — one safe ELF reader (own slice).** `blastit/src/host.rs` reads
   `SHT_DYNAMIC`/`.dynstr` (capabilities/RUNPATH); `blastit-view-php/src/discovery.rs`
   reads `.dynsym` (libphp symbols + `php_version`). The shared part is the
   header/section-table/bounds-checked-read layer (~200 LOC) — a new
   `blastit-sys/src/elf.rs` with each crate keeping its own domain walk, verified
   against both suites' capability and symbol fixtures.
4. **Item 7 — delete `crates/blastit/tests/common/mock_pg.rs` (own slice).**
   Its only `mod` consumer is the **protected**
   `crates/blastit/tests/critical_dev_and_fail_closed.rs` (~40 call sites), and a
   second inline copy lives in the **protected**
   `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs`. Those suites
   drive a real `blastit dev` child whose dbproxy dials a TCP upstream, so the
   fixture is genuinely an upstream PostgreSQL, not a stand-in for blastit code.
   Deleting it means authorizing edits to both protected files and replacing the
   fixture with either a real ephemeral PostgreSQL or a named, non-`Mock*`
   upstream fixture shared by the two suites.
