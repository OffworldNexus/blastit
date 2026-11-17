# Runtime diagnostics: the `INFO runtime` line

- **Status:** Implemented (`MB.25.1` docs, `MB.25.2` code)
- **Contract of record:** [`docs/reference/log-events.md`](../reference/log-events.md)
  (`## The `runtime` line`) and [`docs/local.md`](../local.md) (`## Start`). Those two
  pages are frozen for this node; this document records how the code meets them, not
  what they say.
- **Owner of the event model:** `crates/blastit-events` (`Runtime`, `EventKind::Runtime`).

## The problem this closes

`0.2.0` greeted a PHP developer with a *warning* about the thing that had just worked:

```text
00:11:36.867 WARN manifest.warning the entrypoint php runtime is libphp /usr/lib/libphp8.5.so and shim .../libblastit_php85-x86_64-gnu.so (PHP 8.5, x86_64-gnu)
```

Three defects, one line: the level (`warn` is for something needing attention), the event
(`manifest.warning` is "the manifest is usable but has a problem"), and the content (it
named the internal SAPI shim and said nothing about the `php.ini` in force). A warning that
fires on success teaches people to ignore warnings.

## The shape, and why it is a closed enum

`Runtime` is an enum of three payloads:

| Variant | Fields |
|---|---|
| `Php(PhpRuntime)` | `version`, `libphp`, `php_ini: Option<String>` |
| `Python(PythonRuntime)` | `version`, `environment` |
| `Static` | none |

The frozen table has a per-entrypoint field list, so the *type* carries it: a `python` line
cannot grow a `libphp`, and a `static` line cannot carry a version. `EventKind::Runtime` is
`info` from `component: "init"`; the JSON encoder writes `php_ini: null` (present, not
absent) when PHP opened no file, and the pretty encoder writes `php.ini none`.

`MAX_EVENT_BYTES` and the 2048-byte truncation rules are unchanged. The runtime fields are
ordinary string slots, so a pathological path is cut on a character boundary and named in
`truncated` like every other field.

## Where each line is emitted, and why there

The line is emitted where its facts exist. That is not one place, and pretending otherwise
would mean inventing values.

### `entrypoint php` — the stage-1 probe

`crates/blastit/src/php_view.rs::probe_bindings`, right after the first probe child
succeeds and before `bind_tcp`.

The version and the ini are only true *after* `php_module_startup`, which by `ADR-0010` §3
happens below the fork (stage 0 must never `dlopen` what it inspects). So the probe child:

1. loads the pinned pair and runs `blastit_php_worker_init` (`php_module_startup`),
2. asks the shim for `php_version()` (the full `"8.4.27"`) and for the `php.ini` PHP
   opened,
3. writes one line — `blastit-probe runtime=["8.4.27","/etc/php/8.4/embed/php.ini"]` — on
   its **own** pipe, JSON-encoded so a path containing a newline cannot split the record,
4. shuts the module down and exits `0`.

The parent reads that pipe after reaping the child and emits exactly one `runtime` event for
the manifest (§12.2: one front controller, so every slot's report is the same and the first
one wins). The verdict pipe stays separate: PHP's startup log can be long, and the refusal
detail deliberately keeps only its head — a report at the tail of that buffer would be the
first thing lost.

**Cadence.** One line each time the entrypoint resolves: at boot in `blastit dev` and in
server mode, and again when a `blastit dev` reload cycle re-runs the boot half and re-probes
the pair. `--no-reload` and production have no reload, so they emit exactly one.

**What is never reported.** The shim path, the `(arch, libc)` tuple and the soname list are
internal; the line names the resolved `libphp` (public knowledge, and the thing an operator
checks against their package) and nothing else. `critical_php_backend`'s whole-boot scan
keeps that true for stdout *and* stderr.

### `entrypoint python` — stage 0, when the interpreter is live

`crates/blastit/src/stage0.rs::emit_runtime`, in both `run` and `run_dev`, once the boot is
known good and before the `execve`.

A wheel install (`blastit._blastit` called from the venv's interpreter) is the one install
where stage 0 already has CPython: `Python::try_attach` gives `sys.version_info()` and, for
the environment, `VIRTUAL_ENV` (the environment a developer named) else `sys.prefix`. That
is the earliest point the interpreter exists, and it is the build every published artifact
uses (`uv add blastit` → wheel).

`try_attach`, not `attach`: a claimed-but-absent interpreter answers "no line", never a
panic before the fork. The facts are read from the running interpreter only — a version
baked in at compile time would be a micro-version lie on the next patch release.

**Known limitation (follow-up).** A build that *embeds* CPython in stage 1 instead — a
`cargo` development build with `python` on — has no interpreter in stage 0, so it emits no
`runtime` line for a python manifest. The facts exist one stage later, next to the preload
`boot` line that `stage1.rs` owns; wiring it there belongs to the owner of that file
(`MB.24.x`), and this node may not edit it. Serving is unaffected — only the line is
missing.

### `entrypoint static` — stage 0

Same call site: no resolved `libphp` and no callable is static mounts and no language
runtime, including an `entrypoint php` that no view reaches (it loads nothing, and
`manifest.warning` already reports the unreached entrypoint). The line is
`static  serving static assets only`.

The classifier is one function, `runtime_shape(entrypoint_php, has_app, interpreter)`, so
the decision table is testable without a boot.

## `php_ini`: one additive C symbol, no ABI bump

`php_ini_opened_path` is PHP's own global (`main/php_ini.c`, exported by every 8.x
`libphp`), set by `php_init_config()`: the file it opened, or `NULL` when PHP runs on its
built-in defaults. It is **not** declared in 8.x's public `main/php_ini.h`, so the shim
declares the one symbol itself and returns it:

```c
extern char *php_ini_opened_path;
const char *blastit_php_ini_opened_path(void) { return php_ini_opened_path; }
```

`BLASTIT_PHP_ABI_VERSION` stays **1**: the accessor is optional and additive, exactly like
`blastit_php_file_sink_install`. `Loaded::ini_opened_path()` resolves it with `dlsym` on the
shim's own handle, so a **shim** built before the addition reports no path and serves exactly
as it did. That is the only degradation, and it is a property of the shim's age: the shim
itself refers to `php_ini_opened_path`, so a `libphp` that did not export that global would
fail the shim's `RTLD_NOW` load outright (loudly, naming the symbol) rather than reporting
nothing — every 8.x `libphp` exports it, so no supported host is affected. The method answers
`None` for four cases and never guesses: the shim exports no such symbol, the worker has not
run `worker_init`, PHP returned `NULL`, or the path is not valid UTF-8.

No caller re-derives the path. `PHPRC` and `PHP_INI_SCAN_DIR` are stripped from the view
environment and the SAPI sets `php_ini_ignore_cwd = 1`, so the ini PHP opens is a property
of the host's PHP build — and the only honest answer is the one PHP itself recorded.

## The `warn` contract this restores

`runtime` is never `warn`. After this node `manifest.warning` again means what
`docs/reference/log-events.md` says: `unrestricted`, `egress any`, an ungranted store, a
`raw` body (`blastit dev`: both shapes; production: the acknowledged one), an unused
`trait`, an `entrypoint php` no view reaches. Nothing about discovery, resolution or refusal
changed: a host without a usable `libphp` still fails closed with §4.1's package sentence and
emits **no** runtime line — the line follows a successful probe, so every refusal above it
is silent about the runtime.

## Proof

| Test | What it pins |
|---|---|
| `crates/blastit-events` unit tests | all three shapes encode/decode round-trip, `php_ini: null` is present, an unknown entrypoint is undecodable, the long-path cut stays under 2048, and the three `docs/local.md` lines byte-for-byte |
| `crates/blastit-php-sapi/tests/critical_shim_contract.rs` | the shim reads PHP's own global and returns it, the header declares the accessor, and ABI version 1 is unchanged |
| `crates/blastit/tests/critical_php_backend.rs` | the whole-boot scan: one `info` `runtime` line naming exactly the library the independent probe found, the version's minor agrees, `php_ini` is a path or `null`, no `manifest.warning` and **no shim path** anywhere in normal output; refusals emit no runtime line |
| `crates/blastit/tests/critical_php_embed.rs` | the served pair reports the library and never the shim; every refusal (`not a library`, ZTS, unsupported minor, inode swap, shim minor, hang) emits no runtime line |
