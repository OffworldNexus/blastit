# MB.40.9 — PHP live reload repinning, banner names, static log target, gated dbproxy line

### 1b. One shared boot path for the watcher (review round)
`serve.rs` now owns `boot_and_seed_reload(emitter, &serving, between)` — watcher
construction, PHP boot-file seeding, the marking pass, the boot line, the §4.6
step 7 `reconcile` and the dead-watcher drop — plus its private `settle_reload`.
`serve()` passes the `preload` import as the `between` step (so the files it opens
are reported and marked); `serve_without_interpreter` passes a no-op. The two boot
paths no longer duplicate a line of it.

## What changed

### 1. `entrypoint php` repinning (`crates/blastit-view-php`, `crates/blastit`)
- `PhpBinding` now carries `declared_entry` (the manifest's own `entrypoint php`
  spelling). `resolve_entry()` is the one resolution both the boot and a reload
  use: it joins the declared path under `manifest_dir`, refuses `..`/outside
  lexically, then canonicalises the root **and** the entry and refuses a symlink
  whose target resolves outside the root (a symlinked deploy that stays inside
  still works). `boot_bindings` uses it, so boot and reload cannot disagree.
- `PhpBinding::repin_entry(&mut self, manifest_dir, declared_entry)` re-resolves,
  re-pins and atomically replaces `entry`/`entry_pin`/`entry_fd`.
  `PhpBinding::entry_is_current()` is the read-only "did a save land?" question.
- `Supervisor::reload_all` re-pins **every** PHP slot before `Ctl::Reload`
  (`repin_php_bindings`, php-gated); a refusal is one `manifest.warning` and the
  previous pin stays. `Supervisor::spawn_now` re-pins a PHP slot when
  `dev.is_some() && dev_reload` and the file no longer matches the pin, so a save
  that beat the `fanotify` debounce (or the boot's first request) is served.
- Boot watch seeding: `Policy::track_boot_files` / `ReportTable::track_boot_files`
  seed the union. **Both** stage-1 boot paths call it with every PHP slot's
  `entry` + `document_root` before the marking pass:
  - `stage1/serve.rs` (the `python`/wheel path), and
  - `stage1/boot.rs::serve_without_interpreter` (the standalone `php,static`
    artifact — the path `make rs-test-php` actually exercises). That path also
    boots the watcher and runs the marking pass + `reload` event + §4.6 step 7
    `reconcile`, and now passes `dev_reload: serving.dev_reload` (never `false`).
  `boot_reload`, `announce_reload`, `BootLine`/`after_preload_marks` and
  `FAULT_OPEN` are no longer `python`-gated, so a `php`-only build gets the full
  `fanotify` live-reload ladder.

### 2. Banner (`crates/blastit/src/dev_learn.rs`)
One aligned row per served thing, computed from the widest cell:
`  view    home     GET, HEAD  /` plus `  static  /assets  GET, HEAD  public/assets`
per mount. Route count still counts views only (a mount is not a route, §9.5).
Widths are counted in **characters** (`.chars().count()`), the unit `format!`
pads a `str` by, so a non-ASCII prefix cannot shift a later column.

**Docs sync (review round):** `README.md`, `docs/getting-started.md` and
`docs/local.md` now show the view row too, and
`python/tests/runnable_docs/fixtures/hello/Blastfile` was aligned to
`view hello GET "/*" {}` so the fixture is what the pages say it is. The row
prints `  view  hello  GET, HEAD  /.*`: the banner names the route's **lowered**
paths, which is what `blastit check` and the `rule` field report (`"/*"` lowers to
`/.*`). `docs/frameworks/static-and-spa.md` was left alone: its fence is replayed
by `e2e/static_assets.py` against a container image built from a **released**
wheel, so documenting a row that older image does not print would break CI `e2e`.

### 2b. `docs/reference/log-events.md` (review round)
The `request` row now says `route` is "the view's name; `static` when a static
mount answered; null when neither did", and one `pretty` example line is shown in
a `console` fence with its own inventory entry (`static` mode: the clock comes
from a real run, so the gate checks the shape and the page's JSON example).

### 3. Static request-log target (`crates/blastit-scheduler/src/scheduler/response.rs`)
`emit_request` tags `route: "static"` when a mount answered (new
`served_by_static`); a request neither a mount nor a route claimed keeps `None`
and still renders `(unmatched)`. `dx-parity.txt`/`dx-parity.expected` are a
recorded capture and stay byte-identical.

### 4. `dbproxy` phase line (`crates/blastit/src/stage0/fork_exec.rs`)
`wait_ready_with_line` gives the ready byte 200 ms (`SLOW_READY_LINE`) with no
output; only a slower wait prints `dbproxy  waiting for the ready byte` and then
waits out the rest of `DBPROXY_READY_TIMEOUT`. `wait_ready` stays for the stage-0
unit tests (`#[cfg(test)]`).

## Tests
- `crates/blastit-view-php`: `a_repin_records_the_saved_entry_and_refuses_one_that_left_the_root`.
- `crates/blastit-scheduler/tests/mount_precedence.rs`: `a_static_answer_names_the_mount_and_an_untouched_path_names_nothing`.
- `crates/blastit/src/dev_learn.rs`: banner tests updated + `the_banner_names_static_mounts_in_the_same_columns`.
- `crates/blastit/tests/critical_dev_and_fail_closed.rs`: view-table assertions updated to the named columns.
- `crates/blastit/tests/critical_php_embed.rs`:
  - the two boot-pin immutability legs now pass `--no-reload` (`Boot::start_without_reload`), matching production;
  - new real-pair leg `a_dev_boot_reloads_a_php_save_in_place_by_rename_and_through_an_include`:
    v1 boot → in-place v2 → atomic-rename v3 → `require_once`d helper h4, each a
    `dev.reload` + retired worker + `200`, **zero `5xx`**, then a symlink out of
    `manifest_dir` emits `manifest.warning` and answers `5xx` without running the
    outside file.

## Follow-ups
- The banner prints a route's **lowered** paths (`"/*"` → `/.*`,
  `"/products/{id:int}"` → its compiled regex), because `Route::path()` is the
  compiled list `blastit check` reports as `rule`. Printing the spelling the
  `Blastfile` used would be friendlier DX, and needs the manifest model to keep
  the written pattern — `crates/blastit-manifest`, outside this node's paths.
- `docs/frameworks/static-and-spa.md` still documents only the three opening
  banner lines: `e2e/static_assets.py` replays that fence against a container
  image built from a **released** wheel, so a `static` row added before that
  release ships would fail CI `e2e`. Add it with the release that carries it.

## Evidence

```
$ make rs-test-php
test a_dev_boot_reloads_a_php_save_in_place_by_rename_and_through_an_include ... ok
test result: ok. 29 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
- (blastit-php-sapi) test result: ok. 8 passed; 0 failed; 0 ignored
one `blastit dev` boot served v1, then v2 (in place), v3 (atomic rename), v3-h4
(include) with zero `5xx`, and refused a symlink out of the tree: HTTP/1.1 500 Internal Server Error

$ make rs-no-python
test result: ok. 289 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
test result: ok. 3 passed; 0 failed   (no_python_build)
default build: 0 pyo3 dependencies (no libpython linked)

$ make fast-check
uvx --offline ruff check python tools
All checks passed!
uv run --no-project tools/plan.py validate
plan OK

$ make docs-check docs-test
PASS  README.md  fence 2  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/local.md  fence 1  dev                   0.1s  served on 127.0.0.1:8080
PASS  docs/reference/log-events.md  fence 1  static  0.0s  statically checked …
49 fences: 30 executed, 19 accounted, 0 failed · 26.7s
```
