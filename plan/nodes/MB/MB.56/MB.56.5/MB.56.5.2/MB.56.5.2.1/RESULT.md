# Result: MB.56.5.2.1 — diary-comment strip, every crate except `crates/blastit`

## Status: done

## What changed

- Removed every plan-node citation (`MB.x.y`) from `crates/{blastit-scheduler,blastit-view,blastit-view-static,blastit-view-python,blastit-view-php,blastit-frame,blastit-sys,blastit-py-shim,blastit-py-sapi,blastit-php-sapi,blastit-sqlpolicy,blastit-manifest,blastit-lsp,blastit-dbproxy,blastit-env,blastit-events,blastit-ingress,blastit-http}/src/**/*.rs`: 38 files, ~90 sites. Sentences whose subject *was* the node id were rewritten in the present tense (e.g. ``Nothing here lowers into a `Manifest`: `lower` does that.``), and the punctuation left behind by the deletions was repaired.
- Finished the half-done `blastit-scheduler` (`src/lib.rs`, `src/error.rs`, `scheduler/admission.rs`) and repaired the artifact sweep there (`scheduler/mod.rs`, `scheduler/response.rs`, `scheduler/views.rs`).
- Compressed remaining history to present-tense fact instead of deleting it: `php-sapi/src/loader.rs` (why the sink is boxed and owned), `view-python/src/serve.rs` (why threads are reused), `view-php/src/worker.rs` (no runtime seam around the pin), `scheduler/src/scheduler/response.rs` (why a static answer names its route), `blastit-events/src/lib.rs` (why the reload boot line is an event), `view-python/src/zygote.rs` (the child sequence order).
- Stale references fixed, each naming its real owner:
  - `crates/blastit-view-php/src/pinning.rs` — `scripts/check-no-test-seam.sh` → `scripts/build-standalone.sh` (the script that measures the packaged binary).
  - `crates/blastit-scheduler/src/scheduler/{views,response}.rs` (3 sites) — nonexistent `crates/blastit-view/src/php.rs` → `crates/blastit-view-php/src/worker.rs` (`recycle`/`should_recycle`, the code that actually retires a bailed worker).
  - `crates/blastit-sys/src/child.rs`, `crates/blastit-sys/src/lib.rs`, `crates/blastit-view-python/src/zygote.rs` — the retired `crates/blastit-sys/tests/critical_child_sequence.rs` claim replaced by the real state: `child::enter`'s callers are `view-python`'s `zygote` and `view-php`'s `probe`, and the order is documented (and owned) by `blastit_sys::child`.
  - `crates/blastit-manifest/src/blastfile/lexer.rs` — retired `tests/critical_blastfile_lexer.rs` claim dropped; the cap behaviour is stated directly.
  - `crates/blastit-php-sapi/src/{abi,loader,request}.rs` — retired `tests/critical_shim_contract.rs` → `c/include/blastit_php_abi_v1.h`'s `_Static_assert`s, `make php-sapi-e2e` (real shim load) and `e2e/php2shell.py`'s adversarial `PHP_VALUE` rows.
  - `crates/blastit-lsp/src/lib.rs` — `crates/blastit/tests/critical_lsp.rs` → `crates/blastit-lsp/tests/e2e_lsp.rs` (`a_poisoned_environment_changes_nothing`).
  - `crates/blastit-view-static/src/static_asset.rs` — retired `critical_static_resolve.rs` claim replaced by "a consumer" for the exported `STATIC_RESOLVE_FLAGS`.
- Kept every `// SAFETY:` block, every trap note (`st_ctime` tick, `Frame` Debug leak, `EG(included_files)` bounds, boxed sink address) and every 1–3 line public Rustdoc.

## Verification

```
$ rg 'MB\.[0-9]+(\.[0-9]+)+' crates/{blastit-scheduler,blastit-view*,blastit-frame,blastit-sys,blastit-py-shim,blastit-py-sapi,blastit-php-sapi,blastit-sqlpolicy,blastit-manifest,blastit-lsp,blastit-dbproxy,blastit-env,blastit-events,blastit-ingress,blastit-http}/src
(no output)

$ make fast-check
...
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

`make fast-check` is green (`guard`, `rs-fmt`, `rs-lint` with `-D warnings`, `rs-doc`, `rs-test`, `py-lint`,
`plan-validate`, `universal-smoke`) with zero compiler or clippy warnings.

## Follow-ups

- `crates/blastit` (plan-node citations there: `src/stage0/gates.rs` 15, `src/supervise.rs` 14, `src/stage1/boot.rs` 9, `src/lib.rs` 8, …) is the next slice's scope, not this node's.
- Several `critical_*.rs` suites no longer exist in the tree (`critical_static_resolve`, `critical_child_sequence`, `critical_shim_contract`, `critical_blastfile_lexer`, `critical_lsp`); the behaviour they pinned is now covered by `e2e/php2shell.py`, `make php-sapi-e2e`, `crates/blastit-lsp/tests/e2e_lsp.rs` and the `crates/blastit/tests/e2e_*` suite. If the static-resolve flag oracle should return, it needs a real suite under `crates/blastit-view-static/tests/`.
