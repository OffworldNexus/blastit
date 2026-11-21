# review-ux — MB.41.3

Lens: does the docs harness now protect what the site promises, and was any
page claim bent to fit a test?

Verdict: **FAIL** — 1 BLOCKER, 3 MAJOR.

## BLOCKER

**[BLOCKER] `python/tests/runnable_docs/inventory.toml:64,79,268,294,318,348` — six
`example` fences whose `reason` says the gate checks their flags against the
installed CLI; `mode_example` checks nothing.**

`mode_example` (`harness.py:1726-1730`) only calls `_require_reason` and returns
`(f"accounted: …", False)`. The real static checker is `mode_static`
(`harness.py:1679-1699` → `_static_command`/`_static_blastit`/
`_static_presets`, `harness.py:3212-3236`), which really does validate every
executable, subcommand, flag and preset. So these six reasons are false:

- `:64` `docs/blastfile.md` fence 3 — "checks the documented command and its flags against the installed CLI"
- `:79` `docs/blastfile.md` fence 6 — "Both commands' flags are checked against the installed CLI"
- `:268` `docs/getting-started.md` fence 5 — "the command's flags are checked against the installed CLI"
- `:294` `docs/index.md` fence 3 — "checks both commands' flags against the installed CLI"
- `:318` `docs/local.md` fence 3 — "the command's flags are checked against the installed CLI"
- `:348` `docs/local.md` fence 9 — "Both commands' flags are checked against the installed CLI"

Consequence: a typo in any linked-tab PHP command (`$ blastit dev --prot 8080`,
`$ blastit fmt --chek`, `$ blastit --preset digital-ocean`) ships green while the
inventory tells the next reviewer it was flag-checked. This is the "excuse that
lets a broken command ship" case, and it undoes the one property `mode_static`
was built for. (Contrast `inventory.toml:180`, `docs/frameworks/php/index.md`
fence 1, which kept `mode = "static"` and whose identical sentence is true.)

Concrete fix: change those six `mode = "example"` lines to `mode = "static"` and
keep the reasons verbatim. `_static_command` strips trailing `# …` comments
(`harness.py:3047`), so `$ blastit dev  # the archive, plus the host's NTS
libphp` and `$ blastit --preset digitalocean-paas` validate cleanly; `cli.fmt`
was added by this node, so `fmt --check` validates too. If a fence genuinely
cannot pass `mode_static`, rewrite the reason to say what actually runs
("accounted only; `docs_fences` parses the tab's Blastfile") — never claim a
flag check that does not exist.

## MAJOR

**[MAJOR] `docs/frameworks/static-and-spa.md:30-34` — the banner transcript omits
the `static` mount row the binary always prints, and this node made the fence
execute so the gate now green-lights it.**

The page's own Blastfile is `static "/" { root "public" }`; `dev_learn.rs:106-141`
builds one row per route *and* one per mount ("a banner that listed only views
named a tree it was not serving"; `GET, HEAD` is the mount's fixed method cell),
and `dev_learn.rs:889-905` pins the extra line. The live banner is therefore four
lines, and the page shows three:

```
  http · scheduler · dbproxy · 0 routes
  static  /  GET, HEAD  public          ← missing
```

`expect(policy="exact")` (`harness.py:1839-1875`) only requires each *documented*
line to appear somewhere in the output, so an omission can never fail — and
`e2e/static_assets.py:13` compares "the process's first three lines", so the
mount row is asserted nowhere at all. A regression that dropped the mount row
from the banner would leave both gates green while `docs/local.md:45` still
promises "a row per view and per `static` mount".

Fix: add `  static  /  GET, HEAD  public` to the fence, then make the row count
asserted — in `mode_dev`, when the fence documents a `http · scheduler · dbproxy
· N routes` line, compare the number of documented `  view`/`  static` rows with
the live ones (or add `banner rows` to `e2e/static_assets.py`'s banner check).

**[MAJOR] `python/tests/runnable_docs/inventory.toml:397-398` — the
`docs/security.md` reason cites a `docs/local.md` `denial` fence that no longer
exists; the page's `500` / `sql.denied products` promise is now asserted by nothing.**

`grep -n denial python/tests/runnable_docs/inventory.toml` returns only this
reason. `docs/local.md` now numbers 10 fences, none with `mode = "denial"` /
`fixture = "denial"` (its "## A denial" section is prose only, and `fixtures/denial`
is unused). So "docs/local.md's `denial` fence asserts the same `sql.denied`
line" is false, and the security page's core evidence — `docs/security.md:12-14`,
`500` for an ungranted `/products/9` — has no automated check left. The mode
still exists in the harness (`harness.py:1335`), so this is a lost fence, not a
retired mode.

Fix: restore a `denial` fence on `docs/local.md`'s "## A denial" section
(`fixture = "denial"`, mode `denial`) — it is the promise the page's whole
argument hangs on — or move the fence to `docs/security.md` with `mode =
"denial"`. At minimum, correct the reason and stop claiming a check that does
not exist.

**[MAJOR] `python/tests/runnable_docs/harness.py:1547-1589` (`mode_check_roots`)
never compares the fence's documented output.**

Every other executing mode calls `self.expect(...)`; `mode_check_roots` reads
`code`, then only inspects `--json`. Today's four `check-roots` fences happen to
document no output, so nothing is broken yet — but the moment a page writes
`$ blastit check` followed by a line (e.g. the `clean`/`0 routes` line that
`docs/blastfile.md` prose implies, or the `fix:` block), a fabricated line is
accepted silently and the mode reports "every root audited". That is the exact
"future documentation regression invisible" shape. Fix: after the exit-code
check, `self.expect(command, code, stdout + stderr, "exact")` for the non-`--json`
commands (containment is fine; it is the *absence* of any comparison that is the
hole).

## MINOR

- `docs/security.md:9-13` shows the same transcript with its view row missing
  ("1 route" then straight to `curl`) — same defect as the static page, same fix:
  add the `  view  …  GET, HEAD  …` row the banner prints. It is `mode = "example"`
  so no gate will ever say so.
- `inventory.toml:253,258,268` account three getting-started fences partly by
  "`scripts/build-standalone.sh` builds that archive"; the *documented* release
  asset name (`blastit-x86_64-unknown-linux-gnu.tar.gz`, `docs/getting-started.md:21,27`,
  also `docs/ci.md`) is now checked by nothing in either docs gate. A one-line
  assert in the standalone packaging script or `e2e` that the built tarball name
  equals the documented one would close it.
- `e2e/static_assets.py:198-206` still accepts `uvx blastit …` spellings although
  no page uses `uvx` any more (no `reason` in `inventory.toml` mentions it — that
  part is clean). Dead aliases invite a future page to reintroduce them.
- `python/tests/test_docs_assets.py:1339-1341,1364-1366` pin two identical
  `('app "shop"', True)` / `('app "myshop"', True)` keys per page. Positional
  comparison keeps it honest, but two same-key rows mean a tab-swap or a
  copy-paste between the Python and PHP tabs cannot be told apart by the failure
  message. Prefer the discriminating first line (the `entrypoint` line) as the key
  for the duplicated pairs.
- `inventory.toml:304` (`docs/index.md` fence 4) says `e2e/static_assets.py`
  "runs both commands in its container and compares the banner line for line";
  that driver reads `docs/frameworks/static-and-spa.md` only (`e2e/static_assets.py:48`)
  and compares the *summary* line, not the transcript. Reword to what it does.

## Verified clean (no finding)

- `README.md:67` and `docs/getting-started.md:100` both now read
  `view  hello  GET, HEAD  /.*`, matching `dev_learn.rs:118-131` (a declared `GET`
  route prints `GET, HEAD`, unit test `dev_learn.rs:857`) and the compiled
  pattern. No other published page prints a `GET, HEAD` row.
- The 40/32/8 re-pin in `test_docs_assets.py` kept the real assertions:
  per-page **per-index** key+verdict comparison plus `len(rows) == pinned`
  (`test_docs_assets.py:1786-1813`) and the anti-surplus check in `blocks_for`
  (`:1640`). `docs/local.md: ()` and the dropped `docs/ci.md` row are safe because
  `set(measured) | set(EXPECTED_FENCES)` still fails on a new `text` fence there.
- Tab bodies: the only page edits are two banner lines; the 4-space tab bodies are
  untouched and the renumbered inventory matches the de-indented fence order
  (59 fences, 32 executed, 0 failed).
