---
id: MB.3.37
title: "Protected oracle gaps in critical_php_embed: shim inode pin, egress twin, cpu-vs-wall, POST body"
kind: task
status: ready
owner: implementer
priority: 2
depends_on: []
paths: [crates/blastit/tests/critical_php_embed.rs]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T01:42"
updated: "2026-11-15T01:42"
---

# Goal

Close the four oracle gaps the `AUDIT-MB.3.13` curation found in the **protected**
`crates/blastit/tests/critical_php_embed.rs`. Each one is a mutation that leaves
`make check` green today:

1. **The shim's `(st_dev, st_ino)` pin has no protected witness.** `ADR-0010` §3 and
   design `R7` record `(path, st_dev, st_ino)` for **both** objects, and stage 1
   re-opens both `O_NOFOLLOW` and compares each against its own record. The suite
   swaps only `libphp`; the shim half lives only in the unprotected unit test
   `php_view::tests::a_replaced_libphp_or_shim_is_refused_by_name` (that file is
   `src/`, so whoever removes the shim comparison also owns its test). Add a leg
   that renames a **new shim** over the recorded one between stage 0's announcement
   and stage 1's re-open, and asserts exit `1`, the inode sentence naming
   `shim <path>`, no `ready`/`spawned`/`request`, nothing listening, and no stub
   canary. Reuse `Boot::start_swapping` (act on "the [route.php] runtime is" and
   replace the shim with a copy of the same stub), keeping the bounded 4-attempt
   retry and the "a removed pin produces the same served attempts" panic text.
2. **`egress` has only one direction in a protected test.** The probe child's
   `inet == EPERM` leg pins `SeccompRole::EmbeddedView { egress: false }`; nothing
   pins `egress = true`, so `probe_bindings`'s
   `egress.get(slot).copied().unwrap_or(false)` could hardcode `false` (silently
   denying every `egress = true` PHP route, and disagreeing with the
   `allow_url_fopen=1` the INI renders — the epic's "the INI value and the filter
   can never disagree"). Add the twin of the hang leg with `egress = true` on
   `[[route]]`: the same stub must report `inet == 0` (the socket is allowed) and
   the boot must still refuse at `BLASTIT_PROBE_TIMEOUT_MS=250`, naming the
   timeout.
3. **Nothing distinguishes `cpu_ns` from wall time.** Every fixture is a busy loop,
   and the suite asserts `cpu_ns <= wall_ns`, `cpu_ns > 0 || wall_ns < 1 ms`, and
   `cost_ns >= cpu_ns`. A shim that reported the request's own duration as
   `cpu_ns` satisfies all three. Add a real-pair leg whose front controller blocks
   (`usleep(200000)`) and assert `cpu_ns` is far below the event's `duration_ms`
   (e.g. `cpu_ns < wall_ns / 4` for a 200 ms block): the reading is
   `CLOCK_PROCESS_CPUTIME_ID`, not `duration`.
4. **No protected test that a request body reaches PHP.** The documented manifest
   in `docs/frameworks/php.md` carries `[route.ingress] body = "form"`, and
   `php2shell`'s class is about request data reaching PHP; neither protected suite
   ever POSTs. Add a real-pair leg: the manifest declares
   `[route.ingress] body = "form"`, the front controller echoes `$_POST['x']`, a
   `POST /form` with `x=1` answers `200` with the echoed value, and a body past the
   worker's `max_body_bytes` is a `4xx`/`5xx` after which the slot still serves the
   next request.

# Context

- `docs/testing/AUDIT-MB.3.13.md` (same commit) records each gap with file, line and
  why the existing assertion still passes if the rule is deleted.
- Every leg keeps the suite's **two-outcome** rule: `host_php()` decides, and on a
  PHP-free host the leg asserts the §4.1 package sentence — never a skip. `cc` is
  already a hard requirement.
- Legs 1 and 2 need no host PHP. Legs 3 and 4 are real-pair; their CI witness is the
  sibling task that adds this suite to `.github/workflows/php-sapi.yml`.
- This node may modify the protected file because its `paths` name it verbatim and
  `# Goal` is the reason `critical_guard.py` requires.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_php_embed
running 12 tests
test a_shim_replaced_between_the_stages_refuses_the_boot ... ok
test an_egress_route_lets_the_probe_open_a_socket ... ok
test a_blocking_script_is_charged_cpu_time_not_wall_time ... ok
test a_form_body_reaches_the_front_controller ... ok
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

- Mutation control, each leg red when its rule is inverted (paste both runs):
  * drop the shim comparison in `php_view::open_pinned` → leg 1 fails;
  * pass `false` instead of `egress` in `probe_bindings` → leg 2 fails (`inet 1`);
  * make the shim return the request's wall clock as `cpu_ns` → leg 3 fails;
  * drop the body from `RequestV1` → leg 4 answers an empty `$_POST`.
- `cargo test -p blastit --locked --test critical_php_embed` is still green on a
  PHP-free host (`unshare -m -r` + `mount --bind /dev/null /usr/lib/libphp8.4.so`),
  and under `-- --test-threads=1` twice, leaving 0 directories behind.
- `make check` exits `0`; `uv run --no-project tools/critical_guard.py --base
  <parent sha> --node <this id>` reports the change as authorized.

# Notes

- Do not weaken an existing assertion or add `#[ignore]`; the four legs are additions
  (`critical-skip-check` refuses skip constructs in protected files).
- Leg 1's retry text must keep the "a removed inode pin produces the same served
  attempts" warning. Leg 2 must reuse `HANG_SHIM_STUB_C`'s shape file, not a new
  stub, so the `pid`/`pgid`/`uid` assertions stay in one place.
- `usleep` is not in `disable_functions`; keep the block at 200 ms and the bound at
  `wall_ns / 4` so a loaded CI host cannot flip it.
