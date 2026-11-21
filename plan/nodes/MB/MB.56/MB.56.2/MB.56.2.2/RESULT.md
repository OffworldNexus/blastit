# MB.56.2.2 — Delete non-§7 micro-tests in php-sapi, sys, env, ingress, py-shim, events, frame, py-sapi, lsp/src

## What was deleted (inventory)

| crate | `tests/**` deleted | inline `#[cfg(test)]` deleted | surviving |
|---|---|---|---|
| `blastit-php-sapi` | 1 782 LOC (`critical_file_sink`, `critical_shim_contract`, `php_native`) | 100 LOC (`src/loader.rs`) | none |
| `blastit-sys` | 1 816 LOC (`critical_child_sequence`, `critical_embedded_seccomp`, `critical_fanotify`) | 5 246 LOC across 14 files + `src/testutil.rs` | `src/elf.rs` malformed-ELF module (140 LOC) |
| `blastit-env` | 882 LOC (`environment.rs`) | 950 LOC across 5 files | none |
| `blastit-ingress` | **kept whole** — `critical_ingress.rs` is the §7 owner of `body` enforcement | 44 LOC (`form.rs`, `mime.rs`) | `tests/critical_ingress.rs` (818 LOC) |
| `blastit-py-shim` | 217 LOC (`abi_load.rs`, the `--ignored` `dlopen` probe) | 392 LOC across 7 files | none |
| `blastit-events` | 92 LOC (`lifecycle_build.rs`) | 3 194 LOC across 4 files + `src/fixture.rs` | none |
| `blastit-frame` | – | 2 211 LOC across 5 files + `src/testutil.rs` | `src/decoder.rs` corrupt/truncated-frame module (193 LOC) |
| `blastit-py-sapi` | – | 836 LOC across 4 files | none |
| `blastit-lsp/src` | – (`tests/**` is `.5`) | 400 LOC (`lib.rs`, `hover.rs`) | none |

Also removed: the two dead `#![cfg_attr(test, allow(...))]` lint crates-wide allowances
(`php-sapi/src/lib.rs`, `py-sapi/src/lib.rs`), the `#[cfg(any(test, feature = "testutil"))]
pub mod testutil;` declaration and the `testutil = []` feature in `blastit-sys/Cargo.toml`,
and the now-false `cfg(test)` paragraph in `py-shim/src/lib.rs`'s crate docs.
No `#[ignore]`, no skip construct and no `unwrap()`/`expect()` on untrusted input remains:
`rg 'cfg\(test\)'` over the nine crates names only `elf.rs:788` and `decoder.rs:221`.

Test-only support files deleted whole: `blastit-sys/src/testutil.rs` (368),
`blastit-frame/src/testutil.rs` (149), `blastit-events/src/fixture.rs` (58).
No production logic changed: every hunk is a deletion of a `#[cfg(test)]` item, plus the
`events::format_timestamp` helper that was itself `#[cfg(test)]`-gated and reached only by
the deleted suites.

## The two narrow exceptions (justified)

Both survive for one reason: the input is **bytes no real-binary run can produce**. A
`blastit` boot either refuses before it parses (tampered binary) or never writes a frame the
encoder would reject, so the refusal path has no user-visible trigger to drive E2E.

1. `crates/blastit-sys/src/elf.rs` — **140 LOC** (`mod tests`, 9 cases): no/short magic,
   12-byte `e_ident`, unknown class 3 and big-endian, 40-of-64-byte header, section table
   past EOF (including the `0xffff × 0xffff` multiplication wrap), zero sections / 32-byte
   entries, `sh_offset + sh_size` overflow, a section over the caller's limit, and a section
   index past the table. Cap ≤ 250, used 140.
2. `crates/blastit-frame/src/decoder.rs` — **193 LOC** (`mod tests`, 7 cases): a partial
   frame is not an error, a 4 GiB declared length is refused before buffering (and the
   decoder's buffer never grows), a 4-byte declared length, a poisoned decoder returns its
   first error for every later call, unknown type refused typed but forwarded raw, every
   truncated-but-"complete" payload of `REQ_HEAD`/`RESP_HEAD`/`RESP_ABORT`/`DONE` is
   refused, and a mutated/garbage stream never panics. Cap ≤ 200, used 193.

`blastit-frame`'s `view.chan` codec is an internal transport, not a §7 surface (§7 covers
`blastit-http`'s HTTP/1.1 framing). `blastit-sys`'s ELF walk is likewise not §7. Everything
else in both crates (round-trip, `encoded_len` agreement, enum ranges, Debug redaction,
limits/boundaries, client-IP trust, caps/seccomp/fanotify/socket behaviour) was deleted and
is recorded below for `MB.56.3`/`MB.56.4`.

`blastit-ingress` is **not** an exception: `TESTING_PLAN.md` §7 names "HTTP/1.1 framing &
**body enforcement**" as a focused surface, and `crates/blastit-ingress/tests/critical_ingress.rs`
is its only owner (`blastit-http` does not depend on `blastit-ingress`, so it cannot reach
codec transcoding). That suite is therefore kept whole; only its two inline `form`/`mime`
unit modules were deleted.

## Makefile legs fixed (so `make check` cannot break)

- `py-sapi` (reached by `make fast-check` → `rs-test` → `python-runtime`) lost its
  `cargo test -p blastit-py-shim --test abi_load -- --ignored` step and the
  `PY_SAPI_PROBE` knob; the leg keeps its real assertions (readelf `DT_SONAME`, undefined
  `Py_Initialize`, exported `blastit_py_*`), and the shim is proven end-to-end by the WSGI
  legs (`rs-test`, `e2e-universal --python 3.13/3.14`).
- `rs-test-php` dropped its `cargo test -p blastit-php-sapi --test critical_file_sink` half
  and `PHP_SINK_SUITE_ARGS`; the three `crates/blastit/tests/**` PHP suites remain.
- `rs-test-critical` needs no change: its `crates/*/tests/critical_*.rs` glob still finds
  the suites of `blastit`, `blastit-http`, `blastit-manifest`, `blastit-scheduler` and
  `blastit-lsp` and skips absent files.

## Unique user-visible cases the deleted suites covered (for `MB.56.3`/`MB.56.4`)

### §2 PHP runtimes and §6 zero-trust isolation (`blastit-php-sapi`, `blastit-php-sapi/src`)

- libphp is pinned by descriptor and loaded through `/proc/self/fd/<n>`; a native libphp serves one real request.
- `php2shell` is structurally absent: reserved server names unrepresentable, a proxy header never becomes `HTTP_PROXY`, `$_SERVER` name/value budgets bounded.
- A request reports **what it included**, never a file it merely read: the list is absolute, sorted, de-duplicated, carries the entry script exactly once, never names a `.env` read through `file_get_contents`, resets per request, and a contained `exit()` still reports its includes (§2 runtime compatibility, §6 per-view containment).
- The sink is optional and the ABI is unchanged: `BLASTIT_PHP_ABI_VERSION` stays `1`, the `blastit_php_sink_v1` struct stays frozen, and the shim never replaces `php_plain_files_wrapper` / `php_resolve_path` (§2).
- `ini` lockdown pins every system directive; `allow_url_fopen` follows the declared egress both ways; the ini policy refuses to render an injection.
- A missing legacy `file` sink is a no-op, never a failure; an installed sink receives exactly the paths the shim reports; the sink outlives a request and a moved loader; installing it before the worker starts is not the same answer as a missing symbol.
- A null or empty path is never turned into a slice, and the sink only ever reports paths the wire can carry.
- Toolchain discovery: a missing toolchain names `php-config` and the package to install; an unsupported minor is refused by name; a spawn failure is not reported as "not found"; a *text* file named `php-config` is not a spawn failure either; a fake toolchain on `PATH` is found; the shim source is self-contained and guarded; the build reports one consistent world; the Rust ABI mirror matches the C layout.
- Version parsing only succeeds where a shim exists for that minor.
- Loader refusals are typed and never panic: a non-library file, a missing library, a path with a NUL (refused before `dlopen`), and ABI/minor mismatches each name their fix.

### §5/§6 environment rendering (`blastit-env`)

- §5.1's worked proxy example renders end to end; no output table rewrites a variable in place; the split input form gets the default outputs; a URL plus a split slot aborts the boot.
- Boot refusals name the variable: a missing input variable, an unknown slot, a malformed URL (without repeating the URL), a URL with no database, a bad split port, a multi-host input, an unusable output variable name, an output naming a `BLASTIT_*` variable, two proxies producing one variable, and two failing proxies reporting both errors.
- A URL may not smuggle configuration (`?sslmode=`-style query parameters must be allowlisted); percent-encoded passwords containing `@`/`:` decode; database names that need encoding are encoded; an IPv6 literal loses its brackets.
- Output templates support both filters and any whitespace; a template error aborts; an output may not overwrite a variable it does not own but may redefine its own input.
- Every `BLASTIT_*` variable is removed from the app environment, from the environment *and* the argument strip list; injected defaults never override a value already present and are still injected into an empty environment.
- Every error message names variables and never a value; secret `Debug` redacts the value; error text escapes control characters and renders one line per error.

### §7 `body` policies (`blastit-ingress`, retained whole — not deleted)

`crates/blastit-ingress/tests/critical_ingress.rs` (818 LOC) stays as the §7 owner of the
documented `body` refusals: `body json` → `400` naming the observed `Content-Type`, `body
form` → `400`, `body none` + a body → `413`, `body raw unvalidated` identity, and
duplicate-key/BOM/lone-surrogate/bad-escape/NUL refusals. What it pins:

- JSON: vendor media types canonicalised; keys sorted and whitespace stripped; exact `u64::MAX`/`i64::MIN`; depth 64 accepted and 65 refused; distinct keys accepted and duplicates refused; surrogate pair accepted, lone surrogate refused; BOM, trailing bytes, invalid UTF-8, empty body, non-finite number, `Content-Type` mismatch and non-UTF-8 charset refused; backslash and quote re-escaped, short control escapes and lowercase `\u` emitted, raw control byte and unknown escape refused.
- Form: `;` re-encoded, a raw `;` kept as data, lowercase hex normalised; empty body accepted; bad escape, NUL, invalid UTF-8, non-UTF-8 charset and `Content-Type` mismatch refused.
- `raw` streams with zero buffering and transcode is the identity; `Content-Encoding` on a structured body is refused, `identity` accepted.
- Undeclared routes and `none` policy allow only empty bodies; bodyless safe methods reject bodies; `DELETE` with a body follows POST rules and without one needs no `Content-Type`.
- `plan_body` checks the `Content-Type` before buffering; a refused body maps to `413`; transcoded headers keep one canonical `Content-Length`, strip `Transfer-Encoding`, and emit only a length when no `Content-Type` was sent.
- `parse_mime` returns essence plus charset, caps the essence, and dedupes repeated names while keeping order.

### §4/§5 operator log lines (`blastit-events`)

- Every event kind round-trips through the JSON decoder and encoder; each variant's fields are written in the documented order and a line this build did not write is not an event.
- A bailed request decodes with its own outcome; a request with no cost report still carries its fields; non-finite timings stay valid JSON.
- Truncation is a property: a long `sql`, `app.output` line, `stage`, capability list, preset setting or immortalisation reason is cut and named in `truncated`, the longest field goes first, whole proxies are dropped until the line fits, and a declared `truncated` field is reported even when the event fits.
- A boot event names the preset and its settings (or `null`), reports the privilege mode and capabilities, and reports a skipped immortalisation with its reason; runtime entries for php/python/static and `dev.reload`/`dev.reload.unavailable` have their documented shape and round-trip.
- Text form: one line per kind, aligned name column, unmatched requests say so, a `cap` event reports the measurement and the cap, lifecycle lines show variable *names* never values, colours wrap only the level word and a request status class, long lines are capped with an ellipsis, control characters never reach the terminal raw.
- `app.output` splitting: a short line is one event, an empty line one empty event, a long ASCII or Unicode line is split exactly (every character kept), a line that grows when escaped is still split, part counts and digit widths are consistent, an over-long shared route is cut once on a character boundary, and every piece keeps the given timestamp.

### §6 scheduling/transport internals (`blastit-frame`, `blastit-sys`)

- Frame round-trip for every kind and through the raw layer; `encoded_len` matches `encode` and rejects exactly what it rejects; little-endian header; length covers everything after itself; empty/max payloads; one byte over the limit; output buffer untouched on error; custom limits; id-0 rules for streamless frames; distinct documented type codes; undefined enum values, non-UTF-8 strings, out-of-range status and signal refused; `Debug` never prints header values or bodies; cost report in both wire forms; raw peer distinct from resolved client; trailing bytes and lying header counts refused without huge allocation.
- Limits: 1 MiB default, out-of-range values refused, total/declared-length boundaries, body chunks respect the limit, empty body produces no frames, a big body is split rather than rejected.
- Client IP: an untrusted peer is the client; a trusted peer makes the header decide; a forged leftmost entry never becomes the client; a chain of trusted entries takes the leftmost; an unparsable entry makes the whole header ignored; empty elements skipped; every instance of the header is part of the chain; case-insensitive name; IPv4-mapped addresses normalised; bracketed/port-carrying and zoned peers read; the app is told the address and never the port; the default is localhost only; `trusted_proxies` accepts IPs, CIDRs and `*` and refuses a port or a non-header name.
- `critical_child_sequence`: a child that entered is not dumpable, carries a seccomp filter and is a group leader before the embedded view filter; a symlinked working directory is refused; a refused privilege drop fails at the drop step.
- `critical_embedded_seccomp`: both views refuse `execve`/`clone`/namespaces (the plain view keeps exec and process creation), refuse `clone3` yet still start a thread, refuse `setsid`/`setpgid`, share the hardening, and the embedded view's egress reflects its flag.
- Inline `blastit-sys`: capability status parsing (`CapInh/Prm/Eff/Bnd/Amb`, short/upper/padded hex, missing `CapAmb`, bad field is an error), capability naming and `setcap` line, root-with-whole-set vs capabilities, effective set decides, six missing capabilities named, `setpcap` wanted-not-needed, child drops all, no-`setpcap` bounding set, capset failure reported, `verify` notices a held capability, `CAP_LAST_CAP`.
- `fanotify`: measured layout vs libc, 16 KiB aligned read buffer, init flags/mark mask, errno→op failure ladder, filesystem magics, handle keys, record parsing (name to first NUL, padding collapse, malformed record skipped and the next event kept, overflow and short buffer, non-UTF-8 names), kernel version parsing; `fanotify_init` refusals on a filesystem without handles.
- `sock`: socketpair/pipe CLOEXEC, `SCM_RIGHTS` send/recv round trip and control-message lengths, no-descriptor case, extra descriptors closed and message refused, empty payload refused, credentials do not confuse `recv_with_fd`, control buffer not over-read; `bind_tcp` `SO_REUSEADDR`/accept; `bind_unix_listener` mode/`SO_PASSCRED`/owner, exact mode, stale socket replaced, regular file/symlink/directory refused, closed to another uid; `chmod_socket_via_proc` and its symlink refusal; `make_private_dir` modes, existing dir fix-up, symlink/file/parent-is-file refused.
- `exec`/`proc`/`pid`/`signal`/`fd`/`privs`/`pin`/`mem`/`caps`: exec with empty/given environment and a missing program, exe path is the `/proc` entry; fork refuses a multi-threaded caller, `chdir` moves and reports missing; exit status both ways and ignoring a stopped child; signalfd mask/CLOEXEC/non-blocking, `SIGCHLD` reaping with code and signal, kill of a waiting/gone child; `dup_onto` and `close_fds_except[_scan]`; umask/rlimits/`PDEATHSIG`/`NO_NEW_PRIVS`/dumpable/drop-privileges ordering and rootless capability clearing; pinned object through `/proc`, replaced object refused with both identities, symlink/non-regular refused, descriptor survives unlink; meminfo/cgroup v1+v2 sampling and clamping.

### §4 Python WSGI runtime (`blastit-py-sapi`, `blastit-py-shim`)

- Discovery sentences name the fix verbatim: invalid or relative `BLASTIT_PYTHON`, a free-threaded interpreter, no interpreter (naming where it looked and the one command), an unsupported minor, a missing `libpython` (naming the minor and every remedy), an unsupported-minor venv; a php-only or static-only boot never searches for an interpreter; an unset `PATH` searches the POSIX defaults; a specific interpreter beside the generic one wins; an old `python3` alone is refused by its own minor; a set `PATH` is searched in its own order; the shim search is scope-dependent and the development refusal names its variable; a production boot ignores the activated venv; `pyvenv.cfg` decides the minor after an in-place upgrade; the interpreter name parses dotted and packed spellings; the shim name/minor table are the design's.
- Loader: a text file is refused as not an ELF, a real ELF without the frozen entry points is refused by name, ABI/minor refusals name both versions, a real `libpython` is gated and loaded when the caller names one.
- Frozen ABI: minor encoded as `major × 100 + minor`, buffer is three words in the design order, zygote marker occupies no storage, vtable is ten function pointers in the frozen order with the frozen signatures, fork hooks are three pointers plus a phantom lifetime; an unwritten slot is never read, the refusal placeholder names itself, a null zygote names nothing.
- Shim surface: every vtable slot is the matching boundary function, the exports and the vtable report the same frozen versions, a null zygote is refused with a sentence, `free_buf` of nothing is harmless, a written answer round-trips and is reclaimed once, a null slot is never dereferenced, a lying length is refused instead of corrupting the heap, an error is JSON on the error channel, a panic becomes a refusal plus a sentence, the two refusal codes are the frozen ones, the boot document is read field by field, an unknown field is refused, malformed JSON names itself, the freeze follows the immortalisation pass, a NUL byte (in the source and in the path) is refused before the interpreter is touched, and a child without a view descriptor is refused by name.

### §7 language server (`blastit-lsp/src`)

- A char column is an off-by-one and a line is zero-based; a non-BMP character costs two UTF-16 units; a BMP character does not discriminate; the squiggle covers the word or one unit; a position past the document clamps; the end of an edit is `MAX_POINT_UTF16`; the char index follows UTF-16 units; the word under the cursor is the run and its `$`.
- A header block declares exactly one `Content-Length`; the offset is bytes while the point is UTF-16; the two SQL tokens are source spans; a range meets an interior by overlap or caret; the interior is re-encoded for its spelling; `argv` is a version probe or one sentence.
- Hover: the SQL paragraphs are the page's blockquotes and the hover table is bounded and transcribed.
- Ownership: `crates/blastit-lsp/tests/critical_lsp_stdio.rs` pins **only** the session
  handshake and the diagnostic span; hover text, `textDocument/formatting` edits and the
  `$?` quick fix are pinned by `crates/blastit/tests/critical_lsp.rs`, which `MB.56.3`
  deletes and `MB.56.2.5` re-homes into `crates/blastit-lsp/tests/e2e_lsp.rs`. The inline
  modules deleted here were pure position/offset helpers for that owner.

## Review fix (same slice)

- Inventory: added the two `critical_file_sink` cases nothing else covers (include
  reporting/`exit()` snapshot, and the optional-sink/unchanged-ABI contract) to the §2/§6
  inventory above.
- `e2e/php-sapi/Dockerfile` builds only the two surviving `-p blastit` suites
  (`critical_php_backend`, `critical_php_embed`) and expects `count -ne 2`;
  `Makefile`'s `rs-test-php-image` expectation matches at 2 (its `critical_php_embed` maps
  witness is unchanged, and `Dockerfile.two-package` already expected 2).
- `crates/blastit-sys/src/proc.rs` no longer points at the deleted `crate::testutil`.
- `PY_SAPI_PROBE` is gone from every caller (`publish-pypi.yml`, `scripts/build-wheel.sh`,
  `scripts/build-standalone-in-image.sh`, `scripts/py_sapi_leg.sh`) and every comment that
  promised a `dlopen` probe now states what the legs prove (readelf `DT_SONAME`, undefined
  `Py_Initialize*`, the three exports); `scripts/platform_matrix.py` no longer claims
  `critical_file_sink` 8/8.

## Follow-ups (out of this node's paths)

- `blastit-ingress` **is a §7 owner** (`body` policies) and its suite is retained whole, so
  nothing about body enforcement moves to `MB.56.4`; `blastit-http` owns request framing only
  and does not depend on `blastit-ingress`.
- The refusal sentences the deleted `blastit-py-sapi` suites pinned (`invalid
  BLASTIT_PYTHON`, a relative path in a deployment, a free-threaded interpreter, no
  interpreter, an unsupported minor, a missing libpython) and the deleted `blastit-env` ones
  (a missing input variable, an unknown slot, a malformed URL, two proxies sharing a value,
  an output naming a `BLASTIT_*` variable) are routed to `MB.56.3`'s
  `e2e_prod_refusals.rs` and `e2e_dev.rs` (already logged there).
- Re-coverage targets: the LSP helper semantics → `MB.56.2.5` (`e2e_lsp.rs`), every
  §2/§4/§6 bullet above → `MB.56.3`'s E2E suites against the real binary.

## Evidence

```
$ rg 'cfg\(test\)' <the nine crates>
crates/blastit-frame/src/decoder.rs:221:#[cfg(test)]
crates/blastit-sys/src/elf.rs:788:#[cfg(test)]

$ awk '/^#\[cfg\(test\)\]/{p=1} p' crates/blastit-sys/src/elf.rs | wc -l      -> 140
$ awk '/^#\[cfg\(test\)\]/{p=1} p' crates/blastit-frame/src/decoder.rs | wc -l -> 193

$ cargo test -p blastit-sys --locked elf::      -> test result: ok. 9 passed; 0 failed; 0 ignored
$ cargo test -p blastit-frame --locked decoder:: -> test result: ok. 7 passed; 0 failed; 0 ignored

$ git checkout e13cf58 -- crates/blastit-ingress/tests/critical_ingress.rs   # 818 LOC, kept (§7)
$ cargo test -p blastit-ingress --locked
running 55 tests
test result: ok. 55 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
running 1 test
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # doctest

$ make rs-lint
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.11s

$ bash -n scripts/build-wheel.sh && bash -n scripts/build-standalone-in-image.sh \
      && bash -n scripts/py_sapi_leg.sh            # ok / ok / ok
$ uv run --no-project python -c "import ast; ast.parse(open('scripts/platform_matrix.py').read())"
$ make -n py-sapi; echo $?                        # 0
$ make -n rs-test-php-image; echo $?              # 0 (records `-ne 2`, "expected 2")
$ rg 'PY_SAPI_PROBE|dlopen. probe|critical_file_sink' Makefile scripts e2e .github crates
                                                  # no matches

$ make fast-check
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.11s      # rs-lint, 0 warnings
    test result: ok. N passed; 0 failed; 0 ignored ...                        # rs-test, every suite
    ok   target/debug/blastit: NEEDED libgcc_s.so.1 libm.so.6 libc.so.6 ...  # portable ELF
    test result: ok. 16 passed; 0 failed; 0 ignored ...                       # critical_handoff
    install-test: all cases passed (fixture on http://127.0.0.1:8745, ...)
fast-check: exit 0
```
