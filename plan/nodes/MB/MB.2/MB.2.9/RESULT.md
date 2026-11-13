# Result MB.2.9

## Summary

The four public pages that still described the pre-`[route.static]` world now match the
shipped responder (`MB.2.3`/`MB.2.4`/`MB.2.5`) and the shipped `dev`/`check` wiring
(`MB.2.6`). One line per page, what contradicted the code and what it says now:

- `docs/security.md` — **contradiction:** no static-serving section at all, so the threat
  list the DX page links to (`"threat list in the security model"`) did not exist, and the
  scheduler row claimed it could not exceed an HTTP request. **Now:** one `## Static files`
  section — `400` for a malformed request (`..` after decoding, NUL/control, absolute,
  backslash, invalid UTF-8, a malformed escape, a >255-byte segment, over 4096 bytes),
  `404` for hidden existence (dotfiles, symlinks, FIFOs/sockets/devices,
  unreadable/oversize, and a miss on a route with no `spa` and no `app`), refusals final for
  every method, then `openat2(RESOLVE_BENEATH|RESOLVE_NO_SYMLINKS|RESOLVE_NO_MAGICLINKS)` +
  `(st_dev, st_ino)` identity, mounts followed, and a link to ADR-0007; the scheduler row
  gains the read beneath every declared `root`.
- `docs/how-it-works.md` — **contradiction:** the request narrative jumped from route
  match straight to `SPAWN`, as if every route were a view. **Now:** five lines after the
  diagram: head decode → refuse (`400`/`404`) → resolve (exact / directory `index` / SPA
  `index`) → serve, forward or `404`, reads on the blocking pool under one admission slot.
- `docs/ci.md` — **contradiction:** "It runs `pytest -q`" and "`blastit check` runs
  `pytest -q`" as unconditional facts, plus an exit-code table with no static root.
  **Now:** both sentences are qualified with "With an `app`", and a paragraph states the
  all-static exception: every `root` audited (exists, a directory, readable), no `pytest`,
  nothing on stdout, exit `0`/`2`; `--json` sets the counts at zero except `routes.total`
  (`Report::all_static`), and `2` names the root, plus the route whenever the filesystem
  refuses the root.
- `docs/reference/cli.md` — **contradiction:** the `check` `(none)` row and the exit-code
  paragraph implied pytest always runs. **Now:** the row says an all-static manifest audits
  every `root` and runs no `pytest`; `--learn` there is `2` ("nothing to learn"); the exit
  paragraph says an all-static manifest stops at `0` or `2` with nothing run.
- `docs/reference/manifest.md` — **contradiction:** the `app` row said only "without it
  every route must be a `[route.static]`", while `stage0.rs::app_rule` also refuses `app`
  (or `BLASTIT_APP`) **on** an all-static manifest by name. **Now:** "required when any
  route is not static", and the footnote states the boot error ("because no route could
  reach it").

No fence was added or edited on these five pages, so `python/tests/runnable_docs/inventory.toml`
needs no change and was left untouched (the harness still reports the same 47 fences).

## Review fixups (attempt 2)

Five accuracy findings, each corrected against the code:

| # | Page | Correction | Code it now matches |
|---|---|---|---|
| 1 (MAJOR) | `security.md:32` | scheduler "gets" cell names "every file beneath each declared static `root` — dotfiles and symlinks included"; its "cannot" half names the real boundary — "read the real database credentials; reach another route's socket or a view's memory; go beyond what its uid and a socket-less seccomp role allow" | `StaticRoot::open` runs as the scheduler's uid, which "can read exactly what that uid can read" (design §5.1: the manifest, the code tree, `/tmp`, and the fixture's `link -> /etc/passwd` beneath `root`), so the bound is `ADR-0007`'s uid + socket-less seccomp, not a path prefix. Attempt 2's "read outside those roots" is gone: it claimed a confinement the uid does not have |
| 2 | `security.md:103,107` | the miss is `404` only "on a route with no `spa` and no `app`"; existence sentence now "the `404`-vs-fallback distinction reveals…" | `rewrite::decide(outcome, …, has_app)` → `SpaFallback`/`Forward`, `SpaLost`/step 10 → `404` (design §4.3 rows 7–10) |
| 3 | `security.md:102` | `400` row gains a malformed percent escape and a segment over 255 bytes | `StaticRefusal::{Encoding, SegmentTooLong}` (`static_asset.rs`), matrix rows 12–13 |
| 4 | `security.md:115` | "A bind mount under `root` is followed: the boundary is the path namespace, not one filesystem" | `STATIC_RESOLVE_FLAGS` omits `RESOLVE_NO_XDEV` (design §5.1, `ADR-0007` decision 3) |
| 5 | `ci.md:14-18` | `--json` is "the same document, counts at zero except `routes.total`"; `2` "names the root, and the route whenever the filesystem refuses the root" | `audit_v1.rs::Report::all_static` sets `routes.total = route_count`, everything else `0`, `ok: true`; `static_root_refusal` (unreadable root) prints only the root, while `check_static_roots` names route and root for missing, not-a-directory and unlistable (`ManifestError::StaticRootUnusable`) |

Attempt 3 replaced the two over-corrections above (rows 1 and 5): the scheduler's
"cannot" half now states its uid/seccomp boundary instead of a filesystem prefix, and the
exit-`2` rule now covers every root the filesystem refuses, not only a missing one.

## Evidence

### (a) Every page consistent with the design — acceptance item 1

```console
$ grep -n 'static' docs/security.md docs/how-it-works.md | head
docs/security.md:32:| **the scheduler** | it can send any request to any view; every file beneath each declared static `root` — dotfiles and symlinks included | read the real database credentials; reach another route's socket or a view's memory; go beyond what its uid and a socket-less seccomp role allow |
docs/security.md:97:A `[route.static]` route serves files from one declared `root`, answered by the
docs/security.md:119:[ADR-0007](https://github.com/OffworldNexus/blastit/blob/develop/docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md)
docs/how-it-works.md:85:A `[route.static]` route answers without a view. The scheduler decodes the path
```

```console
$ grep -n 'app' docs/reference/manifest.md | head -5
13:| `app` | no* | the WSGI callable, `module:attribute`; required when any route is not static |
20:\* `BLASTIT_APP` may supply the callable instead, and it wins over `app`. An
21:all-static manifest needs no `app`: every route is a `[route.static]`. Setting
22:`app` or `BLASTIT_APP` there is a boot error, because no route could reach it.
26:app = "myshop.wsgi:application"     # Django; "app:app" for Flask
```

No unconditional `app | yes` remains: the `Required` cell is `no*`, and the footnote now
carries the whole rule. Checked against the code that decides it:

```console
$ grep -n 'fn app_rule' -A9 crates/blastit/src/stage0.rs
308:pub(crate) fn app_rule(config: &Config, manifest: &Manifest) -> Result<(), String> {
309-    if let Some(message) = static_app_refusal(config, manifest) {
310-        return Err(message);
311-    }
312-    if !manifest.is_all_static() && config.app.is_none() {
313-        return Err("BLASTIT_APP: the WSGI callable (`module:attribute`) is required".to_string());
314-    }
315-    Ok(())
316-}
317-
```

(`config.app` is `BLASTIT_APP` when set, else the manifest's `app` — `config.rs:974`.)

```console
$ grep -n 'pytest' docs/ci.md docs/reference/cli.md
docs/ci.md:9:With an `app`, it runs `pytest -q` with Blastit in front of PostgreSQL: every
docs/ci.md:15:directory, readable), runs no `pytest`, prints nothing and exits `0` — or exits
docs/ci.md:92:`--json` writes one document to stdout and nothing else: pytest output, logs and
docs/ci.md:98:`0` clean · `1` findings · `2` could not start (manifest, config, a static root, no `pytest`) · `3` suite failed.
docs/ci.md:102:With an `app`, `blastit check` runs `pytest -q`; without `pytest` it exits `2`
docs/ci.md:103:(`uv add --dev pytest`). With `pytest-django` importable **and**
docs/ci.md:107:Arguments after `--` go to pytest:
docs/reference/cli.md:47:| *(none)* | — | run `pytest -q`, then check every request and statement against the manifest; an all-static manifest audits every `root` and runs no `pytest` |
docs/reference/cli.md:51:| `-- ARGS…` | — | pass `ARGS` to pytest |
docs/reference/cli.md:61:failed (`pytest` exit `5`, no tests collected, is `3`). An all-static manifest
```

Every remaining `pytest` mention is either conditioned on `app` (`ci.md:9`, `ci.md:102`,
`cli.md:47` as the with-app first half) or is the all-static exception itself (`ci.md:15`,
`cli.md:61`). The all-static path matches the code:

```console
$ sed -n 161,171p crates/blastit/src/check.rs
    if manifest.is_all_static() {
        if options.learn {
            return Err("nothing to learn: every route is static".to_string());
        }
        // `--json` prints exactly one document; the human form prints nothing
        // at all, because every root was audited and nothing ran.
        if options.json {
            emit_report(&audit_v1::Report::all_static(&manifest), true)?;
        }
        return Ok(CLEAN);
    }
```

### (b) Security page: refused classes + ADR-0007 — acceptance item 3

```console
$ grep -cE 'traversal|symlink|NUL|dotfile' docs/security.md
5
$ grep -n 'ADR-0007' docs/security.md
119:[ADR-0007](https://github.com/OffworldNexus/blastit/blob/develop/docs/decisions/ADR-0007-static-asset-serving-and-rewriting.md)
```

The link is an absolute GitHub URL, like the page's existing `ARCHITECTURE.md` link, because
`/decisions/` is in `zensical.toml`'s `exclude_docs` and `--strict` would flag an
intra-site link to an unbuilt page.

### (c) Documentation gates — acceptance item 2

```console
$ make docs-unit
...
63 passed, 57 subtests passed in 0.88s
```

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.52s
```

```console
$ make docs-test
...
PASS  docs/reference/cli.md  fence 4  usage                 0.1s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked every command, flag and snippet (The e2e harnesses need Docker and root; the runnable-docs gate is unprivileged and Docker-free, so only the command names and the scripts they name are checked.)

47 fences: 28 executed, 19 accounted, 0 failed · 25.5s
```

Three runs of `make docs-test` across attempts 2 and 3 are clean (`27.7s`, `21.8s`, `25.5s`,
all `0 failed`). One earlier run, seconds after the `docs-check`/`docs-unit` pair, reported
`1 failed`; its FAIL line was cut by `tail -5` and not captured, and no re-run reproduced it —
most likely a port still held by the fence that had just run, not a page change. Flagged as a
Follow-up for the owner of the harness.

`inventory.toml` is unchanged (`git status -- python/tests/runnable_docs/inventory.toml` is
empty): no page gained or lost a `console` fence, and the harness's 47-fence count and every
index on the five pages are the same as before this node.

## Files

- `docs/security.md` — new `## Static files` section (refusal table with the malformed
  escape and 255-byte segment, the `spa`/`app` qualification on the miss, finality,
  `openat2`/inode identity, mounts followed, ADR-0007 link) and the scheduler's read
  beneath every declared `root` in "What a compromise gets".
- `docs/how-it-works.md` — five lines on the static path in "A request".
- `docs/ci.md` — all-static paragraph after the intro (root audit, no `pytest`, `0`/`2`,
  `routes.total` in `--json`); "With an `app`" on the two pytest sentences; static root in
  the exit-`2` list.
- `docs/reference/cli.md` — `check` `(none)` and `--learn` rows, and the exit-code
  paragraph.
- `docs/reference/manifest.md` — `app` row and footnote aligned with `stage0::app_rule`.
- `python/tests/runnable_docs/inventory.toml` — untouched (no new or changed fence).
- `plan/nodes/MB/MB.2/MB.2.9/RESULT.md` — this report.

## Follow-ups

- `docs/frameworks/static-and-spa.md` tells readers "Mechanism in How it works, threat list
  in the security model", and both now exist; no change needed there, but the DX page's
  `curl -i` fences stay `static`/`example` rather than executed — `MB.2.7`'s
  `e2e/static_assets.py` is the execution that closes them.
- One `make docs-test` run in this attempt reported `1 failed` and the next two were clean;
  the failing fence was not captured. If it recurs, the harness owner should name the fence
  in the summary line instead of only counting failures.
