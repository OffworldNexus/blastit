# AI-ergonomics review — MB.23.1 live-reload docs

**Scope.** `docs/local.md` (160/160), `docs/reference/cli.md` (125/125), `docs/reference/log-events.md` (93), `docs/troubleshooting.md` (93); judged against the all-views reload model and the frozen `dev.reload` / `dev.reload.unavailable` contract. Ran `wc -l`, `grep` (banned vocabulary, every `reload` mention, `routes`/`files`/`topology` wording), and `make docs-check` (passed: `zensical build --strict`, "No issues found"). Did not run `make docs-test`; the absent `--no-reload` wiring is out of scope. Every recommendation below keeps `local.md` at 160 and `cli.md` at 125 source lines — no cut is required anywhere.

## Findings

### F1 · MINOR (docs/troubleshooting.md:28)

```text
| `reload disabled` … `EPERM` | live reload is not permitted here: a container security profile, an older kernel, or a filesystem without file handles. The server serves either way; `blastit dev --no-reload` silences it |
```

**Defect.** The row teaches an agent only how to *silence* the warning. It never names `dev.reload.unavailable`, so an agent that greps the event contract cannot connect the JSON event to this remedy, and for the "filesystem without file handles" cause the research note's one-line remedy (a host bind mount) is missing. An agent running `blastit dev` in a container therefore concludes reload is unusable and turns it off, instead of restoring it and exercising the documented all-views behaviour.

**Replacement** (one source line; 93 → 93, no cut):

```markdown
| `dev.reload.unavailable` (`reload disabled` … `EPERM`) | live reload is not permitted here: a container security profile, an older kernel, or a filesystem without file handles. The server serves either way; if the filesystem has no handles, bind-mount the project from the host; `blastit dev --no-reload` silences it |
```

### F2 · MINOR (docs/reference/log-events.md:42)

```text
`routes` lists every view route replaced together, `files` the paths that triggered
it, and `reason` is `loaded` (a file the app loaded), `preload` (a `preload`
module), `blastfile` (the `Blastfile`) or `overflow` (changes were dropped, so it
reloads all).
```

**Defect.** The "never a subset" statement is correct, but the two field values are not assertion-complete. On a `blastfile` reload the route set can gain, lose or rename views, so "every view route replaced together" does not say whether `routes` is the *old* set being retired or the set *serving after* the reload — an agent asserting `set(routes) == views(Blastfile)` after adding a view can guess wrong. `files` likewise does not say whether paths are absolute or project-relative, so an exact match on the saved path is a guess.

**Replacement** (no hard budget on this page; paragraph grows by 0 lines when reflowed):

```markdown
`routes` lists every route serving after the reload, `files` the absolute paths that triggered it, and `reason` is `loaded` (a file the app loaded), …
```

(If the implementation prefers project-relative paths, state that instead; the defect is the unstated basis, and `absolute` matches the only path example on these pages, `local.md:81`.)

### F3 · MINOR (docs/reference/log-events.md:45–46)

```text
`dev.reload.unavailable` is the one `warn` at boot, with `errno` and
`filesystem`; `--no-reload` emits neither, and production has no reload at all.
```

**Defect.** The facts the lens requires are only inferable: (a) `dev.reload` is `info` — the page never says it, so a `level` assertion or a `BLASTIT_LOG_LEVEL` filter is a guess; (b) `errno` and `filesystem` have no value domain — the human line shows `EPERM`, so is JSON `errno` the string `"EPERM"` or `1`, and is `filesystem` a type or a path? (c) the page says the server serves but not *why* (watching is a dev convenience, off the request path). Check/test are also unnamed (fact 8), although the `check` flag table and the `blastit test` alias let an agent infer them.

**Replacement** (log-events has no hard budget; 93 → 96, no cut):

```markdown
`dev.reload` is `info`; `dev.reload.unavailable` is the one `warn` at boot: watching
is a dev convenience, so the server serves without it; `errno` is the symbolic name
(`"EPERM"`), `filesystem` the filesystem type (`"overlayfs"`). `--no-reload` emits
neither, `blastit check` and `blastit test` never watch, and production has no reload
at all.
```

### F4 · MINOR (docs/reference/cli.md:28)

```text
| `--no-reload` | reload on | serve without live reload: nothing is watched, and the boot is silent about it |
```

**Defect.** The `Default` column says "reload on" for a negated flag. A human reads the intended meaning, but an agent reading the table as `flag -> default` can invert it to "`--no-reload` is on by default" (i.e. reload off), the opposite of the model. The description's "it" also has no antecedent, so the page does not say the `dev.reload.unavailable` warning is what gets silenced; only `log-events.md:46` makes that exact.

**Replacement** (one source line; 125 → 125, no cut):

```markdown
| `--no-reload` | off | turn live reload off: nothing is watched, nothing reloads, and no `dev.reload.unavailable` warning is printed |
```

### F5 · MINOR (docs/local.md:24)

```text
| `./Blastfile` or a `preload` module | re-reads it, then reloads the whole topology |
```

**Defect.** `preload` is unresolvable from these four pages: `log-events` uses `preload` for the unrelated boot event (`build`, `immortalize`), so an agent can carry the wrong referent into the reload model. And "it" attaches to `./Blastfile`, so the preload half literally reads "re-reads the Blastfile". The agent can repeat fact 3 but cannot tell which file is a preload module (an `entrypoint` block item), so it cannot exercise that half of the table.

**Replacement** (one source line; 160 → 160, no cut):

```markdown
| `./Blastfile` or a [`preload`](reference/blastfile.md#sections) module | re-reads what changed, then reloads the whole topology |
```

## Verdict

All eight model facts are derivable from the four pages as written. Reload is on by default (`local.md:32`, `cli.md:34`); one tracked change replaces every view together (`local.md:19`, `local.md:23`, `log-events.md:41`); a `Blastfile`/`preload` change re-reads and rebuilds the whole topology (`local.md:24`, `log-events.md:43–44`); `:8080` never drops (`local.md:25`); `--no-reload` is the one dev-only off switch, and `log-events.md:46` says exactly which events it suppresses; the event names, fields and the four `reason` words are fixed and consistent; the unavailable warning is a boot-time `warn` while serving continues (`cli.md:34–35`, `troubleshooting.md:28`); production has no reload, and check/test follow from the alias and the exhaustive per-command flag tables. The superseded per-route model does not leak, the reload behaviour is a prose table rather than a console-fence screenshot, and the banned vocabulary is absent.

F1–F5 are all optional precision/actionability, not model defects: F1 connects the human symptom to the event and restores the container remedy; F2 pins the post-reload route set and the path basis so completeness assertions are exact; F3 pins the info/warn levels, the field value domain, the survival reason, and names check/test; F4 removes the table-only inversion and the dangling "it"; F5 resolves `preload` and the antecedent. None renames a frozen event, field or `reason` word, and none needs a line cut. With them applied the pages are assertion-tight; without them an agent still reaches the correct behaviour and only has to discover field representations at runtime.

VERDICT: PASS
