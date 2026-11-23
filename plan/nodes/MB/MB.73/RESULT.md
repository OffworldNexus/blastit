# MB.73 — every install leg installs the whole package set

## What changed (`e2e/run_packaging.py` only)

- **New `whole_set_files(package) -> list[Path]`**: the core artifact plus one
  sibling per `shim_packages()` row, named
  `f"{shim}{package.name.removeprefix('blastit')}"` (so `_<version>_<arch>.deb`
  and `-<version>-<release>.<arch>.rpm` both derive from the artifact the leg was
  handed). A missing sibling raises `LegFailure` naming that file, so a leg handed
  six of seven fails by name before it can install a partial set. The names come
  from `packaging/package-set.sh --shims` (itself derived from
  `packaging/languages.tsv`) at test time — no literal anywhere.
- **`leg_package()`** keeps the `docs/install.sh` step and its assertion that the
  installer chose exactly the runtime-runnable shims (`chosen` /
  `chosen_packages` / `chosen_payload` from `shim_probe()`), which is what the
  wsgi/php "the installer chose the shim" checks read. After it the leg installs
  the **whole** set by explicit `/pkg/<name>` path, weak deps off:
  - deb: `apt-get install -y --no-install-recommends <shims not already installed>`
  - rpm: one `dnf install -y --setopt=install_weak_deps=False <core + every shim>`
    transaction (replaces the old late glob block; `rpm_set_arch()` deleted, no
    other caller). A comment names `MB.66`'s `Recommends:` as what makes
    installing a shim whose runtime is absent a supported state.
- **Exact-set assertions**: `installed_packages()` must be exactly
  `{"blastit", *shim_packages()}` with `len == 1 + len(shims)`, naming the missing
  (and any unexpected) package; the `/usr/share/blastit` tree must be
  `CORE_SHARED` plus one `libblastit_*.so` per shim row. The old "was installed
  although this image has no runtime" branch is gone — every shim file is
  installed and the owning-package loop covers all of them.
- **One coverage line per leg**, gated on the `shim_probe()` result (`runnable`):
  `served through its shim: <python> (WSGI)` when the wsgi mode ran it,
  `runtime present, not loaded: <php> (no PHP fixture)`, and
  `runtime absent here, not exercised: …`; empty clauses are omitted.
- Docstring deb/rpm bullet, the `CORE_SHARED` comment, the `shim_probe()`
  comment and `leg_package`'s docstring updated where they no longer held. The
  reorder of `import languages` was `ruff --fix` import sorting inside the same
  file (no behaviour change).

## Derivation evidence

```
$ uv run --no-project e2e/run_packaging.py --print-legs
blastit	core	(none)	(none)	(none)
blastit-python-3.13	shim	libblastit_py313-*-gnu.so	python3.13	python3.13
blastit-python-3.14	shim	libblastit_py314-*-gnu.so	python3.14	python3.14
blastit-php-8.3	shim	libblastit_php83-*-gnu.so	libphp8.3-embed	php-embedded
blastit-php-8.4	shim	libblastit_php84-*-gnu.so	libphp8.4-embed	php-embedded
blastit-php-8.5	shim	libblastit_php85-*-gnu.so	libphp8.5-embed	php-embedded
blastit-php-8.6	shim	libblastit_php86-*-gnu.so	libphp8.6-embed	php-embedded
probe	python	3.14
probe	python	3.13
probe	php	8.6
probe	php	8.5
probe	php	8.4
probe	php	8.3
```

Helper, against a scratch directory with only four of the seven files:

```
NAMED: this leg holds no blastit-php-8.4_0.2.1_amd64.deb
full: 7 blastit-python-3.13_0.2.1_amd64.deb blastit-php-8.6_0.2.1_amd64.deb
```

`uvx --offline ruff check e2e/run_packaging.py` → `All checks passed!`

Docker legs are not run here; the lead runs `--leg deb` / `--leg rpm`.

## Review fixes

1. One naming derivation: `whole_set_files()` is the validator, so
   `files = whole_set_files(package)` and
   `shim_files = dict(zip(expected_shim_names, files[1:]))` are bound once and
   feed BOTH install lists (deb: every file but the core in one
   `apt-get install -y --no-install-recommends`; rpm: all files in one `dnf`
   transaction) and the `dpkg-deb --field` / `rpm -qp` header loops
   (`f"/pkg/{shim_files[shim].name}"`). The local `suffix` and every
   `f"/pkg/{shim}{suffix}"` re-derivation are gone.
2. The per-leg line no longer overstates: it is gated on `runnable` and says
   `served through its shim: <python> (WSGI)` only when the wsgi mode served it,
   `runtime present, not loaded: <php> (no PHP fixture)` for the package-level PHP
   proof, and `runtime absent here, not exercised: …` for the rest; empty clauses
   are omitted instead of printing `none`.
3. The tautological `len(packages) == len(expected_names)` clause is dropped
   (`installed_packages()` is deduped); the set-equality message now reads
   `N of M packages installed (…) : missing … ; unexpected …`.
4. `chosen` → `runnable` (the `shim_probe()` runtime truth),
   `chosen_packages`/`chosen_payload` → `installer_packages`/`installer_payload`
   (the pre-whole-set installation the wsgi/php proofs read), and the comment
   above them states exactly that split.


## Acceptance

`make fast-check` (see tail below). Out of scope, no follow-ups needed: the
negative proof and the per-image legs are the lead's `--leg deb`/`--leg rpm` run.

```
$ make fast-check    # review-fix run, EXIT=0
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.18s

sh scripts/test_install.sh
install-test: ok (h) apt installs the core first, each shim alone, and warns instead of aborting on a shim it cannot install
install-test: ok (i) the local-file path installs the core plus only the shim packages it has, and warns about the rest
install-test: ok (i) the rpm half passes --nosignature only with BLASTIT_PKG_ACCEPT_UNSIGNED=1
install-test: ok (j) gen_language_blocks.py --check: the committed blocks are the table's and Cargo.toml's version
install-test: all cases passed (fixture on http://127.0.0.1:8745, scratch /tmp/opencode/install-sh-test)
```

Only this RESULT.md changed after the gate; no source file was touched.
