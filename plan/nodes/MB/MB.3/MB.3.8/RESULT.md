# RESULT — MB.3.8

Release prep for `0.2.0`: the dated changelog now covers static assets, the
pooler presets, embedded PHP (honestly: the host's `libphp`, no bundling, no
fallback), the portable standalone binary and the `raw` boot refusal;
`ARCHITECTURE.md` and `docs/security.md` describe what shipped; the design page
and ADR-0007 were checked against the code.

STATUS: done

## Summary

**Version and lock — confirmed, no edit needed.** `Cargo.toml` is
`[workspace.package] version = "0.2.0"` (no `0.2.1` bump). All **30**
intra-workspace `version = …, path = "../…"` requirements name `0.2.0`, including
`crates/blastit-php-sapi` and `crates/blastit-view`. Every workspace package in
`Cargo.lock` is `0.2.0`; `cargo metadata --locked` agrees. The last lock-chain
link therefore moved nothing — the tree was already consistent at base, so
`Cargo.toml`/`Cargo.lock` (and `crates/*/Cargo.toml`) are untouched.

**`CHANGELOG.md`.** The dated entry is now `## 0.2.0 (2026-11-15)` (the release
date; the entry was drafted on the 14th). It keeps the five static bullets and
the pooler bullet, and gains, in the file's voice (what changes for whoever runs
an app):

- **PHP runs in the same worker** — `[route.php]` + `public/index.php`, no
  `php-fpm`/FastCGI/nginx rewrite, static root beside it, `workers` (1–64) only
  lowers concurrency.
- **PHP is the host's, and the boot names the package** — NTS `libphp`, no
  bundling and no fallback; `libphp-embed` (Debian/Ubuntu), `php84-embed`
  (Alpine), `php-embedded` (Fedora/RHEL), or `BLASTIT_LIBPHP`.
- **The CGI surface is absent, not filtered** — no CGI binary/argv or FastCGI
  parameter stream; `SCRIPT_FILENAME`/`DOCUMENT_ROOT` pinned in C; `.php`/`.phar`
  under a static root is `404`; `EmbeddedView` refuses `execve`/process creation.
- **`dev --learn` / `check` know about PHP routes**: `dev --learn` writes the
  grants; `check --learn` proposes new routes only, and a grant for a statement
  on an existing route is the 0.2.x gap. The request-level audit stays a
  Python-suite feature in 0.2.0.
- **The binary a plain build makes is portable** — `cargo build -p blastit`, no
  `libpython`, no build-host `RUNPATH`, proven in an image with no CPython.
- **`body = "raw"` is refused at boot, not warned about** — production refuses
  before the bind, naming the route and the key; `dev` warns, `check` reports
  `raw_ingress`.

**`ARCHITECTURE.md`.**

- New **§2.6 php (embedded)**: one worker per slot under `UID_BASE+1000+i`, the
  same `view.chan`/`WorkerRuntime` transport (`PhpEmbedRuntime`, raw frames on the
  scheduler side, credit-paced response, `RESP_END_COST`+`DONE`), stage-0
  `.dynsym` discovery and stage-1 `(st_dev, st_ino)` re-open + `ChildIo.keep` +
  `/proc/self/fd/<n>` `dlopen`, the pre-bind probe child, the C-memory `$_SERVER`
  and pinned `SCRIPT_FILENAME`/`DOCUMENT_ROOT`, the once-rendered
  `ZEND_INI_SYSTEM` blob, `php_module_startup` per forked worker on its main
  thread, `EmbeddedView`, `app.output`, and `CLOCK_PROCESS_CPUTIME_ID` /
  `DoneOutcome::Bailed` accounting.
- **§2** no longer says CPython is the only embedded interpreter; the component
  table's `Python?` column became `Interpreter?` and the `view` row names
  `libphp`; a note says `[route.static]` adds no row.
- **§4** now states the `ServeGates` proof: `ServeGates::clear` is the only
  constructor and *is* `raw_ingress_gate`, `bind_view_sockets` takes
  `&ServeGates`, so deleting the call site is a compile error; the process
  behaviour is measured through stage 1's re-read.
- **§7.3** is no longer stale against `seccomp.rs`: `clone3 → ENOSYS`, `clone`
  with any `CLONE_NEW* → EPERM`, `setsid`/`setpgid → EPERM`, and
  `SeccompRole::EmbeddedView` (`execve`/`execveat`/`fork`/`vfork`/non-thread
  `clone` refused).
- §2.4 already carried `DoneOutcome::Bailed` and the three 5xx charge rules; it
  was checked against `blastit-view/src/runtime.rs` / `blastit-events` and left
  as written.

**`docs/security.md`.** New **PHP front controllers** section: no CGI surface,
no request-parameter stream (`PHP_VALUE`/`PHP_ADMIN_VALUE`/`REDIRECT_STATUS`
unrepresentable, `Proxy:` dropped, INI rendered once, process functions
disabled), no process escape (`EmbeddedView`), the host/libphp fd-verification
and no-fallback rule, the `.php`/`.phar` `404`, and the pre-MC `raw` upload
caveat. The seccomp bullet gained the `clone3`/`CLONE_NEW*`/`setsid`/`setpgid`
and PHP-view facts; the scheduler row names the `.php` refusal; the stale
`0.1.1` marker on the multipart paragraph is now `0.2.0`. The `raw` gate
paragraph already described stage 0 + stage 1's re-read (MB.3.53) and was kept.

**`docs/design/static-and-spa.md`.** No `0.2.1` left in an owned file: line 41
says `0.2.0` ships the responder and the `public/index.php` worker; §2's
non-goal is "no FastCGI worker / no external PHP process" rather than "no PHP
execution"; §5.3 says the embedded `libphp` front controller inherits the
refusals and adds the source-file `404`; §5.1 records the scheduler's
socket-less role and `SeccompRole::EmbeddedView { egress }`, citing ADR-0010
§1–§2 and `ARCHITECTURE.md` §1/§2.6.

**`docs/decisions/ADR-0007-…`.** No edit, deliberately: ADR-0010 does not amend
ADR-0007 — §1–§2 restore decision 1's zero-`socket()` scheduler, and the ADR-0008
decision 7 `AF_UNIX` allowance went with the retired pool (MB.3.17). The
acceptance greps pass on the file as it stands.

**Certification checklist.**

1. **No `.php`/`.phar` under a static root is servable — certified.**
   Protected suite `crates/blastit-scheduler/tests/critical_static_entry.rs`
   (MB.3.33), which asserts the refusal happens before the method/body gates,
   without any read-open, for every case: `php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates`,
   `a_source_file_is_hidden_at_resolve_without_any_read_open`,
   `an_unreadable_php_file_is_404_not_a_read_failure`,
   `a_forged_source_url_path_is_never_streamed`,
   `a_source_index_never_answers_the_directory_or_the_spa_fallback`. Ran green in
   this tree (log lines 1787 and 3170). MB.9.2.2 must preserve it.
2. **`body = "raw"` without `allow_raw` refuses at boot in production —
   certified: stage 1 by protected test (`critical_raw_ingress`), stage 0 by the
   `ServeGates` type — `ServeGates::clear` is the only constructor and IS
   `raw_ingress_gate`, `bind_view_sockets` demands the token — so deleting or
   reordering it is a compile error, not a runtime test; stage 0's call site is
   unwitnessed at runtime.** The protected cases are
   `crates/blastit/tests/critical_raw_ingress.rs::production_stage_one_refuses_raw_ingress_without_allow_raw`
   (exit 1, names the route/table/key, no `ready`) and
   `…::production_stage_one_refuses_a_manifest_edited_after_stage_zero_approved_it`
   (the stage-0→stage-1 TOCTOU). Stage 0's ordering is the type, not a test: an
   unprivileged `cargo test` cannot reach it (`caps::detect` refuses first, as
   the suite header documents); the type is unit-tested by
   `stage0::raw_ingress_gate_tests::the_serve_path_proof_is_withheld_from_a_refused_manifest`.

**`docs/VISION.md`: the overclaim is fixed, by the strategist.** Line 25
promised one CI pass over “every … outgoing query/call”, but
`crates/blastit/src/audit_v1.rs:1360` drops every `ObsTarget::Routed` statement,
so `check --learn` cannot add a grant to an existing route. My attempt-1
“no false sentence” was wrong. The owner's ruling kept VISION out of this node's
hands, and the amendment has **landed** as commit
`5332f07` (“Qualify VISION.md single-pass --learn scope (dev vs check)”), which
qualifies the bullet (and the Milestone A/E mentions) with the reviewer's
sentence:

> **Single-pass wire learning (`--learn`):** Because Blastit sits on the HTTP ingress and every outgoing wire protocol, it records every route hit and outgoing query/call across an entire test run in one pass — language-agnostically (Python, PHP, Node, Java) — instead of failing on the first query per request; `dev --learn` writes those grants, while `check --learn` proposes new routes only, and a grant for a statement on an existing route is still the `0.2.x` gap.

There is no escalation item left. I did not edit `docs/VISION.md`.

No shipped material repeats the overclaim: the 0.1.1 changelog bullet is
**restored verbatim** and carries a dated correction inside its own section,
the 0.2.0 bullet says `dev --learn` writes and `check --learn` proposes new
routes only (grant on an existing route = 0.2.x gap), and ARCHITECTURE's
`blastit check` row says the same; `docs/security.md` never made the claim.

**Attempt 3 (review fixes).** Restored the released 0.1.1 bullet verbatim and
added the dated correction inside the 0.1.1 section (below its heading, so
`## 0.2.0 (2026-11-15)` stays the top `## ` heading); kept ARCHITECTURE's
in-place correction (living document); recorded the landed VISION amendment
(`5332f07`) and removed the escalation item; kept the three `0.2.0` PHP markers
and the two-proof certification wording.

## Evidence

```
$ grep -q '^version = "0.2.0"$' Cargo.toml && echo "workspace version is 0.2.0 (no bump)"
workspace version is 0.2.0 (no bump)
$ ! grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep 'version = ' | grep -v 'version = "0.2.0"' && echo "every intra-workspace requirement names 0.2.0"
every intra-workspace requirement names 0.2.0
$ grep -rn 'path = "\.\./' crates/*/Cargo.toml | grep -c 'version = "0.2.0"'
30
$ CARGO_NET_OFFLINE=true cargo metadata --locked --no-deps --format-version 1 > /dev/null && echo "lockfile agrees"
lockfile agrees
```

```
$ CARGO_NET_OFFLINE=true cargo check --workspace
    Checking blastit-view v0.2.0 (…/.worktrees/release-0.2.0/crates/blastit-view)
    Checking blastit-py v0.2.0 (…/.worktrees/release-0.2.0/crates/blastit-py)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.79s
exit=0
```

```
$ uv run --no-project scripts/check-release-tag.sh v0.2.0
tag v0.2.0 names the workspace version 0.2.0
exit=0
$ uv run --no-project scripts/release-notes.sh --require-dated v0.2.0 > /dev/null
exit=0
$ grep -q '^## 0.2.0 (' CHANGELOG.md && echo "dated changelog entry"
dated changelog entry
$ grep -q 'libphp' ARCHITECTURE.md && grep -qi 'php' ARCHITECTURE.md && echo "ok"
ok
$ grep -q 'may not call' docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md \
  && grep -q 'socket()' docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md \
  && ! grep -q 'Amended by ADR-0010' docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md \
  && echo "ADR-0007 decision 1 stands unamended"
ADR-0007 decision 1 stands unamended
```

```
$ make check
… (guard: OK; critical-guard: 11 changed files; cargo fmt/clippy/rustdoc/tests green;
     critical suites include critical_static_entry + critical_raw_ingress; 83 docs-unit
     tests; zensical --strict "No issues found"; docs-test fences PASS; rs-no-python:
     "default build: 0 pyo3 dependencies (no libpython linked)")
uv run --no-project tools/plan.py validate
plan OK
exit=0
```

```
$ grep -n 'critical_static_entry\|critical_raw_ingress\|php_phtml_and_phar\|production_stage_one_refuses' /tmp/opencode/mb38-make-check-a3.log | head
393:test production_stage_one_refuses_raw_ingress_without_allow_raw ... ok
1787:test php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates ... ok
3170:test php_phtml_and_phar_final_segments_are_404_before_the_method_and_body_gates ... ok
3445:test production_stage_one_refuses_raw_ingress_without_allow_raw ... ok
3494:83 passed, 65 subtests passed in 0.66s
3497:No issues found
```

```
$ grep -n "^## " CHANGELOG.md | head -3
8:## 0.2.0 (2026-11-15)
78:## 0.1.2 (2026-11-13)
92:## 0.1.1 (2026-11-12)
```

The released 0.1.1 bullet, restored verbatim (identical to
`git show HEAD:CHANGELOG.md`'s lines):

```
- **Fill the manifest in one pass.** `blastit check --learn` with a test suite,
  or `blastit dev --learn` while you click through the app, records the routes
  and statements it sees instead of denying at the first gap, adds them to
  `blastit.toml`, and prints the path it wrote
  ([fill the manifest in one pass](https://offworldnexus.github.io/blastit/local/#fill-the-manifest-in-one-pass)).
```

The dated correction, at the end of the 0.1.1 section (below its heading,
above `## 0.1.0`):

```
(corrected in 0.2.0: `check --learn` proposes new routes; grants for statements
on an existing route are written by `dev --learn` and are the 0.2.x gap —
MB.3.51.)
```

```
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.25s
exit=0
```

```
$ make e2e
… scripts/build-wheel.sh
E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
…
PASS: docs/frameworks/static-and-spa.md replayed against a real blastit dev (67 checks)
…
D6: an unprivileged boot with no capabilities is refused
  ok   the boot exits 1 (1)
  ok   and the message names the uid and every capability it lacks (…)
  ok   uid 21003 (pid 62) holds nothing and has no_new_privs (… nnp=1)
  ok   the worker's uid cannot run it (sh: 1: /usr/local/bin/python3.14-init: Permission denied)
EXIT=0
```

Run as uid 1000 with Docker access (the driver runs the e2e server inside
containers and the rootless leg in `docker run`); 450 `PASS:`/`ok` checks, no
`FAIL`.

## Files

- `CHANGELOG.md` — `## 0.2.0 (2026-11-15)` with the five areas; the released 0.1.1 bullet restored verbatim + a dated correction inside the 0.1.1 section (heading order unchanged).
- `ARCHITECTURE.md` — §2.6 new; §2 components/table; §4 `ServeGates`; §7.3 seccomp; `blastit check --learn` row says “new routes only”.
- `docs/security.md` — PHP front controllers; seccomp/`EmbeddedView`; scheduler row; `0.2.0` marker.
- `docs/design/static-and-spa.md` — 0.2.1→0.2.0, FastCGI→embedded `libphp`, §5.1/§5.3.
- `docs/decisions/README.md` — ADR-0010's version marker `0.2.1`→`0.2.0`.
- `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md` — `:14` and `:69` version markers `0.2.1`→`0.2.0`; nothing else.
- `plan/nodes/MB/MB.3/MB.3.8/RESULT.md` — this file.
- No change: `Cargo.toml`, `Cargo.lock`, `crates/*/Cargo.toml`, `scripts/*`,
  `docs/decisions/ADR-0007-…` (consistent / deliberately unamended).

## Follow-ups

- The `check --learn` gap is a code gap, not a docs gap: MB.3.51 owns it, and
  `docs/VISION.md` now says so (`5332f07`). Nothing further for this node.
- `ViewUpstream` is named in ADR-0010 and design as if it existed; in this tree it
  is only a `MB.9.3` contract comment in `blastit-view/src/runtime.rs`. §2.6 says
  “the scheduler’s upstream session” rather than naming a type that does not
  exist yet.
- `ADR-0010` §2’s `disable_functions` list mentions `putenv`, which the code’s
  `DISABLED_FUNCTIONS` does not remove. Align the ADR when it is next edited.
- `docs/frameworks/php.md` §8 says a PHP manifest has no request audit in 0.2.0;
  the changelog repeats that honestly. If PHP’s `check` audit lands in `0.2.x`,
  both must move together.

RELEASABLE: yes
