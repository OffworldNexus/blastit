# RESULT — MB.67.2

STATUS: done

Every CI/packaging matrix and the RPM spec are now **generated** from
`packaging/languages.tsv`; adding or removing a row changes the artifact set with
no second edit. The five `--matrix` outputs are byte-identical on the unchanged
tree.

## Files

- `scripts/platform_matrix.py` — imports `scripts/languages.py` (one parse at
  import, `LANGUAGE_ROWS`); `PY_MINORS`/`PHP_MINORS` are `{version: slug}`
  newest-first from `languages.versions_of`, `SMOKE_PYTHONS` is
  `("3.12", *ascending minors)`, `PACKAGE_SET = languages.names(rows)` feeds both
  `PACKAGE_FORMATS[*]["set"]`, and the measured build records live in
  `PHP_LEG_DATA` behind `_php_leg(minor, libc)` — a measured entry wins, a newly
  declared minor gets the documented convention (gnu: `debian:bookworm-slim` +
  `sury`, `php<v>-dev`, `libphp<v>-embed`, `php<v>-curl`, `/usr/lib/libphp<v>.so`;
  musl: `alpine:edge` + `alpine-edge-testing`, `php<d>-{dev,embed,curl}`,
  `/usr/lib/php<d>/libphp.so`) with `suites: False` +
  `suites_because = "… newly declared … no leg has built it yet …"`.
  `--check` drops "PHP_BUILDS names an unknown minor", and adds (a) a stale
  `PHP_LEG_DATA` key plus the existing `PROVEN`/`SHIM_REACHES` invariants, (b) a
  matrix leg (including the hand-spelled `IMAGES`) naming a version absent from
  the TSV, (c) `scripts/rpm/gen_spec.py --check` via `subprocess`, reporting its
  diff. Every other invariant (LEGS, globs, PROVEN keys, install legs, LSP
  targets) is kept; the version lists inside it (`8.3`–`8.6`, `3.13/3.14`, "the
  seven-package set", shim-name regexes) are now derived or shape-only.
- `scripts/rpm/gen_spec.py` (new, stdlib) — emits `scripts/rpm/blastit.spec`
  byte-identically: header tag list, `blastit_glibc_<slug>`/`blastit_libgcc_<slug>`
  defaults (core + one per row, 2.17/0), `%package -n`/`Summary:`/`Requires:`/
  `Recommends:`/`%description -n` in TSV order, `%files -n` globs; static
  plumbing (`Name`/`Version`/`Release`, `%install`, core `%files`) verbatim.
  Modes: default/`--stdout`, `--write`, `--check` (exit 1 + unified diff naming
  the file).
- `scripts/rpm/blastit.spec` — regenerated; byte-identical at the time of the
  slice (−0 lines), then review fix 5 moved its worked example from the
  hand-spelled `8.4` to the newest PHP row (`8.6`), a 2-comment-line diff.
- `packaging/package-set.sh` — `check_spec` and its call deleted; `--check`
  validates the table only and its doc comment/usage say the spec freshness gate
  is `scripts/rpm/gen_spec.py --check`, run by `platform_matrix.py --check`.
- `.github/workflows/{packages,nightly,php-sapi,py-sapi}.yml` — the literal
  package count (`-lt 7`, "the seven package names") is now
  `expected=$(packaging/package-set.sh --names | grep -c '')`; the prose minor
  lists (`8.3–8.6`, `CPython 3.13/3.14`, `(16 legs)`) and the "seven packages"
  phrasings now point at the TSV/matrix. No workflow spells a minor or a count.

## Proof

### 1. Five matrices: before == after (byte-identical)

```
$ for m in shims py-shims packages install wheels; do
    uv run --no-project scripts/platform_matrix.py --matrix $m > /tmp/mb67/before-$m.json; done   # before editing
... after ...
EMPTY shims
EMPTY py-shims
EMPTY packages
EMPTY install
EMPTY wheels
```

### 2. The spec is generated, and the committed file is already the output

```
$ uv run --no-project scripts/rpm/gen_spec.py --write
gen_spec: wrote scripts/rpm/blastit.spec
$ git diff --stat scripts/rpm/blastit.spec
                       (empty — byte-identical)
$ uv run --no-project scripts/rpm/gen_spec.py --check ; echo $?
0
```

### 3. A stale committed spec fails with a diff (one-character TSV edit, then revert)

```
$ # packaging/languages.tsv: `python 3.13` -> `python 3.12`
$ uv run --no-project scripts/rpm/gen_spec.py --check ; echo "stale-exit=$?"
--- a/scripts/rpm/blastit.spec
+++ b/scripts/rpm/blastit.spec
@@ -92,8 +92,8 @@
-%package -n blastit-python-3.13
-Summary: CPython 3.13 view shim for blastit
+%package -n blastit-python-3.12
+Summary: CPython 3.12 view shim for blastit
...
stale-exit=1
$ # reverted -> gen_spec.py --check exits 0 again
```

### 4. Propagation: a declared `php 8.7` row, no other edit

```
$ # packaging/languages.tsv += "php	8.7	php87	libphp8.7-embed	php-embedded"
$ uv run --no-project scripts/platform_matrix.py --count shims
20
$ uv run --no-project scripts/platform_matrix.py --matrix shims | grep -o '"name":"[^"]*php87"'
"name":"x86_64-gnu-php87"
"name":"x86_64-musl-php87"
"name":"aarch64-gnu-php87"
"name":"aarch64-musl-php87"
$ uv run --no-project scripts/platform_matrix.py --leg shims --minor 8.7 --libc gnu --arch x86_64
name=x86_64-gnu-php87
shim=libblastit_php87-x86_64-gnu.so
base=debian:bookworm-slim
dev=php8.7-dev
embed=libphp8.7-embed
curl=php8.7-curl
libphp=/usr/lib/libphp8.7.so
extra_repo=sury
suites=false
reaches=
$ uv run --no-project scripts/platform_matrix.py --matrix packages | grep -o '"set":\[[^]]*\]'
"set":["blastit",...,"blastit-php-8.6","blastit-php-8.7"]
$ uv run --no-project scripts/platform_matrix.py --matrix py-shims | grep -c php87
0
$ uv run --no-project scripts/rpm/gen_spec.py --stdout | grep -A3 'blastit-php-8.7'
%package -n blastit-php-8.7
Summary: PHP 8.7 embedded SAPI shim for blastit
Requires: glibc >= %{blastit_glibc_php87}
%if "%{blastit_libgcc_php87}" == "1"
--
%description -n blastit-php-8.7
The libblastit_php87 shim lets blastit run PHP view scripts through the host's
embedded NTS libphp 8.7. It is loaded by the blastit binary; install it together
with blastit and a PHP 8.7 embedded build, never instead of it.
--
%files -n blastit-php-8.7
%{_datadir}/blastit/lib/libblastit_php87-*.so
```

With the row still present, `platform_matrix.py --check` reports the *committed*
spec as stale ("`scripts/rpm/gen_spec.py --write` regenerates it", plus the
`seven`→`eight`, tag-list, macro and stanza hunks) — the freshness gate firing on
the real path. Row reverted; `--check` green again.

### 5. Gates

```
$ uv run --no-project scripts/platform_matrix.py --check
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared; ... 4 packages installed in 12 container legs; reaches: served 8 of 16 shims, 0 probed, 0 refused, 8 unbuilt; 16 served once the aarch64 legs run; 0 leg(s) dropped
$ uv run --no-project scripts/rpm/gen_spec.py --check ; echo $?
0
$ bash packaging/package-set.sh --check
package-set: ok (7 packages; packaging/languages.tsv is consistent)
$ git status --porcelain
 M .github/workflows/nightly.yml
 M .github/workflows/packages.yml
 M .github/workflows/php-sapi.yml
 M .github/workflows/py-sapi.yml
 M packaging/package-set.sh
 M scripts/platform_matrix.py
?? scripts/rpm/gen_spec.py
    (plan/* entries in the tree are the plan tool's own, not this slice)
```

`packaging/languages.tsv` and `scripts/rpm/blastit.spec` are unmodified after the
revert + regeneration.

### 6. `make fast-check`

```
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s
install-test: ok (a) --user installs into a scratch prefix; blastit --version runs (blastit 0.2.0 (fixture))
...
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

## Review fixes

1. **`SMOKE_PYTHONS` is derived, deduped and floor-checked.** It is
   `tuple(dict.fromkeys(("3.12", *sorted(PY_MINORS, key=languages.version_key))))`,
   and `check()` gains two problems: a declared CPython minor below `3.12`, and a
   duplicate smoke image. A declared `python 3.12` row no longer repeats the
   image, a `python 3.11` row fails `--check` by name:

   ```
   # packaging/languages.tsv += "python 3.12 py312 python3.12 python3.12"
   $ ... --matrix wheels | grep -o '"smoke_pythons":"[^"]*"'
   "smoke_pythons":"3.12,3.13,3.14"          # once, not "3.12,3.12,…"
   $ ... --matrix py-shims | grep -o '"name":"x86_64-gnu-py312"'
   "name":"x86_64-gnu-py312"
   # row changed to 3.11:
   $ ... --check
   platform-matrix: packaging/languages.tsv declares CPython 3.11, below the
   `py3-none` wheel's floor 3.12: the docs promise CPython >=3.12, so this shim
   could never be smoked by the wheel that ships it
   ```

2. **The package-set shim cross-check is keyed by package name and uses the
   row's own slug** (`languages.shim_glob(row.slug)`), never "Python → py name,
   everything else → PHP name". A third-language row demonstrates it:

   ```
   # packaging/languages.tsv += "lua 5.4 lua54 lua5.4 lua"
   $ packaging/package-set.sh --shims | tail -1
   blastit-lua-5.4	libblastit_lua54-*-gnu.so
   $ ... --check 2>&1 | grep -i lua | grep -v "refuses this table"
   (no lua complaint about a PHP/CPython name; the one reported problem is #6's)
   ```

3. **The tautological "leg names an undeclared version" loops over
   `shim_legs()`/`py_shim_legs()` are gone** — they are built from
   `PHP_MINORS`/`PY_MINORS` and could never fire. The `IMAGES` loop and its
   comment are kept as the guard for the one hand-spelled leg list.

4. **`PACKAGE_SET` is no longer aliased into both formats**: each entry holds
   its own `tuple(languages.names(LANGUAGE_ROWS))`, like the two independent
   tuples in the base file.

5. **`gen_spec.py`'s prose is rendered from the rows.** The worked example is
   the oldest Python row and the newest PHP row, so withdrawing a version cannot
   leave a fresh spec naming a package/macro that no longer exists. That changed
   two comment lines (the example moved from the hand-spelled 8.4 to the newest
   row, 8.6), regenerated with `--write`:

   ```
   $ .../gen_spec.py --write && .../gen_spec.py --check ; echo $?
   0
   $ git diff --stat scripts/rpm/blastit.spec
    scripts/rpm/blastit.spec | 4 ++--
    1 file changed, 2 insertions(+), 2 deletions(-)
   # with a declared php 8.7 the same paragraph and macro follow it:
   # `blastit-php-8.7` `Requires: glibc >= %{blastit_glibc_php87}` (2.17), so a
   %{!?blastit_glibc_php87:%global blastit_glibc_php87 2.17}
   ```

6. **`gen_spec.py` separates "cannot render this table" from "stale"**: exit
   `3` with a reason for a language it has no template for, and
   `platform_matrix.py --check` reports it as a refusal, not as a `--write`
   dead end:

   ```
   $ .../gen_spec.py --check ; echo "gen-spec-exit=$?"
   gen_spec: cannot render packaging/languages.tsv: lua: no RPM package template
   for this language (known: php, python)
   gen-spec-exit=3
   $ .../platform_matrix.py --check
   platform-matrix: scripts/rpm/gen_spec.py refuses this table: gen_spec: cannot
   render packaging/languages.tsv: lua: no RPM package template …
   ```

7. **Both loaders validate the runtime columns** (they are `Recommends:`
   values): leading/trailing whitespace and any `%`, `#`, `{`, `}` are refused
   with the existing `file:line` style. Proven on scratch copies in
   `/tmp/mb67` (reverted; the real table passes both):

   ```
   $ .../languages.py --check --tsv /tmp/mb67/bad-space.tsv
   /tmp/mb67/bad-space.tsv:4: blastit-php-8.6 has rpm-runtime 'php-embedded ' with
   leading or trailing whitespace: it is a `Recommends:` value, not prose
   $ BLASTIT_LANGUAGES_TSV=/tmp/mb67/bad-macro.tsv packaging/languages.sh --check
   languages: /tmp/mb67/bad-macro.tsv:4: blastit-php-8.6 has runtime
   '%{blastit_php_runtime}' containing % # { or }: a runtime column is a package
   name for Recommends:, never spec/control syntax
   $ .../languages.py --check          -> languages: ok (6 language rows: 4 php, 2 python)
   $ packaging/languages.sh --check    -> languages: ok (6 language rows in packaging/languages.tsv)
   ```

Fix 5 leftover closed: the generated `Recommends:` paragraph renders its CPython example from the newest `python` row (one rule, same as PHP), so withdrawing a row moves it with no generator edit; the Rocky-9 "it ships 3.9" measurement is untouched, and the regenerated spec passes `gen_spec.py --check` and `platform_matrix.py --check`.

Re-verified after the fixes (no cargo/make/docker): all five `--matrix` outputs
still equal the `/tmp/mb67/before-*` baselines (`EMPTY shims|py-shims|packages|
install|wheels`), `gen_spec.py --check` and `platform_matrix.py --check` green,
`package-set.sh --check` green, both language loaders green, and the `php 8.7`
add/revert snippet still works (`--matrix packages` gains `blastit-php-8.7`,
`--matrix shims` gains the four `php87` legs, `gen_spec.py --stdout` gains the
`%package`, `Recommends:`, `%files` and macro stanzas). `packaging/languages.tsv`
is back to its committed bytes (`git status` clean for it).

## Follow-ups

- `docs/design/backend-runtimes.md` §"scripts/platform_matrix.py is the record"
  still names `PHP_BUILDS`; it is `PHP_LEG_DATA` + `_php_leg` now (`MB.67.4`).
- `gen_spec.py` has templates for `python` and `php` only: a third language row
  fails loudly ("no RPM package template for this language") instead of guessing
  a `Summary:` — extend `_TEMPLATES` when one is declared.
- `nightly.yml`'s "newest seven nightlies" is artifact retention, unrelated to
  the language table; left alone.
