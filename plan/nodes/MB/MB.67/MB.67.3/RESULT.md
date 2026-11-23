# MB.67.3 — Rust compile-time language tables

STATUS: done

## What changed

One include-only Rust loader plus four `build.rs` consumers. No new crate, no
dependency: `Cargo.toml`/`Cargo.lock` are untouched.

* `packaging/languages.rs.inc` (new) — `Row`, `parse`, `load`, `versions`
  (newest first, per numeric component), `digits` (`"3.13"`→`"313"`), `sentence`,
  `ascending`. `std` only; included by every `build.rs` and every freshness test.
* `crates/blastit-view-php` — `build.rs` (new) emits `$OUT_DIR/languages.rs`:
  `LIBPHP_CANDIDATES`, `LIBPHP_PROBES`, `PER_MINOR_DIRS`, `NEWEST_PHP`,
  `SUPPORTED_PHP`, `supported_php_minor`; `src/discovery.rs` `include!`s it,
  `LIBPHP_PROBES` keeps its public name (adds `name()`/`per_minor_dir()`
  accessors), `library_dirs()` walks `PER_MINOR_DIRS`, `PhpVersion::supported`
  calls the generated `matches!` helper.
* `crates/blastit-php-sapi` — `build.rs` generates the `Minor` enum, `parse`,
  `code` (= major×100+minor), `digits`, `dotted`, `config_names`,
  `SUPPORTED_MINORS`, `NEWEST_MINOR`, `CONFIG_NAMES`,
  `SUPPORTED_MINORS_SENTENCE`/`supported_minors_sentence()`; enum variants keep
  their historical (`Php83`-first) order so the derived `Ord` is unchanged.
  `CANDIDATES`/`SUPPORTED` and the "no toolchain" warning derive newest first;
  the never-fail-the-build rule and `cfg(php_toolchain)` are untouched. All
  refusal prose now calls the generated sentence or `NEWEST_MINOR`.
* `crates/blastit-py-shim` — `build.rs` derives the built minor from
  `PYO3_PYTHON` (unchanged) and panics, naming the declared list, if the TSV does
  not declare it; emits `SUPPORTED_PY_MINORS`; `blastit_py_minor()` unchanged.
* `crates/blastit-py-sapi` — `build.rs` (new) emits `SUPPORTED_MINORS`
  (`[14, 13]` today), `DEFAULT_MINOR`, `default_minor()`,
  `supported_minors_sentence()` and `PY_MINOR_<digits>`; `discovery/error.rs`
  refuses with those instead of literals; the `lib.rs` doctest uses
  `encode_py_minor(3, 13)` so withdrawing a row cannot break it.
* `crates/blastit/src/stage0/project_python.rs` — uses
  `blastit_py_sapi::supported_minors_sentence()` and
  `blastit_py_sapi::default_minor()`.
* Freshness tests: `crates/{blastit-view-php,blastit-php-sapi,blastit-py-shim,blastit-py-sapi}/tests/languages.rs`
  parse `packaging/languages.tsv` with the same fragment and compare every
  generated constant.

## Proof

`cargo fmt --all -- --check` → exit 0.

`cargo test -p blastit-view-php -p blastit-php-sapi -p blastit-py-shim -p blastit-py-sapi`
→ 13 `test result: ok` lines, 0 failed; tails:

```
running 5 tests   (view-php freshness)
...
test result: ok. 5 passed; 0 failed

running 3 tests   (py-sapi freshness)
test result: ok. 3 passed; 0 failed

running 1 test    (py-shim freshness)
test result: ok. 1 passed; 0 failed

Doc-tests blastit_php_sapi: 1 passed
Doc-tests blastit_py_sapi:  1 passed
```

`cargo test -p blastit --features python` → exit 0, 0 failed:

```
test result: ok. 24 passed
test result: ok. 21 passed
test result: ok. 25 passed
test result: ok. 29 passed
```

### Deliberately-wrong emission (verify the freshness test catches it)

`crates/blastit-py-sapi/build.rs` was temporarily changed to emit
`minor(version) + 1` for `SUPPORTED_MINORS`; reverted immediately after.

```
running 3 tests
test the_supported_minors_are_the_tsv_newest_first ... FAILED
---- the_supported_minors_are_the_tsv_newest_first stdout ----
assertion `left == right` failed
  left: [15, 14]
 right: [14, 13]
test result: FAILED. 2 passed; 1 failed
```

### No version table left in the source

```
$ grep -rn '8\.[3-6]' crates/blastit-view-php/src crates/blastit-php-sapi/src crates/blastit-py-shim/src --include=*.rs
crates/blastit-view-php/src/worker.rs:393:    /// The host `libphp`'s version string (`"8.4.27"`), for the boot log.
crates/blastit-view-php/src/probe.rs:109:/// `["8.4.27","/etc/php/8.4/embed/php.ini"]` (`null` when it opened none).
crates/blastit-view-php/src/probe.rs:199:/// The child sends one line, `blastit-probe runtime=["8.4.27", <ini or null>]`;
crates/blastit-view-php/src/probe.rs:278:/// version is the loaded library's own `php_version()`, the full `"8.4.27"` the
crates/blastit-view-php/src/discovery.rs:67:    /// a `libphp.so` there is the 8.4 library the *path* names.
crates/blastit-view-php/src/discovery.rs:388:/// `(PHP 8.4, x86_64-gnu)` and the shim that tuple needs.
crates/blastit-view-php/src/discovery.rs:491:                 chosen for it; give it one versioned name (libphp8.4.so, or the same minor \
crates/blastit-view-php/src/discovery.rs:504:             be chosen for it; give it a versioned name (libphp8.4.so, or the same minor \
crates/blastit-view-php/src/discovery.rs:788:/// Stage 0 may follow symlinks (`/usr/lib/libphp8.so -> libphp8.4.so` is a
crates/blastit-view-php/src/discovery.rs:1001:/// `libphp.so` is usually a symlink to `libphp8.4.so`, and a descriptor's link
crates/blastit-view-php/src/discovery.rs:1031:/// `libphp8.4.so`) and a per-minor parent directory (`/usr/lib/php84`,
crates/blastit-view-php/src/discovery.rs:1169:/// The function is `return PHP_VERSION;` in every PHP 8.3+ build: one
crates/blastit-view-php/src/discovery.rs:1172:/// `8.4.27` inside the `X-Powered-By: PHP/8.4.27` literal, which is why a
crates/blastit-view-php/src/discovery.rs:1224:/// `"8.4.27"` -> PHP 8.4: the first two dot-separated components.
crates/blastit-view-php/src/discovery.rs:1392:    /// `"8.4"`, for messages, `(PHP 8.4, …)` tuples and package names.
crates/blastit-view-php/src/discovery.rs:1398:    /// The version `"8.4"` states, the form the stage-1 document carries.
crates/blastit-php-sapi/src/lib.rs:49://! only inside `e2e/php-sapi/` (`debian:trixie-slim` + `php8.4-dev` +
crates/blastit-php-sapi/src/lib.rs:50://! `libphp8.4-embed`), by `make php-sapi-e2e`, which `dlopen`s the real
crates/blastit-php-sapi/src/probe/main.rs:10://! `php8.4-dev` + `libphp8.4-embed`), never by `make check`: a PHP-free host can
crates/blastit-php-sapi/src/toolchain.rs:350:/// `php-config8.4`, with no ELF header and no `#!`) are facts about this PATH
crates/blastit-php-sapi/src/loader.rs:345:    /// What `php_version()` reported, e.g. `"8.4.27"`.
crates/blastit-php-sapi/src/loader.rs:617:    /// The host `libphp`'s version string (`"8.4.27"`).
```

Every match is a single example in prose (a version string, or one spelling);
no table remains, and every list/sentence is generated from
`packaging/languages.tsv`.

`make fast-check` is the final gate of this slice (guard, fmt, clippy `-D warnings`
across the workspace including these `build.rs` files, rustdoc `-D warnings`,
workspace tests, Python lint and plan validation).

## Follow-ups

* `crates/blastit/tests/e2e_cli.rs::an_elf_candidate_runs_from_the_verified_descriptor`
  failed once on `ETXTBSY` ("Text file busy") and passed on rerun: the known
  transient-spawn flake the toolchain retry logic documents, not this slice.
* `packaging/languages.sh`, `scripts/rpm/blastit.spec` and docs/e2e consumers are
  `MB.67.1`/`MB.67.2`/`MB.67.4`.

## Review fixes

Consolidated round findings, all fixed:

1. **`NEWEST_PHP` is asserted.** `crates/blastit-view-php/tests/languages.rs`
   gained `the_newest_minor_is_the_first_row`
   (`assert_eq!(NEWEST_PHP, from_dotted(rows[0].version))`). Proof: emitting the
   **oldest** row as `NEWEST_PHP` turned it red, then reverted:
   ```
   left: PhpVersion { major: 8, minor: 3 }
   right: PhpVersion { major: 8, minor: 6 }
   test result: FAILED. 0 passed; 1 failed
   ```
2. **`PY_MINOR_<digits>` is asserted.** `the_py_minor_constants_are_the_rows`
   checks one generated constant per declared row against
   `encode_py_minor(major, minor)`. Proof: emitting `minor + 1` in the constants
   turned it red, then reverted:
   ```
   assertion `left == right` failed: PY_MINOR_313 is the row's encoding
     left: 314
    right: 313
   test result: FAILED. 0 passed; 1 failed
   ```
3. **Withdrawing a CPython row is no longer a build failure.** Removed the
   `pub use crate::{PY_MINOR_313, PY_MINOR_314};` re-export and the two
   `PY_MINOR_313`/`PY_MINOR_314` intra-doc links from `src/abi.rs` (they now say
   `PY_MINOR_<digits>` / `encode_py_minor`). Proof with the `python 3.13` row
   temporarily removed, then restored:
   ```
   cargo check -p blastit-py-sapi
       Compiling blastit-py-sapi v0.2.1
       Finished `dev` profile ... in 0.26s
   generated: pub const PY_MINOR_314: u32 = crate::abi::encode_py_minor(3, 14);
   ```
4. **Test headers corrected.** The four `tests/languages.rs` now cite `MB.67` and
   say they are generator-output verification (no `TESTING_PLAN.md` §7 claim).
   Follow-up: a human may add a §7 bullet for this surface if they want it
   recorded there; no docs were changed here (the doc lists are `MB.67.4`'s).
5. **`slug` is load-bearing.** `packaging/languages.rs.inc` gained `rows_of`,
   `Row::slug()` and `Row::digits()`. All four `build.rs` emit from `&Row`:
   per-minor dirs and shim infixes come from `row.slug()` (view-php
   `PER_MINOR_DIRS`/`php_slug`, php-sapi `Minor::slug()`/`shim_file`,
   py-sapi `py_slug`/`shim_file`), distro names (`libphp<digits>`,
   `php-config<version>`, `php<digits>`) from `row.version`/`digits`. Proof with a
   temporary `php84` → `php84x` slug rename (then reverted):
   ```
   view-php:  LibphpProbe { name: "libphp.so", per_minor_dir: Some("php84x") }
              PER_MINOR_DIRS = ["php86", "php85", "php84x", "php83"]
              (8, 4) => Some("php84x")
   php-sapi:  Self::Php84 => "php84x"
   ```
6. **Strictness only for the shipped artifact.** `blastit-py-shim/build.rs`
   panics on an undeclared minor only when `CARGO_FEATURE_EXTENSION_MODULE` and
   `PYO3_PYTHON` are both set; every other shape prints `cargo:warning`. Proof
   with the host's CPython 3.12:
   ```
   non-artifact: warning: ... is CPython 312, which packaging/languages.tsv does
                 not declare (CPython 3.13 and 3.14); ... the build continues
                 test result: ok. 2 passed
   artifact:     panicked: ... is CPython 312, and packaging/languages.tsv
                 declares only CPython 3.13 and 3.14
   ```
7. **`supports_py_minor` makes the table load-bearing.**
   `blastit-py-shim/build.rs` emits `const fn supports_py_minor(u32)` from the
   rows and, for the artifact shape, `const _: () =
   assert!(supports_py_minor(BUILT_PY_MINOR));` (`BUILT_PY_MINOR` is the generated
   numeric read by `blastit_py_minor()`). The freshness test also asserts the
   predicate is exactly the declared set.
8. **Sentences unchanged for the e2e asserts.** Generated from the restored TSV:
   ```
   pub const SUPPORTED_MINORS_SENTENCE: &str = "CPython 3.13 and 3.14";
   pub const SUPPORTED_MINORS_SENTENCE: &str = "PHP 8.3, 8.4, 8.5 and 8.6";
   pub const SUPPORTED_PHP: &str = "PHP 8.3, 8.4, 8.5 and 8.6";
   ```
   `e2e_dev.rs:1084` / `e2e_prod_refusals.rs:1121` ("CPython 3.13 and 3.14 are
   supported") still pass, as does `an_unsupported_php_minor_names_the_supported_set`.

Verification after the fixes: `cargo fmt --all -- --check` ok; the four crate
suites 13× `test result: ok` (view-php freshness now 6 tests, php-sapi 5,
py-sapi 5, py-shim 2, doctests 1+1); `cargo test -p blastit --features python`
exit 0 (`24/21/25/29 passed`); the two e2e refusal tests pass; `grep '8\.[3-6]'`
over the three `src` trees still shows only the 22 single prose examples.
`make fast-check` is the final gate of this round (guard, fmt, clippy `-D
warnings` incl. build scripts, rustdoc `-D warnings`, workspace tests, Python
lint, plan validation, install-test).

