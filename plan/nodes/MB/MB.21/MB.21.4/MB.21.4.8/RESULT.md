# MB.21.4.8 — result

**STATUS: blocked** — the mechanism is **hypothesis 3 (server-side)**, not the test client:
`blastit-scheduler` admits an `AllowEmptyOnly` request from its head and `blastit-view` starts the
WSGI app on `REQ_HEAD`, so a body byte's `413` races the app's `200`. Both files are outside
`paths`. The client defect the leaf describes is real and is now fixed and pinned, but it is **not**
what made the protected assertion read `200`; that flake is **not closed** and needs the follow-up in
§5.

- **BASE:** `c2283901212dd61b6172aa1af5f172f59e357995` (captured before the first edit).
- **Changed:** `crates/blastit/tests/critical_dev_and_fail_closed.rs`, `docs/knowledge/LESSONS.md`.

## 1. The mechanism, with its evidence

`POST /env` with `Content-Length: 5` on `view home regex '/.*'` (no `[route.ingress]`) must be `413`
(`R12`). The server sometimes answers it **`200`, served by the app**:

1. `crates/blastit-scheduler/src/scheduler.rs:1487` sets
   `request.body_ready = !matches!(plan, BodyPlan::Buffer { .. })`, so an `AllowEmptyOnly` request is
   *ready at the head*, and `:1507` calls `self.admit_ready(stream_id)` from `begin_raw` — the request
   is dispatched before `REQ_END` is seen and before any body byte can be refused.
2. `crates/blastit-view/src/lib.rs:474` (`Frame::ReqHead => self.start`) and `:501`/`:540`
   (`start` → `pool.assign`) hand the request to a worker at the **head**; `serve_one` (`:822`) runs
   the WSGI application without waiting for `REQ_END` (the body is fed lazily to `wsgi.input`).
   This app never reads `wsgi.input`, so it answers as soon as it is started.
3. The body byte then reaches `request_body` → `BodyPlan::AllowEmptyOnly` → `refuse_body(413)`
   (`scheduler.rs:1901`/`:1907`). If the view's `RESP_HEAD` got there first,
   `view_head` (`scheduler.rs:4154`) has already forwarded it (the request is not `complete` yet) and
   `blastit-http`'s `fill_body` (`crates/blastit-http/src/conn.rs:917`) takes it as the answer —
   `BodyOutcome::Early` → `relay`. The client is an ordinary HTTP client here: it reads a well-formed
   `HTTP/1.1 200 OK` with a declared body.

**Evidence A — the flake itself, with the suite unchanged apart from read-only instrumentation.**
40 full runs of the suite binary at 94 % host memory (`max_view_processes=1`) gave 2 failures, both
reading `200`:

```console
$ grep -n "probe :37115" run_30.log | head -4
[probe :37115] GET /env   read=Ok(705)  raw=HTTP/1.1 200 OK ... Content-Length: 614 ... {"pid": 1735265, …
[probe :37115] GET /sql   read=Ok(102)  raw=HTTP/1.1 200 OK ... Content-Length: 12 ... {"ok": true}
[probe :37115] POST /env  read=Ok(705)  raw=HTTP/1.1 200 OK ... Content-Length: 614 ... {"pid": 1735265, …
$ grep -n "POST /env" run_30.log
42:15:11:45.932 INFO  request     POST /env → home  413 unmatched  0.7ms
```

The client read a **complete** `200` (705 bytes, `Content-Length: 614`, EOF, nothing left over) whose
body is the app's `/env` JSON with the view's pid, while the scheduler logged `413 unmatched` — and in
the second failing run the scheduler logged `POST /env → home 200 0.7ms` (`"outcome":"ok"`), i.e. the
app really answered the POST. So the status the assertion saw is what the server sent; no client
buffer can produce a complete, framed 200 out of a 413.

**Evidence B — deterministic, without load.** Send the head and the 5 body bytes as two writes:

```console
$ uv run --no-project /tmp/opencode/mb2148/fix/split.py       # one `blastit dev`, BLASTIT_MAX_VIEW_PROCESSES=8
warm GET /env -> 413 (expect 413 for the broken one)
head, sleep 0.000s, body -> 413 (132 bytes)
head, sleep 0.000s, body -> 413 (132 bytes)
head, sleep 0.001s, body -> 200 (1066 bytes)
head, sleep 0.005s, body -> 200 (1066 bytes)
head, sleep 0.020s, body -> 200 (1066 bytes)
head, sleep 0.100s, body -> 200 (1066 bytes)
head, sleep 0.300s, body -> 200 (1066 bytes)
{"bytes_in":0,…,"event":"request","method":"POST","outcome":"ok","path":"/env","route":"home","status":200,…}
```

A **1 ms** gap between head and body is enough: the request is dispatched (`bytes_in: 0` — the body
never reached the scheduler) and the app answers `200`. In the suite the two go out in one `write_all`,
so the gap is normally ~0 and the `413` wins; under CPU contention the scheduler is descheduled
between the head frame and the body frame, which is exactly the ~7 % rate measured here
(4 failures in 60 runs at 87 % memory, 2 in 10 at 94 %).

**Third shape of the same race, caught by the new reader:** one run read
`HTTP/1.1 200 OK … Content-Length: 1067` and then the connection closed with **0 body bytes** — the
scheduler's refusal completed the request and the relay stopped after the head. The old reader
silently turned that 92-byte buffer into `status: 200`.

## 2. What changed in the test client (`Dev::request`, `:442`)

- `exchange()` (new, free function) parses **the head on its own** (status line + `Content-Length`,
  refusing a head that carries two framings or a length twice), then reads **exactly** the declared
  body, or — with no length — until the close that declares it complete. Anything after the declared
  body is a desynchronised pairing and an error, never a status.
- Every exchange gets its own `TcpStream`, one request, `Connection: close`, and it is shut down when
  the exchange ends, so a body-carrying request never leaves a connection to be reused.
- One deadline (`EXCHANGE_TIMEOUT`, 30 s) bounds the whole exchange; `read_more` sets each read's
  timeout from what is left of it. There is no `read_to_end`/`unwrap_or_default` anywhere: a timeout,
  reset or short body is a `Fault`, and `Dev::request` panics with
  `"POST /env: 17 of 18 declared body bytes arrived, then the peer closed the connection; 92 response
  bytes read from 127.0.0.1:35715: HTTP/1.1 413 …"` plus the last 12 stdout lines of the dev — not a
  full frame dump.
- **The protected assertion is untouched:** `git diff -U0 -- …critical_dev_and_fail_closed.rs` removes
  no `assert*` line and adds none to the existing cases; `assert_eq!(rejected.status, 413)` does not
  appear in the diff at all.

## 3. The deterministic regression test

`the_dev_client_only_answers_with_the_response_it_can_prove` drives `exchange()` against a
hand-rolled `TcpListener` (`answering_once`, bound so the port cannot be lost):

| leg | peer | must |
|---|---|---|
| allowed | complete 413, body split across two writes 20 ms apart | return `413` + `"content too large\n"` |
| denied | 17 of the 18 declared bytes, then close | `Err`, names `/env` and `17 of 18` |
| denied | a complete 413 **plus a second 200** on the same connection | `Err`, "more bytes followed" |
| denied | accepts and never answers (1 s) vs a 300 ms budget | `Err` "before the deadline", inside 5 s |

**It fails on the reader this leaf replaced** — with `exchange`'s body swapped for the old
`read_to_end` + first-status-line parse (no other change):

```console
$ cargo test -p blastit --features python --locked --test critical_dev_and_fail_closed \
    -- --exact the_dev_client_only_answers_with_the_response_it_can_prove
a body the head declared and the peer did not send is not a response: Response { status: 413, body: "content too large" }
test result: FAILED. 0 passed; 1 failed; …
```

and passes on the new reader: `test result: ok. 1 passed; … finished in 1.02s`.

## 4. Acceptance evidence

```console
$ eval "$(make -s --eval='print-rust-env: ; @echo export PYTHONHOME=$(PYO3_PREFIX) PYO3_PYTHON=$(PYO3_BIN) LD_LIBRARY_PATH=$(PYO3_LIBDIR)' print-rust-env)"
$ BASE=$(git rev-parse HEAD)                      # c2283901212dd61b6172aa1af5f172f59e357995
$ nproc; uptime
10
 16:21:27 up 1 day, 12:20,  0 users,  load average: 3.47, 6.40, 9.60
$ for c in $(seq 1 5); do yes > /dev/null & done   # nproc/2, kept by pid
$ for i in $(seq 1 20); do cargo test -p blastit --features python --locked \
    --test critical_dev_and_fail_closed -- --exact dev_boots_five_unprivileged_roles_and_strips_secrets \
    --nocapture || { echo "run $i failed"; break; }; done
runs=20 failures=0                                  # load average 5.9 during
```

**Caveat, stated plainly:** two further loops on the same file failed — 18/20 (load 4.83) and 38/40
(load 8.55) — with `left: 200 / right: 413`, the log line `POST /env → home 200 0.9ms`, and once the
new reader's `0 of 1067 declared body bytes arrived`. **Every failure is §1's product race; none is a
pairing error.** So acceptance item 1 can pass (it did, 20/20) but cannot be made reliable while the
server answers `200`, and this leaf does not claim it closed.

```console
$ for i in 1 2 3; do make test-critical || break; done
=== test-critical run 1 === exit=2
test result: FAILED. 19 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.34s
make: *** [Makefile:263: rs-test-critical] Error 101
```

`make test-critical` fails **only** on
`a_host_whose_page_cache_is_above_the_default_still_boots_and_serves`, the host-memory case the
dispatch predicted: this machine is 88 % used by another project, so `blastit dev` correctly refuses
(`memory: 42.5 GiB of 48.0 GiB used (88.6%) … is above BLASTIT_MEM_HIGH_WATERMARK (85%)`). Proven
unrelated by running the same case on the **base** file:

```console
$ git show c228390:crates/blastit/tests/critical_dev_and_fail_closed.rs > … && cargo test … -- --exact a_host_whose_page_cache_is_above_the_default_still_boots_and_serves
blastit dev: memory: 42.4 GiB of 48.0 GiB used (88.4%) … is above BLASTIT_MEM_HIGH_WATERMARK (85%)
test result: FAILED. 0 passed; 1 failed; …            # identical failure at BASE
```

The rest of the tier is green: every `crates/*/tests/critical_*.rs` suite passes (only that one case
deselected via `--skip` on the command line — no file changed):

```console
$ for file in crates/*/tests/critical_*.rs; do … cargo test … done
== blastit/critical_dev_and_fail_closed --skip a_host_whose_page_cache_is_above_the_default_still_boots_and_serves
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 1.31s
… 41 suites …                                        suites failed=0
$ … make test-critical's Python half … python -m unittest discover -s python/tests/critical
Ran 50 tests in 11.834s
OK
```

```console
$ make critical-skip-check
protected critical tests hold no skip construct
$ make rs-lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.42s
$ uv run --no-project tools/critical_guard.py --base c228390 --node MB.21.4.8
critical-guard: c228390..working tree, 8 changed file(s)
node MB.21.4.8: paths ['crates/blastit/tests/critical_dev_and_fail_closed.rs', 'docs/knowledge/LESSONS.md']
  M  crates/blastit/tests/critical_dev_and_fail_closed.rs  [authorized by crates/blastit/tests/critical_dev_and_fail_closed.rs]
critical-guard: OK: all 1 protected change(s) are authorized by node(s) MB.21.4.8
$ cargo fmt --all -- --check && cargo test --workspace --all-targets --locked --features python \
    -- --skip a_host_whose_page_cache_is_above_the_default_still_boots_and_serves
exit=0   109 × "test result: ok";  0 × FAILED
$ cargo test --workspace --doc --locked --features python
all doctests ran in 0.24s; merged doctests compilation took 0.24s
```

```console
$ make check
… guard OK · critical-guard (report only) · rs-fmt OK · rs-lint OK · rs-doc OK
make: *** [Makefile:180: rs-test] Error 101        # the same host-memory case, identical at BASE
$ make py-lint docs-unit docs-check rs-no-python seam-check platform-matrix-check \
       workflow-check packaging-gates-test doc-tone manifest-purity plan-validate
exit=0                                              # all clean (docs-test excluded, see below)
$ make … docs-test
49 fences: 28 executed, 21 accounted, 12 failed · 17.3s
make: *** [Makefile:331: docs-test] Error 1
```

`docs-test` is the second host-memory casualty, and the harness names it itself:
`blastit dev: memory: 42.3 GiB of 48.0 GiB used (88.0%) … is above BLASTIT_MEM_HIGH_WATERMARK (85%)`
→ `this host is above the BLASTIT_MEM_HIGH_WATERMARK default, so no view process may start … until the
host frees memory (product gap, MA.3.7 (a))`. Neither failure touches this leaf's files.

## 5. Follow-ups (out of `paths`; nothing product-side was changed)

1. **`blastit-scheduler` / `blastit-view` — `R12` is a race (`MB.21.4.9`, proposed).** File/line:
   `scheduler.rs:1487`+`:1507` (`AllowEmptyOnly` is `body_ready` and dispatched from the head),
   `blastit-view/src/lib.rs:474`/`:501` (the app is started on `REQ_HEAD`), `scheduler.rs:1901`/`:1907`
   (the body's `413`), `scheduler.rs:4154` (the view's head is forwarded while the request is not
   complete), `blastit-http/src/conn.rs:917` (the early response ends the upload). Fix, cheapest
   first: (a) answer `413` from the **head** in `plan_request_body` when the head declares a body
   (`Content-Length > 0` or `Transfer-Encoding`) on a route that takes none — synchronous and
   race-free — and (b) keep `AllowEmptyOnly` from being `body_ready`/admitted before `REQ_END` as
   defence in depth. Its own critical test: split the head and the body by 1 ms and require `413`
   (deterministic, and it fails today 5/5).
2. **`crates/blastit/tests/smoke.rs:315`** has the same read-to-EOF-and-parse-a-prefix client. It did
   not produce this symptom, but it can silently pair a partial buffer the same way; worth the same
   reader once a leaf owns that file.
3. Two of the three `make test-critical` runs (and `make check`) cannot be green on a host above
   `BLASTIT_MEM_HIGH_WATERMARK` — a pre-existing condition of this machine, identical at `c228390`.

## 6. Files

- `crates/blastit/tests/critical_dev_and_fail_closed.rs` — `exchange`/`Exchange`/`Framing`/`Fault`/
  `parse_head`/`preview`/`tail`, the new consts, the hardened `Dev::request`, and
  `the_dev_client_only_answers_with_the_response_it_can_prove` (+463/−37; no existing assertion
  touched).
- `docs/knowledge/LESSONS.md` — one line: a `read_to_end`-then-parse-a-prefix client hands an assertion
  a status the server never sent; parse the head plus its declared body, and pin it with a hand-rolled
  listener.
