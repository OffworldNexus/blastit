# Result MB.21.4.1

## Summary

Stopped before editing: the leaf's acceptance cannot be met without work its `paths` forbid and a
sibling owns. Flipping discovery to `./Blastfile` (acceptance 1/2/6) breaks every suite outside this
leaf's `paths` that boots the real binary against a v1 fixture under its default working directory;
switching the reader to `parse_blastfile` (acceptance 3's dev/check/boot legs) breaks them again, in
their bodies this time. Measured, not assumed: a one-constant experiment and two binary probes below.
No repository file was changed; `config.rs` was restored to base after the experiment.

The three other findings a replan must fold in are in `## Follow-ups`: the `error.rs` visibility gap
that makes "call `error::{escape_control, clipped}` from `cli.rs`" impossible inside this `paths` list,
the two conflicting missing-file sentences, and acceptance 3's exit code contradicting R19/§15.1.

## Evidence

### Acceptance 1/2/6 (discovery) — BLOCKED by sibling fixtures

`crates/blastit/tests/*` suites that boot the binary with a fixture written as `blastit.toml` and no
`BLASTIT_MANIFEST` would all fail the moment `config::manifest_path` defaults to `./Blastfile`. Measured
by temporarily changing the two defaults in `config.rs` (`Default::default` and `manifest_path`) and
running one suite owned by `MB.21.4.2`; the experiment also shows the pre-existing missing-file text:

```console
$ cargo test -p blastit --features python --test critical_static_cli \
    a_missing_static_root_is_named_with_its_route_before_any_output
running 1 test
test a_missing_static_root_is_named_with_its_route_before_any_output ... FAILED

thread '...' panicked at crates/blastit/tests/critical_static_cli.rs:368:9:
blastit.toml missing from: blastit dev: ./Blastfile: No such file or directory (os error 2)
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 11 filtered out
```

`config.rs` was then restored; `git status --short` shows only the dispatcher's `plan/` files.

Affected, all outside this node's `paths` and owned by `MB.21.4.2`/`MB.21.4.3`
(`crates/blastit/tests/**` is in 4.2's paths): `critical_dev_and_fail_closed.rs`,
`critical_static_cli.rs`, `critical_raw_ingress.rs`, `critical_php_backend.rs`, `critical_php_embed.rs`,
`no_python_build.rs`, `smoke.rs`, `dev_signal_reap.rs`, and the parts of `critical_check_engine.rs`
that drive the binary. `smoke.rs`/`dev_signal_reap.rs` set `BLASTIT_MANIFEST` explicitly, so the
*discovery* flip alone spares them, but the *parse* flip below does not.

### Acceptance 3 (foreign body) — dev/check/boot BLOCKED by the retired reader still being the reader

The one product surface that already parses `Blastfile` proves the sentence; the other three do not
reach the Blastfile parser at all so the same v1 body boots. Probe with
`./Blastfile` = `version = 1`, `app = "app:application"`, one `[[route]]`:

```console
$ target/debug/blastit fmt
./Blastfile:1:1: `version` is not a keyword; the sections are `app`, `entrypoint`, `postgres`,
trait`, `view`, `static`
fmt exit=2

$ BLASTIT_MEM_HIGH_WATERMARK=100 target/debug/blastit dev
  blastit dev · single-uid simulation · 127.0.0.1:8080
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
07:08:23.481 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
07:08:23.528 ERROR app.output  stderr  BLASTIT_APP (app:application): ModuleNotFoundError: No module named 'app'
dev exit=1
```

The dev boot accepted the v1 body as a v1 manifest (`1 route`) and failed later on the Python import,
never on syntax. `Config::from_env`'s `manifest_wiring`, `stage0::{run,run_dev}` and `check` still call
`Manifest::from_toml_str`/`parse_and_validate`; turning those into `parse_blastfile` is the parse
cutover, and it makes all eight suites above fail on their v1 bodies even with discovery fixed. That
fixture translation is `MB.21.4.2`'s declared work ("Suites: extend/replace fixtures in …"), so the
cutover and its fixtures must land in one leaf while this one stays discovery-only — or this leaf has
to own `crates/blastit/tests/**` too.

### Acceptance 4 — NOT RUN (the flip it measures is not landed)

```console
$ grep -rnE 'blastit[.]toml|TomlManifest|looks_like_toml|is_toml|strip_suffix\("\.toml"\)' crates/blastit/src | wc -l
31
$ grep -c 'BLASTIT_MANIFEST' crates/blastit/src/cli.rs crates/blastit/src/config.rs
crates/blastit/src/cli.rs:11
crates/blastit/src/config.rs:13
```

These are the 31 hits the leaf exists to remove; they remain because the discovery flip is the blocked
change.

### Acceptance 5 — NOT RUN

`make check` was not run: no buildable change was kept, and running it would only measure the base
tree. It is expected to be 0, as at `b4c8344`.

### Acceptance 7 — BLOCKED by a `paths` gap

`blastfile::fmt`'s re-parse is independent and safe to write, but "`cli.rs`'s `one_line_text` replaced
by `error::{escape_control, clipped}`" is not reachable from this node's `paths`:

```console
$ grep -n 'pub\|fn escape_control\|fn clipped' crates/blastit-manifest/src/error.rs | head -4
10:pub(crate) const MAX_QUOTED_CHARS: usize = 120;
13:pub(crate) fn escape_control(value: &str) -> String {
32:pub(crate) fn clipped(value: &str) -> String {
$ grep -n '^mod error\|^pub mod error\|pub use error' crates/blastit-manifest/src/lib.rs
mod error;
pub use error::{Location, ManifestError, ManifestErrors, ManifestWarning, Position};
```

Both helpers are `pub(crate)` in `blastit-manifest` and `mod error` is private, so `crates/blastit`'s
`cli.rs` cannot name them. Landing the requirement needs `crates/blastit-manifest/src/{error,lib}.rs`
in `paths` (make the two `pub`, re-export them) or a public wrapper in `print.rs` **plus**
`blastfile/mod.rs`'s `pub use` line. None of those three files is listed. Writing the re-parse half
alone would leave `one_line_text` in place and acceptance 7's second grep non-empty.

## Files

None changed. `crates/blastit/src/config.rs` was temporarily edited for the experiment and restored;
`git status --short` shows only the dispatcher's `plan/{BUDGET,JOURNAL}.md` and this node's
`LOG.md`/`NODE.md`.

## Follow-ups

### Blocker and proposed split (needs_split)

The cutover must be one leaf because discovery and parsing are one behaviour, and every suite that
boots a v1 fixture is in `crates/blastit/tests/**` — a path `MB.21.4.2` owns, not this node.

1. **`MB.21.4.1a` (new, lands first): translate every booting test fixture to `Blastfile`.** Paths:
   `crates/blastit/tests/**` (the eight files listed above) and `crates/blastit-manifest/tests/**`
   where a fixture feeds the binary. No model change, no assertion deleted; this is the fixture half
   `MB.21.4.2` currently carries.
2. **`MB.21.4.1` (re-scoped): discovery + parse cutover + fmt re-parse, green against 4.1a.**
   `config::manifest_path` `pub(crate)` as the only discovery; `fmt_path` deleted; the shared
   missing-file sentence at dev (fd 2), production stage 0 (`app.output` on fd 1) and `check`/`fmt`;
   `manifest_wiring`/`stage0::run`/`stage0::run_dev`/`check` parse through
   `Manifest::parse_blastfile`; `print::fmt` re-parses; `cli.rs`'s two helpers deduplicated.
   Needs `crates/blastit-manifest/src/{error,lib}.rs` added to `paths` (see acceptance 7 above).
3. **`MB.21.4.2` (narrower): §4.3 removals + the capability-map parity assertions only**, with the
   fixture translation already done by 4.1a.

If the lead prefers not to add a leaf, the alternative is to merge 4.1 into 4.2 and let 4.2's
`crates/blastit/{src,tests}/**` own the whole cutover.

### Brief defects to fix during the replan (do not silently resolve)

- **Two missing-file sentences.** The node's Context pins
  `missing ./Blastfile in the working directory (or set BLASTIT_MANIFEST)`; design §15.1 (called
  authoritative in the same paragraph) pins
  `./Blastfile: no such file or directory (put a Blastfile in the working directory, or set
  BLASTIT_MANIFEST)`. The accepted design text is the one a test can copy; the replanned brief should
  carry one of them verbatim.
- **Acceptance 3's exit code.** It says `1` for all four entry points; R19 and design §15.1 make `fmt`
  and `check` exit `2` on a parse/read failure (and the protected `critical_blastfile_fmt.rs`/
  `fmt_command.rs` already pin `2`). Boot/`dev` are `1`.
- **`--manifest` rewording** in `cli.rs` is safe and belongs with the discovery leaf; keep the refusal
  and name `Blastfile` plus `BLASTIT_MANIFEST` (three sites: `parse_dev`, `parse_check`, `parse_fmt`;
  the `--blastit-manifest` pytest error also names the file).
- **`check::read_manifest`** currently owns a second missing-file sentence; once discovery is shared it
  should render the one sentence and keep exit `2`.
- **`docs/knowledge/LESSONS.md`'s two-streams entry** still quotes the retired name and path in its
  example; it is outside this node's `paths` and the epic keeps that file for `MB.21.5.1`.
