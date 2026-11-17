# MB.23.1 — live-reload DX docs (local, dev flags, log events, one troubleshooting row)

## Summary

The five Goal edits landed in four public pages, plus a widened-scope sweep of the two pages
that still taught the retired behaviour. Both line budgets were tightened rather than
raised: `docs/local.md` 165 → 160 and `docs/reference/cli.md` 124 → 125 (cap 125).

- `docs/local.md`: the retired "The file is read once. Restart after editing a view or a
  grant." sentence is replaced by "Edits reload your views, all together. `--no-reload`
  turns that off." plus a 3-row prose table (any loaded file — even an editable package or
  template outside the project — reloads every view together; `./Blastfile` or a `preload`
  module re-reads and reloads the whole topology; `:8080` never drops, no restart, no
  second terminal). A `--no-reload` row says reload is on by default and nothing is
  watched when it is off. Compression elsewhere (merged intro, tighter learn/denial/editor
  wording, one merged wrap) paid for it; the one-uid "not a sandbox" warning, the
  throwaway-database advice and every editor step are intact. The learn-mode promise keeps
  its "allows": "Learn mode allows what the file does not cover and records it".
- `docs/reference/cli.md`: `$ blastit dev --no-reload` is a `$ ` line inside the existing
  `## blastit dev` fence (index 1, mode `dev-flags`, fixture `hello`), so its index is
  unchanged and `inventory.toml` needs no edit. The flag table gains `--no-reload` with
  default `reload on`, and the prose says reload is on **in `dev` only**, needs the host to
  allow it, and that a denying container profile gets one boot warning while serving
  continues.
- `docs/reference/log-events.md`: `reload` joins the `process.retired` reasons; `dev.reload`
  (info; `routes`, `files`, `reason` ∈ `loaded|preload|blastfile|overflow`) and
  `dev.reload.unavailable` (warn; `errno`, `filesystem`) join the table. The paragraph pins
  `routes` as every route serving after the reload (never a subset), `files` as absolute
  paths, the two levels, the field value domain, and that check/test never watch and
  production has no reload.
- `docs/troubleshooting.md`: one row for `reload disabled` … `EPERM`, tagged with
  `dev.reload.unavailable`: the three causes, that serving continues, the host bind mount
  for a filesystem without handles, and `blastit dev --no-reload` as the silencer. It lives
  in its own `## A boot warning` table, because the server starts and serves.
- Widened scope: `docs/blastfile.md` and `docs/frameworks/python/index.md` no longer say
  "read once — restart after an edit". Each gains the same one-clause truth ("edits reload
  your views, all together — `--no-reload` turns that off") with the rest of its sentence —
  the bind/`--port`/log clause, and the `fmt`/canonical-layout remark — unchanged. Prose
  only: no fence, no heading, no page structure changed. The stale "0.1.1 page, rewritten in
  this release" trivia is gone from both places on `docs/blastfile.md`.
- Two fresh-context critics (`dx`, `ai-ergonomics`) each wrote their review and ended
  `VERDICT: PASS` with no BLOCKER and no MAJOR. All in-path MINORs were fixed line-neutrally,
  including dx F4, the "read once" contradiction above.

`make docs-test` was deliberately not run: the new `$ blastit dev --no-reload` fence line
cannot execute until MB.23.5 wires the flag. MB.23.6 runs it.

## Evidence

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.43s

$ make docs-unit
107 passed, 65 subtests passed in 1.60s

$ UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
35 passed in 0.59s

$ grep -rniE "inotify|BLASTIT_DEV_RELOAD" docs/local.md docs/reference/cli.md docs/reference/log-events.md docs/troubleshooting.md
(no output, rc=1)

$ grep -rni "read once" docs/local.md
(no output, rc=1)

$ grep -rniE "only (that|this) (view|route)|each of those routes|reloads exactly" docs/local.md docs/reference/cli.md docs/reference/log-events.md docs/troubleshooting.md
(no output, rc=1)

$ grep -rni "read once" docs/ README.md --include=*.md   # excluding design/decisions/research/testing/knowledge
(no output, rc=1 — no public page teaches the retired behaviour)

$ grep -rniE "fanotify|blastit\.toml" <the four pages>
(no output, rc=1)

$ wc -l docs/local.md docs/reference/cli.md
  160 docs/local.md
  125 docs/reference/cli.md

$ head -5 docs/design/dev-reload.review-dx.md
# DX review — MB.23.1 live-reload docs

Scope: `docs/local.md` (160 lines), `docs/reference/cli.md` (125), `docs/reference/log-events.md` (93), `docs/troubleshooting.md` (93), judged against the epic's all-views model (one tracked change reloads every view; no per-route or single-view reload) and the frozen contract (`dev.reload` with `routes`/`files`/`reason` in `loaded|preload|blastfile|overflow`, `dev.reload.unavailable` with `errno`/`filesystem`, `reload` in the `process.retired` reasons).

$ tail -2 docs/design/dev-reload.review-dx.md
VERDICT: PASS

$ tail -2 docs/design/dev-reload.review-ai-ergonomics.md
VERDICT: PASS

$ uv run --no-project tools/plan.py validate
plan OK
```

Both review files carry 0 BLOCKER and 0 MAJOR (dx: 4 MINOR, ai-ergonomics: 5 MINOR). They
judged the pre-fix text; every MINOR was then fixed, including dx F4 once its two pages
entered this node's `paths`. No critic re-run was needed because neither verdict was FAIL
and the widened-scope prose states the same model.

## Files

- `docs/local.md` (160 lines)
- `docs/reference/cli.md` (125 lines)
- `docs/reference/log-events.md` (96 lines)
- `docs/troubleshooting.md` (98 lines — one new `## A boot warning` table, one row)
- `docs/blastfile.md` (98 lines — prose only, 6-line paragraph, fence count unchanged)
- `docs/frameworks/python/index.md` (66 lines — prose only, fence count unchanged)
- `docs/design/dev-reload.review-dx.md` (critic, VERDICT: PASS)
- `docs/design/dev-reload.review-ai-ergonomics.md` (critic, VERDICT: PASS)
- `python/tests/runnable_docs/inventory.toml` — untouched; the `blastit dev` fence kept
  index 1, so no entry changed

## Review fixes

The goldfish review passed with four MINORs; all four are applied:

1. `docs/local.md:32` no longer reads as self-contradicting: "turns reload off (it is on by
   default); nothing is watched".
2. `docs/local.md:35-37` restores "never production data or an `unrestricted` route" inside
   the same three lines.
3. `docs/troubleshooting.md` moves the `reload disabled` / `dev.reload.unavailable` row out
   of `## It does not start` into a one-row `## A boot warning` table, because the server
   starts and serves (93 → 98 lines, no duplication).
4. `docs/blastfile.md` drops the stale "(0.1.1 page, rewritten in this release)" trivia at
   both places, line-neutral.

## Follow-ups

1. **`make docs-test` is expected red on the new fence line** until MB.23.5 wires
   `--no-reload`; MB.23.6 owns that run. The line must not be removed or stubbed.
2. **Frozen contract hand-off to MB.23.2/MB.23.5:** `dev.reload` (`info`) with `routes`,
   `files`, `reason` ∈ `loaded|preload|blastfile|overflow`; `dev.reload.unavailable`
   (`warn`) with `errno`, `filesystem`; `reload` added to `process.retired` reasons. The
   docs now also commit to `routes` = every route serving after the reload, `files` =
   absolute paths, JSON `errno` = symbolic name (`"EPERM"`), `filesystem` = filesystem type
   (`"overlayfs"`). Design and implementation follow these, not the reverse.
3. **Boot-line wording:** the troubleshooting row quotes the stable words `reload disabled`
   and `EPERM` rather than the mechanism name, which is banned on public pages. MB.23.5's
   human-readable line must keep those two words visible so the row stays findable.
4. **Verified, not changed:** `uv run blastit fmt` exists
   (`crates/blastit/src/cli.rs`, `Some("fmt") => Self::Fmt`), so the canonical-layout remark
   on `docs/frameworks/python/index.md` is correct and was left as it is.
