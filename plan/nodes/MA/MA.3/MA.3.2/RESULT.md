# RESULT — MA.3.2 docs / README / ARCHITECTURE drift (attempt 2)

STATUS: done

Attempt 1's reconciliation stands (the reviewer verified every product claim);
this attempt fixes exactly the five precision findings and the one leanness
finding from the REVIEW FAIL (1/3). Docs prose only: no `crates/`, no
`python/tests/runnable_docs/**`, no `zensical.toml`, no plan mutation.

## What changed (finding → fix)

| # | Finding | File(s) | Now says |
|---|---|---|---|
| 1 | [BLOCKER] the watermark is a share of *total*, not of non-reclaimable memory | `docs/reference/settings.md:112`, `docs/deploy/anything-else.md:56-59` | `85%` **of `total`**, the smaller of `MemTotal` and the cgroup limit; **the non-reclaimable reading** (page cache and reclaimable slab left out) is what the percentage is compared against. Matches `stage1.rs:688` / `mem_usage.rs:215` (`watermark = total × percent / 100`), compared with `usage.used()` (`mem_usage.rs:100-102`), and `mem_usage.rs:124-160` for what `used` leaves out |
| 2 | [MINOR] `dev` is not loopback-only | `ARCHITECTURE.md:75` | "loopback by default; a non-loopback bind needs `--host` and `BLASTIT_DEV_EXPOSE=1`, and the banner then says `EXPOSED, no auth`" (`dev_learn.rs:73`, `cli.rs:154`) |
| 3 | [MINOR] plugin skip conditions incomplete | `ARCHITECTURE.md:1066-1068` | skips when there is no `--blastit-manifest`, no WSGI callable, a postgresql proxy that does not take its URL from one variable, **or no `django_db_setup` fixture** (the four `pytest.skip` sites: `pytest_plugin.py:1817, 1824, 2004, 2015`) |
| 4 | [MINOR] `memory.stat` fallback keeps slab | `ARCHITECTURE.md:1369-1372` | falls back to ``inactive_file + slab_reclaimable`` — `parse_reclaimable` adds `slab` in both branches (`mem_usage.rs:124-160`) |
| 5 | [MINOR] garbled WARN sentence | `docs/troubleshooting.md:88` | "the reading is above the `85%` default and you raised `BLASTIT_MEM_HIGH_WATERMARK` past it" |
| 6 | [MINOR] `ci.md` 188 lines + internals leak | `docs/ci.md`, `ARCHITECTURE.md:1031-1045` | the page is **150 lines**. `Your suite` keeps three user-visible lines (needs `pytest`; `pytest-django` **and** `DJANGO_SETTINGS_MODULE`/`manage.py` ⇒ migrations and fixtures use `test_*`; statements outside a request are not audited). The `0600` file, `swapped_env`, `set_db` and the `django_db_setup` finalizer moved to a new **The test database** bullet in `ARCHITECTURE.md` §6, next to the other pytest-plugin bullets |

Also trimmed in `ci.md` (same leanness finding, no behaviour lost): the `--json`
paragraph now states the contract once (severity, `sql`, `fix.to`, the eight
`fix.action` names) instead of restating the table; the exit-code table became a
one-line legend; the `permissions` block in the workflow is one line.

Fence order and content in `ci.md` are unchanged for every fence the harness
runs: console fences 1–5 are still the same commands, the `findings` fence still
prints the six real findings the harness matches line by line, and no fence was
added or removed (indices in `inventory.toml` still resolve).

## Acceptance

### `make docs-test`

```console
$ make docs-test
PASS  docs/ci.md  fence 3  json                  0.9s  one JSON document with 6 findings
PASS  docs/ci.md  fence 4  check                 0.9s  check reported a clean suite
PASS  docs/ci.md  fence 5  learn-sequence        2.9s  learn, dry-run and check agree
…
PASS  docs/reference/cli.md  fence 4  usage      0.0s  the documented fold signature exists
38 fences: 26 executed, 12 accounted, 0 failed · 23.8s
$ echo $?
0
```

Run twice in this attempt after the final `ci.md` edit: `0 failed · 23.8s` and
`0 failed · 22.2s`.

### `make docs-check`

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
$ echo $?
0
```

Run after the last `ARCHITECTURE.md` reflow; zero warnings, no broken link or
anchor.

### `grep` for stale `0.1.0`

```console
$ grep -RIn '0\.1\.0' README.md docs/ | grep -v changelog
docs/VISION.md:40                     … `v0.1.0` is already tagged, so Milestone A ships as `0.1.1`
docs/decisions/ADR-000{2,3,4}-*.md    … "In `0.1.0`" / "the `0.1.0` `pytest11` plugin"
docs/design/ma-lean-dx*.md            … "legacy `0.1.0` plugin interference"
docs/knowledge/LESSONS.md:16          … `dist/blastit-0.1.0-*.whl`
```

No public page or `README.md` line matches; every remaining mention is release
history, and all of these files are excluded from the Zensical build.

### Help comparison (`blastit --help` / `blastit check --help`)

```console
$ bash -c 'blastit --help; blastit check --help' >/tmp/help.txt && grep -c -- '--learn' /tmp/help.txt
5
```

Same count as attempt 1, on `blastit 0.1.0` installed from the wheel this tree
just built into a throwaway venv. `--learn` appears in the usage lines for `dev`
and `check` and in the two flag lists; `--dry-run` and `-- ARGS...` are listed
under `check`; exit codes `0/1/2/3` are printed by `check --help`. The
flag-by-flag table from attempt 1 is unaffected: no flag, exit code or `--json`
key changed this attempt.

### Manifest fences still parse

```console
$ cargo test -p blastit-manifest --test docs_fences --locked
test only_a_complete_blastit_fence_is_collected ... ok
test an_invalid_regex_is_refused ... ok
test every_documented_manifest_parses_and_validates ... ok
test result: ok. 3 passed; 0 failed
```

### Page lengths

`ci.md` 150 · `local.md` 149 · `django.md` 148 · `cli.md` 104 ·
`troubleshooting.md` 89 · `settings.md` 165 · `anything-else.md` 176.

## Follow-ups (unchanged, outside this node's paths)

- `docs/knowledge/LESSONS.md` still has two overlapping uv-cache bullets.
- The built binary reports `build=0.1.0` while the docs describe `0.1.1`;
  `MA.3.6` owns the version bump.
