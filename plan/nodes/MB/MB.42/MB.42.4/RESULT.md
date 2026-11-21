# RESULT — MB.42.4

## Status
Everything this node owns is green: `make docs-unit` (117 passed), the runnable-docs
harness (all 60 public console fences, 32 executed / 28 accounted / 0 failed),
`make docs-check`, `platform_matrix.py --check`, `make fast-check`, and `make e2e`'s wheel
plus Docker leg (`docs_deploy: 46/46`, the PHP `141`-promise leg). Two gates cannot pass on
this machine and neither is fixed by a file in my `paths`; both are under
[Follow-ups](#follow-ups-out-of-scope): `make docs-test` (and so `make check`) dies inside
`scripts/build-wheel.sh` before any fence runs, and `make e2e`'s final **rootless** leg needs
`CAP_SETFCAP`, which this outer environment refuses (`setcap cap_setuid+ep` → `Operation not
permitted`).

## 1. The PHP install line is a `console` fence, not `text`
`docs/frameworks/php/index.md` carried
`curl -fsSL https://offworldnexus.github.io/blastit/install.sh | sh` in a `text` block. A
`text` block means "not a command the gate knows", but that line *is* the documented
install command, so it is now `console` with the same `$ ` prompt the hero uses, and the
inventory accounts it (`example`, written reason) instead of pretending it is prose.

The census is **not** bumped. `test_every_text_block_classifies_as_measured` pins 40
`text` blocks (32 highlighted + 8 plain); the surplus fence was the 41st, so moving it out
of `text` restores exactly the pinned 40 with the pinned split. `EXPECTED_FENCES` and
`STALE_CLAIMS` need no edit. Page trimmed 131 → 129 lines, still with its Mermaid diagram
and `## Where next`.

## 2. `python/tests/runnable_docs/inventory.toml`
* **`docs/install.md`** (new page entry, 4 fences): the installer trio
  (`install.sh`, `--version`, `dev`), the apt trio, the dnf trio and the local
  `.deb`/`.rpm` install — each `example` with a written reason naming what is checked
  instead. The page's three prompt-bearing `bash` blocks became `console`, because
  `fences.scan_page` refuses a `$ ` prompt inside a `bash` block: a reader copies those
  lines, so the gate must see them.
* **`docs/index.md`** 4 → 1 entry: the hero's three universal lines, `example`.
* **`docs/getting-started.md`** 6 → 4 entries, in the page's order: the install line
  (`example`), `uv init --app` + `uv add --dev blastit` (`install`, executed for real),
  `composer require --dev` (`example`), and the `dev` banner on the `hello` fixture
  (`dev`). Page trimmed 133 → 129 lines.
* **`docs/frameworks/php/index.md`** renumbered to 3: install line (`example`),
  `blastit dev --learn`, `blastit fmt --check`. Pages are kept, never deleted to make a
  count match.

## 3. Harness: one `py3-none` wheel (`python/tests/runnable_docs/harness.py`)
`MB.42.3.3` ships `blastit-<version>-py3-none-<platform>.whl`, but `_wheel_python` raised
`carries no cp3NN ABI tag` for it, so `docs-test` would have died at `wheel_path()` the
moment the inventory was accepted. It now names the harness's own CPython for a
`py3-none` wheel (one artifact serves 3.12+) and keeps the ABI tag for a legacy `cp3NN`
wheel; `wheel_path`, `interpreter_for` and the provenance comment follow. `test_harness.py`
gains one test that a `py3-none` wheel is the wheel `wheel_path()` returns.

## 4. Navigation
`zensical.toml` gains `{ "Install" = "install.md" }`: the page was published (and linked
from the hero, Getting started and the PHP page) but absent from the menu.

## 5. Fence pins (`crates/blastit-manifest/tests/docs_fences.rs`)
The content rewrite changed how many fences three pages carry, and `PAGES`/`TOTAL` still
held the pre-rewrite numbers (`rs-test` failed on `docs/index.md`: 3 documents + 2
illustrations, pinned 5). Re-measured with the same classifier the suite uses and pinned:
`docs/index.md` 5 → 2 illustrations, `docs/getting-started.md` 10 → 8, the PHP page 4 → 5
(its install line became a `console` fence, which is still an illustration);
`TOTAL` 63 → 59 illustrations, documents unchanged at 17.

## 6. `docs/deploy/digitalocean-paas.md` (`make e2e`)
`e2e/docs_deploy.py` holds 46 documented pooler promises against the page; two had drifted
in the rewrite. Both are the harness's own sentence: the pool step now says
"Create pool**: name `shop-pool`" (pairing the console step with
`doctl databases pool create … shop-pool --db defaultdb --size 18 --user app`), and the tag
sentence reads "`REPLACE_WITH_YOUR_TAG` below is this `$GIT_SHA`". No command, name or value
changed.

## 7. `e2e/php2shell.py` (the PHP leg reads this page's commands)
The leg replays the PHP page's `console` commands and refused the new install line
(`None` in its kinds) plus the retired "release tarball / `unknown-linux` / PyPI wheel ships
none" sentence. `command_kind` now knows the signed-repository `curl … /install.sh | sh`
as `install`, the pinned order is `["install", "dev-learn", "fmt-check"]`, and the "where
the binary comes from" promise asserts `install.sh` + "signed repository" instead.

## Evidence

### `make docs-unit`
```
117 passed, 1 skipped, 65 subtests passed in 6.54s
```

### runnable-docs harness (all 60 fences)
```
PASS  docs/getting-started.md  fence 2  install   0.6s  installed blastit-0.2.0-py3-none-linux_x86_64.whl and verified it against the built wheel
PASS  docs/getting-started.md  fence 4  dev       0.0s  served on 127.0.0.1:8080
PASS  docs/install.md          fence 1  example   0.0s  accounted: Opens with the installer, …
...
60 fences: 32 executed, 28 accounted, 0 failed · 22.1s
```

### `make docs-check`
```
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.35s
```

### `uv run --no-project scripts/platform_matrix.py --check`
```
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared;
… 4 packages installed in 12 container legs; … 0 leg(s) dropped
```

### `make fast-check`
```
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch …)
exit=0
```

### `make e2e` (with `E2E_SQLALCHEMY_PORT=18091 E2E_TLS_PORT=18453`)
A foreign container on this host publishes `18081` and `18443`; the compose files document
these overrides for a second checkout. Wheel and Docker leg:
```
📦 Built wheel to /wheel-out/blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl
wheel-tag: blastit-0.2.0-py3-none-manylinux_2_28_x86_64.whl: Tag: py3-none-manylinux_2_28_x86_64
docs_deploy: 46/46 documented pooler promises hold
uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt
…
FAIL: 22 failure(s)
service "app" is not running
make: *** [Makefile:531: e2e] Error 1
```
The rootless leg is the only failure, and it is the host: `setcap cap_setuid+ep` here →
`unable to set CAP_SETFCAP effective capability: Operation not permitted`, which is exactly
the "capable copy" D6/E1 asserts.

### `uv run --no-project e2e/php2shell.py --self-test`
```
PASS: every `$ ` command on the page is replayed or explained (['install', 'dev-learn', 'fmt-check'])
PASS: the page's console commands are the install line and the two this leg replays (…)
PASS: the page still says where the binary comes from: the signed repository's one-liner
PASS: self-test, 25 checks
```

### `make check`
Every prerequisite before `docs-test` is green (this run's
`uvx --offline zensical build --strict` → `No issues found`); it then stops in
`scripts/build-wheel.sh` with the out-of-scope floor error below. Nothing in `docs/**`,
`python/tests/**`, `e2e/**` or `crates/blastit-manifest/tests/**` is named by it.

```
📦 Built wheel to .../target/docs-wheel/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
build-wheel: staged 2 shim(s) from this host for x86_64-gnu
error: --max-glibc '2_39_x86_64' is not <major>_<minor> (e.g. 2_28) or auto
error: libblastit_py313-x86_64-gnu.so outgrew the wheel's glibc floor (2_39_x86_64)
make: *** [Makefile:366: docs-test] Error 1
```

## Follow-ups (out of scope)

**`scripts/build-wheel.sh` cannot build a host wheel with maturin 1.16.** maturin tags the
`bindings = "bin"` wheel `py3-none-manylinux_2_39_x86_64`, and the floor is derived as
`${manylinux_ver:-${wheel_platform#manylinux_}}`, which keeps the architecture
(`2_39_x86_64`):

```
repack: target/docs-wheel/blastit-0.2.0-py3-none-manylinux_2_39_x86_64.whl
wheel-tag: error: invalid --max-glibc '2_39_x86_64': expected <major>_<minor>, e.g. 2_28
```

so `make docs-test` (and `make check`, and CI's `pipx install maturin && scripts/build-wheel.sh`
legs) stop before the fences run, while the script's own comment still expects the host
path to "keep the unversioned `linux_<arch>` tag". The fix belongs to whoever owns
`scripts/`: strip the architecture from the derived floor (or pass `auto`) and pin the
maturin CI resolves. The equivalent host wheel — and the one every harness verdict above was
produced from — is `scripts/build-wheel.sh -o <dir> --compatibility linux`.

Two generated trees were touched to get there, neither of them source: `target/php-sapi`
(a distro-built shim set a previous run had staged into the docs wheel) was removed, and
`target/py-sapi` was kept, because the `dev` fences need the CPython shim inside the wheel.

**`make e2e`'s rootless leg needs `CAP_SETFCAP`.** `e2e/rootless/run.py` sets file
capabilities on the capable copy; this host denies that call, so the app container exits and
22 cascading D6/E1 checks fail. Nothing in this node's diff touches the production layout.

**`docs/deploy/digitalocean-paas.md` is 225 lines**, over the page budget, from before this
node (only two sentences changed here); shortening a 46-promise deployment recipe is a
review of its own.
