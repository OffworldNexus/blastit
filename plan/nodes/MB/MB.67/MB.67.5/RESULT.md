# MB.67.5 — one source for the workspace version

`Cargo.toml`'s `[workspace.package] version` is now the only place the version is
written; every script, fixture and generated doc example derives it.

## What changed

| File | Change |
|---|---|
| `scripts/version.sh` **(new)** | The one loader: `blastit_workspace_version [MANIFEST]`, sourceable and executable (`scripts/version.sh` prints it), root from `${BASH_SOURCE[0]}`, the same single `sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml \| head -1` as `scripts/dev-version.sh`, hard error naming the file when the line is absent. `-h/--help`; an unknown option exits `2`. |
| `packaging/package-set.sh` | Sources `scripts/version.sh` and re-exports `blastit_package_version` (the way it already re-exports the language rows), plus `--version` on the CLI and its two usage texts. |
| `scripts/gen_language_blocks.py` | New `<!-- BEGIN/END generated:version -->` block in `docs/install.md`: the "Without `curl \| sh`" console sample with this version's real filenames (deb `blastit_<version>_amd64.deb` / `blastit-php-<newest-minor>_<version>_amd64.deb`, rpm `blastit-<version>-1.x86_64.rpm` / `blastit-php-<newest-minor>-<version>-1.x86_64.rpm`). The generator reads the version by running `scripts/version.sh` (no second manifest parser) and the newest PHP minor from `packaging/languages.tsv`. `--check` (the default) fails with a unified diff on a version bump; `--write` regenerates. |
| `docs/install.md` | The download sample is the generated block (now PHP 8.6, the newest row); the "e.g. 0.2.1" prose example is gone (`<version>` is the only spelling left in prose). |
| `docs/install.sh` | `BLASTIT_DEFAULT_VERSION="<version>"` is a `# BEGIN/END generated:version` block written by the same generator (the digest table already keys on `"$BLASTIT_DEFAULT_VERSION":`, so nothing else moved); the Docs job's own version guard is unchanged and still passes. |
| `scripts/test_install.sh` | `workspace_version=$(bash "$root/scripts/version.sh")`; the fixture binary, the `(stub)` binary, the three faked `.deb` names, the two faked `.rpm` names and all four `BLASTIT_DEB`/`BLASTIT_RPM`/case assertions derive from it. No `0.2.0` literal remains. Case (j)'s prose now says it covers the version too, since it runs the same `--check`. |
| `packaging/README.md` | File-name example is `blastit-php-8.4_<version>_amd64.deb` / `blastit-php-8.4-<version>-1.x86_64.rpm`, with the one sentence that `<version>` is `Cargo.toml`'s `[workspace.package] version`. |
| `e2e/README.md` | The three `--deb/--rpm/--wheel` dist examples are `blastit_<version>_amd64.deb`, `blastit-<version>-1.x86_64.rpm`, `blastit-<version>-py3-none-manylinux_2_28_x86_64.whl`, with the one sentence defining `<version>` as the `Cargo.toml` workspace version. |
| `RELEASING.md` | Nightly stamps are `<version>~nightly<YYYYMMDD>.r<run>+g<sha7>` / `<version>-0.nightly.<YYYYMMDD>.r<run>.g<sha7>`, and the development-wheel paragraph says `<version>-dev<run>` (the workspace version plus `-dev<run>`, `scripts/dev-version.sh`) — naming-scheme examples, not a stamped release. |

`Cargo.toml` is untouched by this slice.

## Evidence

### 1. The acceptance grep

```
$ grep -rnE '0\.[0-9]+\.[0-9]+-' scripts/ e2e/ docs/install.md packaging/README.md RELEASING.md
scripts/check-release-tag.sh:37:# `0.1.0-dev42` is a development wheel's version (dev-version.sh): what CI
scripts/dev-version.sh:11:#   scripts/dev-version.sh 42        # 0.1.1 -> 0.1.1-dev42 -> blastit-0.1.1.dev42-...whl
scripts/dev-version.sh:21:#   one, so `0.1.1` would no longer accept `0.1.1-dev42`. The rewrite tolerates
scripts/dev-version.sh:63:# `0.1.1-dev42`. maturin normalises that to the PEP 440 spelling a wheel carries,
docs/install.md:114:$ sudo dnf install ./blastit-0.2.1-1.x86_64.rpm ./blastit-php-8.6-0.2.1-1.x86_64.rpm
```

Itemised:

* `scripts/check-release-tag.sh:37`, `scripts/dev-version.sh:11,21,63` — **exempt
  history/prose**: they quote `0.1.0-dev42`/`0.1.1-dev42` to explain what a
  development wheel's version *is*, exactly the exemption `MB.67` names. No
  configuration reads them.
* `docs/install.md:114` — the **generated** block: `0.2.1` is not typed there, it is
  what `scripts/version.sh` printed this tree. The bump below proves it (the same
  lines read `9.9.9` with only `Cargo.toml` edited, and `--check` fails otherwise).
  (`docs/install.md:113`, the apt line, matches the same block; the grep pattern
  only sees the rpm line because only that one carries `-1.`.)

No other executable, fixture or doc example in those paths spells a version.

### 2. Scratch bump to `9.9.9` (only `Cargo.toml` edited, then reverted)

```
$ sed -i '0,/^version = "0.2.1"$/s//version = "9.9.9"/' Cargo.toml
== Cargo.toml workspace line
[workspace.package]
version = "9.9.9"
edition = "2024"
== scripts/version.sh
9.9.9
== packaging/package-set.sh --version
9.9.9
== sourced blastit_package_version
9.9.9
== gen_language_blocks.py --check (expect stale)
--- a/docs/install.md
+++ b/docs/install.md
@@ -110,8 +110,8 @@

 <!-- BEGIN generated:version -->
 ```console
-$ sudo apt-get install ./blastit_0.2.1_amd64.deb ./blastit-php-8.6_0.2.1_amd64.deb
-$ sudo dnf install ./blastit-0.2.1-1.x86_64.rpm ./blastit-php-8.6-0.2.1-1.x86_64.rpm
+$ sudo apt-get install ./blastit_9.9.9_amd64.deb ./blastit-php-8.6_9.9.9_amd64.deb
+$ sudo dnf install ./blastit-9.9.9-1.x86_64.rpm ./blastit-php-8.6-9.9.9-1.x86_64.rpm
 ```
 <!-- END generated:version -->

generated-blocks: docs/install.md is stale: run `scripts/gen_language_blocks.py --write`
exit=1
== --write
generated-blocks: wrote docs/install.md
generated-blocks: docs/install.sh is current
== --check again
exit=0
```

`sh scripts/test_install.sh` under `9.9.9` (no edit to it):

```
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 9.9.9 (fixture))
...
install-test: ok (i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/mb67/install-999/install-sh-test)
```

The fixtures it built, read back from its scratch tree:

```
$ ls /tmp/mb67/install-999/install-sh-test/h/set /tmp/mb67/install-999/install-sh-test/h/rpmset
.../h/rpmset:
blastit-9.9.9-1.x86_64.rpm
blastit-php-8.3-9.9.9-1.x86_64.rpm
.../h/set:
blastit_9.9.9_amd64.deb
blastit-php-8.3_9.9.9_amd64.deb
blastit-python-3.13_9.9.9_amd64.deb

$ grep 'blastit 9.9.9' .../h/bin/blastit
2:printf "blastit 9.9.9 (stub)\n"
```

Revert + regenerate + drift check:

```
$ cp /tmp/mb67/Cargo.toml.orig Cargo.toml
$ scripts/version.sh
0.2.1
$ scripts/gen_language_blocks.py --write
generated-blocks: wrote docs/install.md
generated-blocks: docs/install.sh is current
$ scripts/gen_language_blocks.py --check ; echo check-exit=$?
check-exit=0
$ git status --porcelain Cargo.toml docs/install.md scripts/ packaging/README.md e2e/README.md RELEASING.md
 M RELEASING.md
 M docs/install.md
 M e2e/README.md
 M packaging/README.md
 M scripts/gen_language_blocks.py
 M scripts/test_install.sh
?? scripts/version.sh
$ git diff docs/install.md | diff -u /tmp/mb67/docs-before.diff - && echo docs-install.md-diff-identical-to-pre-bump
docs-install.md-diff-identical-to-pre-bump
```

`Cargo.toml` is absent from `git status` (reverted byte-for-byte) and the install
page's diff after the revert is identical to the diff before the bump: the only
committed docs change is this slice's generated block.

### 3. One value, three ways to ask

```
$ scripts/version.sh
0.2.1
$ bash packaging/package-set.sh --version
0.2.1
$ bash -c 'source packaging/package-set.sh; blastit_package_version'
0.2.1
$ sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1
0.2.1
```

### 4. Gates

```
$ make docs-check
uv run --no-project scripts/gen_language_blocks.py --check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.52s

$ sh scripts/test_install.sh
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)

$ make fast-check
...
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s

sh scripts/test_install.sh
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 0.2.1 (fixture))
install-test: ok (b) a wrong digest exits 1 and installs nothing
install-test: ok (c) an overridden origin without a digest fails closed with zero requests
install-test: ok (d) an archive with a symlink member is refused before unpacking
install-test: ok (e) --help exits 0 with env -i (no root, no network)
install-test: ok (f) usage errors exit 2 (relative --prefix, --nightly --user, path-shaped --version)
install-test: ok (g) a piped 'curl | bash' run whose child reads stdin reaches the last line
install-test: ok (h) apt installs the core first, each shim alone, and warns instead of aborting on a shim it cannot install
install-test: ok (i) the local-file path installs the core plus only the shim packages it has, and warns about the rest
install-test: ok (i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)

$ uv run --no-project tools/plan.py validate
plan OK
```

(the same four commands were re-run once more after this file's last edit, with the
same result: `final-exit=0`; the follow-up pass below re-ran them again.)

### 5. Follow-up pass: the last four literals (paths extended)

| # | File | Before | After |
|---|---|---|---|
| 1 | `docs/install.sh:61` | `BLASTIT_DEFAULT_VERSION="0.2.1"` (typed) | a `# BEGIN/END generated:version` block; `scripts/gen_language_blocks.py` writes `BLASTIT_DEFAULT_VERSION="<workspace version>"`. The digest table already keys on `"$BLASTIT_DEFAULT_VERSION":`, so no other line moved, and the Docs job's `sed` guard still reads the same shape (proved below, both states). |
| 2 | `RELEASING.md:165` | `blastit==0.2.1` | `blastit==<version>`, plus one sentence: `<version>` is `Cargo.toml`'s `[workspace.package] version`, the release the merge just published. |
| 3 | `e2e/run_packaging.py:5`, `scripts/install_matrix.sh:4` | `dist/blastit_0.2.0_amd64.deb` | `dist/blastit_<version>_amd64.deb`. |
| 4 | `packaging/README.md` | `package-stamp.sh --base 0.2.1`, `Version: 0.2.1` / `Release: 1`, `build-deb.sh`/`build-rpm.sh --version 0.2.1`, `--version 0.2.1 --ref refs/tags/v0.2.1` | `<version>` throughout, the nightly stamp *shape* and dates kept (`<version>~nightly20261122.r42+g90cb2db`, `<version>` + `Release: 0.nightly.20261122.r42.g90cb2db`), plus one sentence that `<version>` is `Cargo.toml`'s `[workspace.package] version`. |

Left exactly as they were, per the brief: `RELEASING.md:73`'s `0.2.0` → `v0.2.0`
tag illustration and `:132`'s "why `0.2.0`'s packages never built" history.

Scratch bump again (`Cargo.toml` → `9.9.9`, nothing else edited):

````
== guard before bump
guard: cargo=0.2.1 install.sh=0.2.1 OK
== version.sh
9.9.9
== --check (expect stale, both files)
--- a/docs/install.md
+++ b/docs/install.md
@@ -110,8 +110,8 @@
 <!-- BEGIN generated:version -->
 ```console
-$ sudo apt-get install ./blastit_0.2.1_amd64.deb ./blastit-php-8.6_0.2.1_amd64.deb
-$ sudo dnf install ./blastit-0.2.1-1.x86_64.rpm ./blastit-php-8.6-0.2.1-1.x86_64.rpm
+$ sudo apt-get install ./blastit_9.9.9_amd64.deb ./blastit-php-8.6_9.9.9_amd64.deb
+$ sudo dnf install ./blastit-9.9.9-1.x86_64.rpm ./blastit-php-8.6-9.9.9-1.x86_64.rpm
 ```
 <!-- END generated:version -->
--- a/docs/install.sh
+++ b/docs/install.sh
@@ -61,7 +61,7 @@
 # BEGIN generated:version
-BLASTIT_DEFAULT_VERSION="0.2.1"
+BLASTIT_DEFAULT_VERSION="9.9.9"
 # END generated:version
 exit=1
== --write
generated-blocks: wrote docs/install.md
generated-blocks: wrote docs/install.sh
== install.sh stamp
64:BLASTIT_DEFAULT_VERSION="9.9.9"
== guard under bump
guard: cargo=9.9.9 install.sh=9.9.9 OK
== sh -n
sh-n-ok
== test_install under bump
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/mb67b/install-999/install-sh-test)
== revert + write
generated-blocks: wrote docs/install.md
generated-blocks: wrote docs/install.sh
gen-check=0
== guard after revert
guard: cargo=0.2.1 install.sh=0.2.1 OK
== git status (Cargo.toml/docs)
 M docs/install.md
 M docs/install.sh
== docs diff identical to pre-bump
docs-diff-identical
````

`Cargo.toml` never appears in `git status`, and the two docs files' diff after the
revert is byte-identical to their diff before the bump: no drift from the cycle.

Gates for this pass (`sh -n docs/install.sh` → `sh-n-ok`,
`scripts/gen_language_blocks.py --check` → 0, `make docs-check` → "No issues
found", `make docs-unit` → `Ran 16 tests … OK`, `make fast-check` → 0 with
`install-test: all cases passed`).

One environment note: `make docs-unit` first failed in
`test_a_package_with_no_binary_names_the_fix` because this worktree's untracked
`.venv/bin/blastit` is a stale `0.2.0` binary dated Nov 21 — `_blastit.py`'s
documented "beside the interpreter" fallback found it and answered `--version`
with `blastit 0.2.0` (exit 0). It is not a repo file (`git status python/` is
clean); the stale binary was moved out of the venv and the suite is green, so no
source change was involved.


## History deliberately left as records

Not configuration, so not touched — the list this slice verified rather than
"fixed":

* `scripts/dev-version.sh` (`0.1.1 -> 0.1.1-dev42`, "would no longer accept
  `0.1.1-dev42`", "`0.1.1-dev42`. maturin normalises…") and
  `scripts/check-release-tag.sh` ("`0.1.0-dev42` is a development wheel's
  version") — prose explaining an existing naming rule.
* `RELEASING.md`: "That documented exception is why `0.2.0`'s packages never
  built" (why a past release's packages are missing) and "for example, `0.2.0` →
  `v0.2.0`" (the tag scheme, a concrete illustration).
* `.github/workflows/publish-pypi.yml`'s "why v0.2.0's packages were never built",
  the `workers … removed in 0.2.0` refusal strings, `CHANGELOG.md`, ADRs,
  `plan/`, `docs/testing/*`, `e2e/php2shell.py`'s "a PHP manifest has no request
  audit in `0.2.0`": all outside this slice's paths, all records.

## Left as-is, and why

After the follow-up pass (§5) the only version in an executable file is the
generated `BLASTIT_DEFAULT_VERSION` line itself. The installer's two
`--version` usage errors name `1.2.3` as an example of the *token shape* they
accept (deliberately not a real release, so a grep cannot mistake a usage string
for the shipped version). What else remains is only:

* the exempt history above (`0.1.x-dev42` prose in `dev-version.sh` /
  `check-release-tag.sh`, `RELEASING.md`'s `0.2.0` tag illustration and its
  "why `0.2.0`'s packages never built" note);
* the concrete worked package name `blastit-php-8.4` in the packaging docs,
  which names a *language minor* row, not the workspace version.

## Follow-ups

None. The two earlier follow-ups (`e2e/run_packaging.py:5`,
`scripts/install_matrix.sh:4`) were fixed in the follow-up pass below once the
paths were extended.
