# DX review — MB.23.1 live-reload docs

Scope: `docs/local.md` (160 lines), `docs/reference/cli.md` (125), `docs/reference/log-events.md` (93), `docs/troubleshooting.md` (93), judged against the epic's all-views model (one tracked change reloads every view; no per-route or single-view reload) and the frozen contract (`dev.reload` with `routes`/`files`/`reason` in `loaded|preload|blastfile|overflow`, `dev.reload.unavailable` with `errno`/`filesystem`, `reload` in the `process.retired` reasons). Cross-checked `docs/VISION.md` "The Guiding Principle" and the TL;DR plus "Developer Experience Implications" of `docs/research/2026-11-16-dev-fanotify-reload.md`; its per-route table was ignored as superseded. Ran `make docs-check` (green), `wc -l`, and greps for `inotify`, `BLASTIT_DEV_RELOAD`, `fanotify`, `blastit.toml`, `read once`, and single-view/per-route promises (all empty). Judged only what is on disk; `--no-reload` not yet wired is expected and not a finding. `docs/local.md` and `docs/reference/cli.md` are at their hard caps, so every in-path suggestion below is line-neutral.

## Findings

### F1 · MINOR (docs/reference/cli.md:28)

> | `--no-reload` | reload on | serve without live reload: nothing is watched, and the boot is silent about it |

"The boot is silent about it" has no clear referent and reads as if the reload-on boot normally says something about reload — it does not; only the unavailable warning prints. A developer in a container who was told (line 34) to expect "one warning at boot" cannot tell whether silence under `--no-reload` means "off as requested" or "the warning failed to print". Name the thing that is absent:

> | `--no-reload` | reload on | serve without live reload: nothing is watched, and the boot prints no warning about it |

### F2 · MINOR (docs/reference/cli.md:34)

> Reload is on by default and needs the host kernel to allow it: a container profile

The CLI reference never says reload is `dev`-only. A reader of this page — the one page that lists every flag — cannot answer "does production reload?" or "does `blastit check` take `--no-reload`?" without leaving it; the only production statement is in `log-events.md:46`, and `check`/`test` are stated nowhere. The flag table's placement implies it, so this is reinforcement, not a broken promise. One clause closes it without a new line:

> Reload is on by default in `dev` only, and needs the host kernel to allow it: a
> container profile that denies it gets one warning at boot and the server keeps serving.

Naming `check`/`test` explicitly would cost a third line and force a cut elsewhere on a page already at 125/125; "in `dev` only" already excludes them.

### F3 · MINOR (docs/reference/log-events.md:41)

> `dev.reload` is one reload of every view, never a subset: on any tracked change

The sibling event is called out as "the one `warn` at boot" (line 45), but `dev.reload` never states its level, even though line 16 documents `BLASTIT_LOG_LEVEL` as a filter. A developer who quiets a noisy `app.output` with `BLASTIT_LOG_LEVEL=warn` then saves a file, sees nothing, and concludes reload is broken. The frozen contract says `info`; say it:

> `dev.reload` is an `info` event: one reload of every view, never a subset. On any tracked change `routes` lists every view route replaced together,

This page is at 93 lines with no cap, so the paragraph may reflow freely.

### F4 · MINOR (out of MB.23.1's paths: docs/blastfile.md:41, docs/frameworks/python/index.md:26)

> It reads the file once — restart after an edit — binds `127.0.0.1:8080` (`--port`
> moves it), and logs each request and each decision.

> `uv add blastit`, then `uv run blastit dev` from the project root. The file is read once, so
> restart after an edit; `uv run blastit fmt` puts it in canonical layout.

Both public pages still teach the exact opposite of `docs/local.md:19` ("Edits reload your views, all together"). A developer who reads the Python connector or the "The Blastfile" tour and then saves a view will restart manually and conclude live reload is broken — the most expensive failure this documentation can cause. Neither file is in this node's `paths` and no MB.23.x node owns them, so this cannot gate MB.23.1; record it as a follow-up (a docs-sweep leaf task) with these replacements:

> It binds `127.0.0.1:8080` (`--port` moves it), logs each request and each decision,
> and reloads every view after a save; `--no-reload` turns that off.

> `uv add blastit`, then `uv run blastit dev` from the project root. Edits reload every view;
> `uv run blastit fmt` puts the file in canonical layout.

Both are line-neutral or shorter than what they replace.

## Verdict

All five reader questions are answered on disk: (a) reload exists and is on — `local.md:19` plus the `reload on` default in `cli.md:28`; (b) what a save does and never does — the `local.md:21-25` prose table (every view together; `:8080` up; no restart), with `log-events.md:41` "never a subset" and no per-view promise anywhere (grep empty); (c) how to turn it off — `local.md:32`, `cli.md:20` and `cli.md:28`, with `dev`-only implicit in the reference structure and explicit for production at `log-events.md:46`, tightened by F2; (d) the blocked-watcher warning — `cli.md:34-35` and `troubleshooting.md:28` both say the server keeps serving; (e) event and field names — `log-events.md:38-46` carries all five frozen field names, all four reason words and the `reload` retire reason, unrenamed. Reload behaviour is taught in prose tables, not console fences, and the four pages contain none of the banned tokens.

F1–F3 are optional polish (F3 is the one I would take if only one edit were made); F4 is a genuine contradiction but lies outside this node's paths, so it is a follow-up rather than a gate. No BLOCKER or MAJOR remains in the four pages, so a cheap implementer can wire `--no-reload` to these words without renegotiating the docs.

VERDICT: PASS
