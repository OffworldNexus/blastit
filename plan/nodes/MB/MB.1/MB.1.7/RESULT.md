# RESULT MB.1.7 — Build and brand hygiene: wheel README links, `CARGO_NET_OFFLINE=true`, events serde, logo contrast

## Summary

1. **`scripts/build-wheel.sh` now builds the wheel PyPI needs.** The README is rewritten in place with
   `scripts/readme-absolute-links.py` right before `maturin build`, and a `trap restore_readme EXIT`
   copies the saved original back on **every** exit path (success, maturin failure, `uv` failure),
   removing the temp file. So `make check` -> `docs-test` -> this script cannot leave a dirty tree,
   and a locally built `dist/*.whl` carries the same absolute `github.com`/`raw.githubusercontent.com`
   links the published wheel carries. The rewrite runs on `uv`, falling back to `PYO3_PYTHON` for the
   CI jobs that build an e2e wheel from a pipx maturin and have no `uv`.
2. **`CARGO_NET_OFFLINE=true` everywhere a maturin run can inherit it.** `Makefile` `test-critical`
   (lines that used `=1`) now spell `true`, with a comment pointing at `LESSONS.md`; nothing under
   `Makefile`, `scripts/` or `.github/` contains `CARGO_NET_OFFLINE=1` any more.
3. **`serde` dropped from `crates/blastit-events`.** The crate only used `serde_json` (no `serde::`
   path, no `Serialize`/`Deserialize` derive anywhere in `src/`), so the dependency line went and
   `Cargo.lock` lost exactly one edge (`- "serde"` under `blastit-events`); the `serde` package
   entry itself stays, still required by other members. Builds clean `--offline` and `--locked`.
4. **Light-scheme logo contrast fixed inside the asset.** `docs/brand/logo.svg` keeps the paper
   cutout (`#fbfaf6`) as its default and adds an inline
   `@media (prefers-color-scheme: light) { #head-cutout { fill: #161a20 } }` rule — no second file,
   no page CSS (the header loads it as an `<img>`, which page CSS cannot reach). `#161a20` is the
   brand ink, i.e. the same two colours as `mascot-head-ink.svg` / `mascot-head-paper.svg`.
5. **`scripts/dev-version.sh` polish (both MINOR items from `MB.1.9`).** The post-rewrite `awk` scan
   now also reads the workspace-root `Cargo.toml` (so a `path = "../…"` requirement declared there is
   reported instead of silently escaping), and the failure message says a requirement "names a
   version other than `<dev>` (or none)". Re-proved on throwaway `/tmp` copies, including the
   before/after on the root-`Cargo.toml` escape.

A real defect was found on the way: the first draft of the `<style>` comment contained a literal
`<img>`, which makes the SVG ill-formed XML — the asset then decodes to `naturalWidth 0` (alt text in
the page) while `make docs-check` still reports "No issues found". Fixed by `&lt;img&gt;`, verified by
parsing the file and rendering it in Chromium, and recorded in `docs/knowledge/LESSONS.md`.

## Evidence

### 1. No strict-bool-hostile spelling remains (PASS)

```console
$ ! grep -rn 'CARGO_NET_OFFLINE=1' Makefile scripts .github && echo "only true remains"
only true remains
$ grep -rn 'CARGO_NET_OFFLINE' Makefile scripts .github
Makefile:100:# `CARGO_NET_OFFLINE` has to be the string `true`, not `1`: cargo accepts both,
Makefile:112:	$(RUST_ENV) CARGO_NET_OFFLINE=true $(CARGO) build --locked -p blastit-py -p blastit-dbproxy; \
Makefile:116:	CARGO_NET_OFFLINE=true UV_OFFLINE=1 PYTHONPATH="$${stage}:$(CURDIR)/python" \
```

### 2. A local wheel build rewrites and restores the README; the wheel is PyPI-correct (PASS)

```console
$ ./scripts/build-wheel.sh
README.md: .github/brand/header-light.svg → https://raw.githubusercontent.com/OffworldNexus/blastit/develop/.github/brand/header-light.svg
README.md: ARCHITECTURE.md → https://github.com/OffworldNexus/blastit/blob/develop/ARCHITECTURE.md
README.md: LICENSE → https://github.com/OffworldNexus/blastit/blob/develop/LICENSE
🍹 Building a mixed python/rust project
🐍 Found CPython 3.14 at /home/remy/.cache/uv/archive-v0/uJWfWrFj-SC_y89E/bin/python3
    Finished `release` profile [optimized] target(s) in 21.99s
📦 Including files matching "python/blastit/_bin/blastit-dbproxy"
📦 Built wheel for CPython 3.14 to dist/blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl
EXIT=0

$ test -z "$(git status --porcelain README.md)" && echo "README restored"
README restored
```

The acceptance block's heredoc is refused by this session's shell tool, so the same snippet was run
from a file (`/tmp/opencode/mb17-check-wheel.py`, byte-identical body):

```console
$ uv run --no-project python /tmp/opencode/mb17-check-wheel.py
wheel: dist/blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl
wheel description links are absolute
```

The rewrite runs on `uv` by default, but four `ci.yml` jobs (`e2e`, `rootless`, `wheel-smoke`,
`pytest-plugin`) and `live.yml` call this script with `MATURIN=maturin` from a pipx install and have
no `setup-uv`, exporting `PYO3_PYTHON` instead — so `uv` is tried first, `PYO3_PYTHON` second, and
the script refuses with an actionable message rather than building a wheel with 404 links. All three
branches, run from the extracted block of the shipped script:

```console
$ <block from scripts/build-wheel.sh, uv on PATH>
chosen: uv run --no-project python
runs: 3.14.10
$ PATH=/usr/bin:/bin:/home/remy/.cargo/bin PYO3_PYTHON=…/cpython-3.14.10-…/bin/python3.14 <same block>
chosen: /home/remy/.local/share/uv/python/cpython-3.14.10-linux-x86_64-gnu/bin/python3.14
runs: 3.14.10
$ PATH=/usr/bin:/bin <same block>; echo "exit=$?"
the README rewrite needs uv or PYO3_PYTHON: install uv, or point PYO3_PYTHON at an interpreter
exit=1
```

**The trap also restores when the build fails** (every exit path, not just the happy one):

```console
$ sha256sum README.md > /tmp/opencode/readme.before
$ ./scripts/build-wheel.sh --no-such-maturin-flag; echo "script EXIT=$?"
error: unexpected argument '--no-such-maturin-flag' found
script EXIT=2
$ sha256sum -c /tmp/opencode/readme.before
README.md: OK
$ git status --porcelain README.md | wc -l
0
$ ls /tmp/blastit-README.* 2>/dev/null | wc -l
0
```

### 3. `serde` gone, the lockfile still locks the workspace (PASS)

```console
$ ! grep -qE '^serde *=' crates/blastit-events/Cargo.toml && echo "serde removed"
serde removed
$ CARGO_NET_OFFLINE=true cargo build --workspace --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.27s
$ CARGO_NET_OFFLINE=true cargo build --workspace --locked --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.10s
$ git diff --stat Cargo.lock
 Cargo.lock | 1 -
 1 file changed, 1 deletion(-)
$ git diff Cargo.lock
@@ -170,7 +170,6 @@ dependencies = [
 name = "blastit-events"
 version = "0.1.1"
 dependencies = [
- "serde",
  "serde_json",
```

(The `dev`/`--offline` builds ran with `make`'s `RUST_ENV`: `PYTHONHOME`/`PYO3_PYTHON`/`LD_LIBRARY_PATH`
for CPython 3.14, so `blastit-py` links as `make check` does.)

### 4. Light-scheme logo contrast (PASS — 15.22:1 against `#f4efe3`)

```console
$ grep -q 'prefers-color-scheme' docs/brand/logo.svg && echo "light-scheme aware"
light-scheme aware
$ LIGHT_FILL=$(grep -oE '#[0-9a-fA-F]{6}' docs/brand/logo.svg | sort | uniq -c | sort -rn | head -2 | tail -1 | awk '{print $2}') \
  uv run --no-project python -c '...'          # the acceptance snippet, verbatim
contrast 15.22:1
$ echo "$LIGHT_FILL"                           # which colour that was, same pipeline
#161a20
```

`#161a20` is the light-scheme cutout the file ships (its second hex); the first is the dark-scheme
paper `#fbfaf6` the light rule replaces.

`#f4efe3` is the documented light background; the real light header is `#ffffffb3` over it (nearer
white, so the ratio there is higher). The `<img>` isolation is real, and the asset-level media query
is what closes it — rendered in Chromium, `colorScheme` emulated, page background set by hand:

| scheme | page background | rendered asset |
|---|---|---|
| `light` | `#f4efe3` | dark ink bird head, clearly visible (`.playwright-mcp/mb17-light.png`) |
| `dark` | `#121820` | paper-white bird head, unchanged (`.playwright-mcp/mb17-dark.png`) |

```console
$ uv run --no-project python -c 'import xml.etree.ElementTree as ET; ET.parse("docs/brand/logo.svg"); print("parses")'
parses
```

### 5. Docs site + full gate (PASS)

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s

$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
PASS  docs/security.md  fence 1  static  ...
38 fences: 26 executed, 12 accounted, 0 failed · 22.9s
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
real 0m56.193s
```

`make check` runs `docs-test` -> `scripts/build-wheel.sh`, and the tree afterwards is the intended
one only (`README.md` clean, no stray files):

```console
$ git status --porcelain README.md Cargo.lock dist python/blastit
 M Cargo.lock
```

### 6. `scripts/dev-version.sh` polish, proved on `/tmp` copies (PASS)

Happy path on a throwaway copy (`rsync` of the tree, 133M, never run in the repo):

```console
$ cd /tmp/opencode/mb17/work && CARGO_NET_OFFLINE=true scripts/dev-version.sh 14
0.1.1-dev14
exit=0
$ grep -n '^version' Cargo.toml
6:version = "0.1.1-dev14"
$ # requirements rewritten to 0.1.1-dev14 (count over crates/*/Cargo.toml)
requirements rewritten: 28
$ grep -n -A2 '^name = "blastit-py"' Cargo.lock
220:name = "blastit-py"
221-version = "0.1.1-dev14"
$ CARGO_NET_OFFLINE=true cargo metadata --locked --no-deps --format-version 1 >/dev/null && echo "metadata --locked OK"
metadata --locked OK
```

(a) A requirement declared in the workspace-root `Cargo.toml` is now scanned — and the **old**
script exits 0 on it, which is the escape being closed:

```console
$ # /tmp/opencode/mb17/rootcase, Cargo.toml + [workspace.dependencies] oops = { version = "0.1.1", path = "../oops" }
$ scripts/dev-version.sh 14
intra-workspace requirements not rewritten to `0.1.1-dev14`:
Cargo.toml:24: oops = { version = "0.1.1", path = "../oops" }
a requirement naming a version other than `0.1.1-dev14` (or none) does not accept the pre-release `0.1.1-dev14`, so no build of it can resolve
rootcase exit=1

$ # same tree, HEAD's script copied in as scripts/old-dev-version.sh
$ bash scripts/old-dev-version.sh 14
0.1.1-dev14
old-script exit=0
```

(b) A *versionless* path dependency now says so:

```console
$ # /tmp/opencode/mb17/nonecase, blastit-manifest = { path = "../blastit-manifest" }
$ scripts/dev-version.sh 14
intra-workspace requirements not rewritten to `0.1.1-dev14`:
crates/blastit-env/Cargo.toml:14: blastit-manifest = { path = "../blastit-manifest" }
a requirement naming a version other than `0.1.1-dev14` (or none) does not accept the pre-release `0.1.1-dev14`, so no build of it can resolve
nonecase exit=1
```

## Files

- `scripts/build-wheel.sh` — README rewrite + `trap … EXIT` restore + header note.
- `Makefile` — `CARGO_NET_OFFLINE=true` (2 sites) and the strict-bool comment.
- `scripts/dev-version.sh` — `awk` scan now includes `Cargo.toml`; reworded message (2 places).
- `crates/blastit-events/Cargo.toml` — `serde` removed (no `src/` change).
- `Cargo.lock` — one edge removed under `blastit-events`.
- `docs/brand/logo.svg` — inline `@media (prefers-color-scheme: light)` cutout colour.
- `docs/knowledge/LESSONS.md` — one line on SVG-as-`<img>` well-formedness + asset media queries.

## Follow-ups

- **The asset's media query follows the OS preference, not the site toggle.** A reader on a dark OS
  who clicks the site's "light" toggle still gets the paper cutout on the near-white header, because
  `data-md-color-scheme` cannot reach inside an `<img>`. Closing that needs either the logo inlined
  into the header template (page CSS can then style it) or `<picture>` with two assets — both are
  outside this node's scope, and the node forbids a second logo file.
- `docs/design/ma-lean-dx.md` (R42, and several `MA.*` node acceptance blocks) still spell
  `CARGO_NET_OFFLINE=1`; those are historical plan/design text, not executables.
- `dist/` now holds a locally built `blastit-0.1.1-cp314-cp314-manylinux_2_39_x86_64.whl`
  (gitignored) with `develop` links, from the acceptance run.
- `.playwright-mcp/mb17-light.png` / `mb17-dark.png` are the render proofs quoted above (tool output,
  gitignored).
