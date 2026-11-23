# MB.67.4 — generated docs/installer blocks, e2e + php 8.7 propagation proof

STATUS: done

## What changed

* `scripts/gen_language_blocks.py` (new, stdlib, imports `scripts/languages.py` as
  the one parser). `--write` regenerates; `--check` exits 1 with a unified diff
  naming the file when a committed block is stale. It rewrites:
  * `docs/install.md` — `<!-- BEGIN/END generated:languages:apt -->`, `:dnf`,
    `:table`, `:sentence`. Each snippet is generated whole (the marker sits
    outside the fence, because an HTML comment inside one renders as text); the
    apt/dnf `case "$py"`/`case "$php"` arms, the core + one-row-per-TSV-row table
    and the "Shims ship for …" sentence all come from `packaging/languages.tsv`.
    Refusals read "the table below lists the versions Blastit ships" — never a
    typed range.
  * `docs/install.sh` — `# BEGIN/END generated:languages` around `detect_shims`'
    `for minor in …` loops, emitted at that function's indent (4).
* `docs/install.sh` — header/usage prose no longer spells `python3.13`/`3.14` or
  `libphp8.x`; it says `python3.<minor>`/`libphp<minor>` and points at the table.
* `scripts/test_install.sh` — `py_primary`/`py_other`/`py_slashed`/`php_primary`
  are read from `packaging/languages.sh` so cases (h)/(i) carry no literal
  minor; new case **(j)** runs `gen_language_blocks.py --check`.
* `Makefile` — `docs-check` now depends on a new `docs-blocks-check` target that
  runs `gen_language_blocks.py --check` before `zensical build --strict`.
* `e2e/run_packaging.py` — `SHIM_PROBE` is now `shim_probe()`, built from the
  loader; the PHP/WSGI shim expectations are `shim_for_deb_runtime(...)`; added
  `--print-legs` (no Docker).
* `e2e/php_sapi_probe.py` — `SUPPORTED_DIGITS` and the leg table come from the
  loader/matrix; added `--print-legs`.
* `e2e/universal_binary.py` — `LIBPHP_84` became `libphp_candidates(minor)`;
  added `--print-legs` (`--binary` is required only for a real run).
* Docs prose lists → pointers to `install.md#one-package-per-language-version`:
  `docs/getting-started.md`, `docs/index.md`, `docs/deploy/index.md`,
  `docs/security.md`, `docs/frameworks/php/index.md`,
  `docs/frameworks/python/index.md`, `docs/reference/settings.md`,
  `docs/troubleshooting.md` (the quoted `CPython 3.13 and 3.14 are supported`
  row now points at the boot's own sentence/table).
* `packaging/README.md`, `RELEASING.md`, `README.md`, `e2e/README.md` — version
  lists deleted, each pointing at `packaging/languages.tsv` with one worked
  example naming a single minor. Page lengths: `docs/install.md` 130 (was 130),
  `getting-started` 129, `index` 127, `deploy/index` 96, `frameworks/php` 129,
  `frameworks/python` 96, `troubleshooting` 128.

## Proof

### The generator and the installer are the table's

```
$ uv run --no-project scripts/gen_language_blocks.py --check
exit 0
$ sh scripts/test_install.sh
install-test: ok (h) apt installs the core first, each shim alone, and warns instead of aborting …
install-test: ok (i) the local-file path installs the core plus only the shim packages it has …
install-test: ok (i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are packaging/languages.tsv's
install-test: all cases passed (fixture on http://127.0.0.1:8745, …)
```

### Each e2e driver prints what it derived

```
$ uv run --no-project e2e/run_packaging.py --print-legs
blastit	core	(none)	(none)	(none)
blastit-python-3.13	shim	libblastit_py313-*-gnu.so	python3.13	python3.13
…
blastit-php-8.6	shim	libblastit_php86-*-gnu.so	libphp8.6-embed	php-embedded
probe	python	3.14 … probe	php	8.6 …

$ uv run --no-project e2e/php_sapi_probe.py --print-legs | head -2
8.6	gnu	x86_64	libblastit_php86-x86_64-gnu.so	debian:bookworm-slim	php8.6-dev	libphp8.6-embed	/usr/lib/libphp8.6.so	true
8.5	gnu	x86_64	libblastit_php85-x86_64-gnu.so	ubuntu:26.04	php8.5-dev	libphp8.5-embed	/usr/lib/libphp8.5.so	true

$ uv run --no-project e2e/universal_binary.py --print-legs | head -3
python	3.14
python	3.13
php	8.6
```

### Propagation: append `php 8.7` to the TSV, touch nothing else

Before: `package-set.sh --rows | grep php-8.7` → `(no php-8.7 row)`;
`--count shims --libc gnu --arch x86_64` → `4`; `run_packaging.py --print-legs |
grep php87` → `(no 8.7 leg)`; `--matrix packages|shims` name no 8.7.

```
$ printf 'php\t8.7\tphp87\tlibphp8.7-embed\tphp-embedded\n' >> packaging/languages.tsv

$ packaging/package-set.sh --rows | grep php-8.7
blastit-php-8.7	shim	libblastit_php87-*-gnu.so	libphp8.7-embed	php-embedded

$ uv run --no-project scripts/rpm/gen_spec.py | grep -A3 'blastit-php-8.7'
# `blastit-php-8.7` `Requires: glibc >= %{blastit_glibc_php87}` (2.17), so a
…
%package -n blastit-php-8.7
Summary: PHP 8.7 embedded SAPI shim for blastit
Requires: glibc >= %{blastit_glibc_php87}
…
%files -n blastit-php-8.7
%{_datadir}/blastit/lib/libblastit_php87-*.so

$ scripts/platform_matrix.py --matrix packages | grep -o 'blastit-php-8\.7' | sort -u
blastit-php-8.7
$ scripts/platform_matrix.py --matrix shims | grep -o '"name":"[^"]*php87"' | sort -u
"name":"aarch64-gnu-php87"  "name":"aarch64-musl-php87"
"name":"x86_64-gnu-php87"   "name":"x86_64-musl-php87"
$ scripts/platform_matrix.py --count shims --libc gnu --arch x86_64
5

$ uv run --no-project e2e/run_packaging.py --print-legs | grep php87
blastit-php-8.7	shim	libblastit_php87-*-gnu.so	libphp8.7-embed	php-embedded
$ uv run --no-project e2e/universal_binary.py --print-legs | grep '^libphp	8.7'
libphp	8.7	/usr/lib/libphp8.7.so
libphp	8.7	/usr/local/lib/libphp8.7.so
libphp	8.7	/usr/lib/php87/libphp.so

$ cargo test -p blastit-view-php -p blastit-php-sapi
running 5 tests  (blastit-php-sapi freshness)   test result: ok. 5 passed; 0 failed
running 6 tests  (blastit-view-php freshness)   test result: ok. 6 passed; 0 failed
running 1 test   (blastit_php_sapi doctest)     test result: ok. 1 passed; 0 failed
$ grep -h -o 'Php87\|8\.7\|php87\|libphp8\.7\.so' target/debug/build/*/*/out/languages.rs | sort | uniq -c
      8 8.7
      2 libphp8.7.so
      6 php87
      9 Php87
```

`--matrix install` **does not change**, because it declares distributions, not
interpreter versions: `12 install legs across debian-12, debian-13,
fedora-latest, rocky-9, ubuntu-22.04, ubuntu-24.04; php87 mentions: 0`. Its
installed package set is proved by `run_packaging.py --print-legs` above (and by
`--leg install`/`--leg deb|rpm` in CI, which read the same names).

`gen_language_blocks.py --check` then FAILS with exactly the 8.7 lines:

```
--- a/docs/install.md
-case "$php" in 8.3|8.4|8.5|8.6) apt-get install "blastit-php-$php" ;; *) …
+case "$php" in 8.3|8.4|8.5|8.6|8.7) apt-get install "blastit-php-$php" ;; *) …
…
-Shims ship for **CPython 3.13 and 3.14, and PHP 8.3, 8.4, 8.5 and 8.6**, by design: …
+Shims ship for **CPython 3.13 and 3.14, and PHP 8.3, 8.4, 8.5, 8.6 and 8.7**, by design: …
…
+| `blastit-php-8.7` | the PHP shim for NTS `libphp` 8.7 | `blastit (= <version>)` | `libphp8.7-embed` |
--- a/docs/install.sh
-    for minor in 8.3 8.4 8.5 8.6; do
+    for minor in 8.3 8.4 8.5 8.6 8.7; do
generated-blocks: docs/install.md is stale: run `scripts/gen_language_blocks.py --write`
generated-blocks: docs/install.sh is stale: run `scripts/gen_language_blocks.py --write`
```

After `--write`, the before/after diff of the docs is **only** those 8.7 lines
(two generated `case` arms, the sentence, the one table row, the installer loop);
`--check` is green again. Reverting the TSV row and re-running `--write` restores
the files byte-for-byte (`cmp` against the pre-append copies), `packaging/languages.tsv`
is absent from `git status`, and `gen_spec.py --check` is green.

### Removal: delete the `php 8.5` row

```
$ grep -c 'blastit-php-8.5'   (package-set.sh --names)   -> 0
names: blastit blastit-python-3.13 blastit-python-3.14 blastit-php-8.3 blastit-php-8.4 blastit-php-8.6
$ scripts/platform_matrix.py --matrix packages | grep -o 'blastit-php-8\.5'  -> (nothing)
$ scripts/platform_matrix.py --matrix shims | grep -c php85                 -> 0
$ scripts/platform_matrix.py --count shims --libc gnu --arch x86_64         -> 3 (was 4)
$ uv run --no-project scripts/rpm/gen_spec.py --check
-# Packs a staged Blastit bundle (`make install PREFIX=<dir>`) as seven RPM
+# Packs a staged Blastit bundle (`make install PREFIX=<dir>`) as six RPM
-# are `core`, `py313`, `py314`, `php83`, `php84`, `php85`, `php86`. …
+# are `core`, `py313`, `py314`, `php83`, `php84`, `php86`. …
exit 1
$ uv run --no-project scripts/gen_language_blocks.py --check   (exit 1)
-case "$php" in 8.3|8.4|8.5|8.6) …
+case "$php" in 8.3|8.4|8.6) …
-| `blastit-php-8.5` | the PHP shim for NTS `libphp` 8.5 | …
-    for minor in 8.3 8.4 8.5 8.6; do
```

After `--write` the docs table is 6 rows (core + 5 shims, was 7). Restoring the
row and regenerating: docs identical to baseline, `gen_spec.py --check` and
`gen_language_blocks.py --check` green, `git status --porcelain packaging/
scripts/rpm/ docs/` lists only this slice's intended files (no TSV/spec drift).

### Gates

```
$ make docs-check
uv run --no-project scripts/gen_language_blocks.py --check
uvx --offline zensical build --strict
No issues found
Build finished in 0.52s

$ sh scripts/test_install.sh
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch …)

$ make fast-check
… e2e_cli: test result: ok. 21 passed; 0 failed
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch …)
fast-check exit=0

$ uv run --no-project tools/plan.py validate
plan OK
```

`git status --porcelain` after all of it lists only this slice's files (no
`packaging/languages.tsv`, no `scripts/rpm/blastit.spec`, no generated-doc
drift).

## Follow-ups

* `--matrix packages` lists the 8.7 **name** in each leg's `set` rather than
  gaining a leg (a package leg is arch × format, by design); the 8.7 shim legs
  are the `--matrix shims` rows.
* `docs/install.md` is at 130 lines: the four markers plus one table row per TSV
  row cost ~12 lines, paid back by folding the old prose (no page grew).
* `e2e/run_prod.py` carries no interpreter-minor expectation (its PHP image is a
  `debian:trixie` fixture), so it needed no `--print-legs`.

## Review fixes

1. **`docs/frameworks/index.md` no longer enumerates.** The Python cell says
   "`blastit` + one of the CPython minors in the [install table]" and the PHP
   cell "one shim per minor the [install table] lists"; `make docs-check`
   (`--strict`) resolves both anchors.
2. **The `!!! note` restores runnable per-distro commands.** Ubuntu:
   `sudo add-apt-repository ppa:ondrej/php`; Debian: a keyring-signed source
   (`deb https://packages.sury.org/php/ bookworm main` in a `.sources` file,
   `Signed-By` your copy of `https://packages.sury.org/php/apt.gpg`), then
   `sudo apt-get update` and `sudo apt-get install blastit-php-<minor>`. The page
   stays at **130 lines** by folding four paragraphs (the `nightly` sentence, the
   `--version` sentence, the per-project-pins paragraph and the shim paragraph);
   the three-step lead, the Mermaid diagram, both `!!!` notes and all four
   generated blocks are untouched.
3. **The table's `Recommends` is deb / rpm.** Header `` `Recommends` (deb / rpm) ``;
   a PHP row reads `` `libphp8.6-embed` / `php-embedded` ``; the core row stays
   `—`.
4. **The Python refusal is guarded** — `no WSGI shim for CPython ${py:-no
   python3}: …` — like the PHP branch, so a host with no `python3` still names
   what was asked for.
5. **`e2e/README.md` no longer overclaims.** The shim/package *names the driver
   asserts* derive from the TSV (and `--print-legs` prints them); each Docker
   leg's *provisioned host runtime* (`libpython3.13`, `libphp8.4-embed`) is a
   named fixture, so withdrawing such a row is a deliberate fixture change.
6. **`e2e/run_packaging.py` says "the package set (core plus one per shim row)"**
   in all four former "seven packages" spots, and the docstring's "every name"
   claim is narrowed the same way as (5).
7. **`docs/install.sh`** — `--help` names the full
   `https://offworldnexus.github.io/blastit/install/#one-package-per-language-version`
   URL, and the committed block comment is reader-facing: "The shim versions ship
   with Blastit; this list is generated — do not edit." (markers unchanged).
8. **`docs/install.md` explains `<version>` once**: "the release it was built
   against, e.g. 0.2.1, resolved by apt/dnf and never typed by hand".
9. **The generated `Depends` matches `packaging/README.md`**: a shim row is
   `` `blastit (= <version>)`, measured `libc6` ``.
10. **`docs/troubleshooting.md`** symptom is a searchable literal —
    `` `CPython … is not supported for WSGI views` `` — and the dangling "the boot
    prints it supports" is gone ("a minor the boot lists as supported (the same
    list as the install table)").
11. **`scripts/test_install.sh`** interpolates `$py_primary` in the (h) failure
    message and the (i) comment now says "the oldest PHP" (it uses the
    first/oldest row).

Re-proof against the reviewed generator:

```
$ printf 'php\t8.7\tphp87\tlibphp8.7-embed\tphp-embedded\n' >> packaging/languages.tsv
$ uv run --no-project scripts/gen_language_blocks.py --check
+Shims ship for **CPython 3.13 and 3.14, and PHP 8.3, 8.4, 8.5, 8.6 and 8.7**, …
+| `blastit-php-8.7` | the PHP shim for NTS `libphp` 8.7 | `blastit (= <version>)`, measured `libc6` | `libphp8.7-embed` / `php-embedded` |
$ uv run --no-project scripts/gen_language_blocks.py --write   # then revert the row
   install.md/install.sh byte-identical to the pre-append copy (cmp)

# removing `php 8.5`
-Shims ship for **… PHP 8.3, 8.4, 8.5 and 8.6** …
+Shims ship for **… PHP 8.3, 8.4 and 8.6** …
-| `blastit-php-8.5` | … | `libphp8.5-embed` / `php-embedded` |
   docs table 7 rows -> 6, docs/install.md 130 -> 129 lines, no `8.5` left
   revert + --write: byte-identical, --check green, `git status packaging/ scripts/rpm/`
   shows no TSV/spec drift
```

Gates this round (after the fixes): `make docs-check` → `No issues found`
(includes `gen_language_blocks.py --check`); `sh scripts/test_install.sh` → all
cases passed including (j); `make fast-check` and `tools/plan.py validate` close
the round.
