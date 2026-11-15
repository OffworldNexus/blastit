---
id: MB.3.59
title: "The Alpine libphp cannot be versioned by name: the boot refuses 'no shim can be chosen' unless BLASTIT_LIBPHP is versioned"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.3.58]
paths:
  - "crates/blastit/src/**"
  - "crates/blastit-php-sapi/src/**"
  - "crates/blastit/tests/**"
  - "e2e/php-sapi/**"
  - "scripts/**"
  - docs/frameworks/php.md
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T17:56"
updated: "2026-11-15T19:34"
---

# Goal

Resolve a `libphp`'s PHP minor **statically** — from the library's own bytes and the path it
is reached by — so the boot serves a perfectly good `libphp` whose *file name* states no
version, and so a library that genuinely cannot report a version is refused with a sentence
that says **that**. Stage 0 keeps its **zero `dlopen`** budget: nothing here may load the
library to ask it.

Three facts share one sentence today and they are not the same defect (`MB.3.58` measured
the first and third; the owner ruled they must be fixed before the release):

1. **Alpine's spelling parses as no candidate.** `crates/blastit/src/stage0.rs`'s parser
   (`PhpVersion::parse`) needs a **dot** between major and minor and a `libphp` prefix, so
   `/usr/lib/libphp83.so` (`8`+`3` packed, no dot) yields `None`, and
   `/usr/lib/php84/libphp.so` states its minor only in its **parent directory** — the name
   states nothing at all. `PhpRefusal::NoVersion` (`stage0.rs:821-826`) then says it
   outright: *"is an NTS libphp whose file name names no PHP major.minor, so no shim can be
   chosen for it; keep the distro name … or `BLASTIT_LIBPHP` with a versioned one"*.
2. **Discovery never reaches Alpine's directory.** `library_dirs()` (`stage0.rs:602-617`)
   lists `/usr/lib/<triplet>`, `/usr/lib64`, `/usr/lib`, `/usr/local/lib/<triplet>`,
   `/usr/local/lib`. There is no `/usr/lib/php84` (nor `/usr/lib/php83`), so with
   `BLASTIT_LIBPHP` **unset** the fixed candidates are never joined to the directory Alpine
   installs into: a correct name alone cannot find `/usr/lib/php84/libphp.so`.
3. **Debian 12's `libphp8.2` really has no `php_version`** (`dlsym` → `undefined symbol`;
   `nm -D` shows no definition), so no resolution strategy — static or not — can learn its
   version from the symbol. That leg must refuse — but with the *symbol* sentence, not the
   *file name* one.

# Context

Measured by `MB.3.58` attempt 2, in each leg's own image (the shim and the probe are
built there; the suites run there):

- musl 8.4 with `BLASTIT_LIBPHP` pointing at a **versioned** copy
  (`cp /usr/lib/php84/libphp.so /tmp/libphp8.4.so`): the worker serves —
  `16:34:05 INFO request GET / → app 200 17.6ms`. musl 8.3 likewise (`13.3ms`).
- musl 8.4/8.3 with Alpine's own name (`/usr/lib/php84/libphp.so`, `/usr/lib/libphp83.so`):
  the boot refuses with *"… whose file name names no PHP major.minor, so no shim can be
  chosen for it …"* — same shim, same library, same worker.
- musl 8.2 with a versioned copy (`/tmp/libphp8.2.so`): refuses with *"the loaded images
  have no `php_version`: Symbol not found: php_version"* — the honest, different sentence.
- Debian bookworm 8.2: `dlsym(libphp, "php_version")` → `undefined symbol`; the protected
  suite's boot refuses with the same symbol sentence.

So: nothing about the Alpine pair is broken except Blastit's opinion of its name and the
directories it looks in. The matrix currently records `reaches: probed` for musl 8.3/8.4 and
`refused` for 8.2 both libcs (`scripts/platform_matrix.py`, `SHIM_REACHES`); see `H-009` in
`plan/HUMAN.md`.

**The budget this node designs inside.** Stage 0 "never loads anything: the `libphp` is
opened, `fstat`ed and read as an ELF `.dynsym` table — `dlopen` happens inside a view worker,
never here" (`stage0.rs:899-902`), and `crates/blastit/tests/critical_php_embed.rs:5-6` pins
it: *"§3 puts the discovery in stage 0 with **zero `dlopen`**"*. Consequences the fix must
respect:

- The `.dynsym` walk can only say that `php_version` **is exported** (each symbol names), not
  what it **returns** — `php_version()` is a function whose returned string is only
  obtainable by calling it.
- The only place a *value* exists is stage 1's probe, which loads the pair on purpose
  (`blastit_php_sapi::Loaded::load`; `php_view.rs:658` compares the shim's
  `blastit_php_minor()` with the host's `php_version()`). That check already turns a wrong
  static answer into a boot refusal rather than a worker crash (`config.rs:645-651`: *"it is
  therefore what the file says it is"*).

# Acceptance

- **Alpine 8.4 serves through its own package name**: with `BLASTIT_LIBPHP` **unset**,
  `blastit dev` answers `GET / → 200` with the `libblastit_php84-x86_64-musl.so` shim it
  selects — reaching `/usr/lib/php84/libphp.so` through the discovery directory list, and the
  same for `/usr/lib/libphp83.so` on 8.3 through the packed name. The chosen shim's minor must
  equal the **host** `php_version()`'s minor (what stage 1's probe reads out of the loaded
  library), not a guess: a static resolution that disagrees is refused before the first
  request, with a sentence naming both minors.
- **8.2's refusal names the missing symbol**: the sentence says the library exports no
  `php_version`, so its minor cannot be determined — never "the file name names no
  major.minor" (a file name that *does* carry the minor must not change which sentence a
  symbol-less library gets).
- **The filename fast path still works and nothing new loads**: a versioned name
  (`libphp8.4.so`, `libphp-8.4.so`) resolves without the `.dynsym`/section read happening
  first, an unversioned name whose *path* or *library* does state the minor resolves without
  opening for load, and no new `dlopen` appears in stage 0 (the protected suite's stub-ELF
  test still proves discovery on a host with no PHP).
- **Re-measure and raise the matrix**: run `make php-sapi-e2e` + `make rs-test-php-image`
  on the Alpine 8.3/8.4 legs; if the boot serves, flip `reaches` to `served` in
  `scripts/platform_matrix.py` (`SHIM_REACHES`) and update the support paragraph in
  `docs/frameworks/php.md` §1 (`:50-62` — it currently says Alpine's names are refused and
  prescribes a `sudo install` to `/usr/local/lib`), and `--summary`/`--check` will print the
  new served count.
- `make check` exit 0 and `make e2e` exit 0. The Makefile target and the php-sapi image are
  **invoked** here, not edited: this node's edits stay in `crates/blastit/src/**`,
  `e2e/php-sapi/**`, `scripts/**` — plus `docs/frameworks/php.md` and the protected suites,
  which need the path widening noted below.

# Notes

- **Ordering**: `depends_on: [MB.3.58]`. The owner's sequence is `.58` attempt 3 → this
  node → re-run the matrix legs → `RELEASE_READY`.
- **The files this node changes** (one change, all four listed in `paths`):
  1. `crates/blastit/src/**` — `PhpVersion::parse`/`version_of`, `library_dirs`,
     `zend_abi`, `PhpRefusal::NoVersion`, the inline suite at `stage0.rs:4217`.
  2. `e2e/php-sapi/**`, 3. `scripts/**` — the matrix legs.
  4. **`docs/frameworks/php.md`** — the §1 support paragraph.
  5. **`crates/blastit/tests/**`** — the two protected suites
     (`critical_php_backend.rs`, `critical_php_embed.rs`) assert the refusal sentences
     (`critical_php_embed.rs:381-443` builds the stub `php_version`/`blastit_php_minor`) and
     `host_php()` there assumes the Debian spelling. The guard needs `tools/critical_guard.py
     --node MB.3.59`, or the sentence change has to keep every existing assertion true.
- **The fix is static, in this order** — no step may load the library:
  1. **Name fast path**, unchanged: `version_of` (`stage0.rs:1327-1338`) parses the file name,
     then the descriptor's `readlink` target.
  2. **The distro's packed/path spellings**: accept `libphp83.so`/`libphp84.so` (major and
     minor digits, no dot) and the parent directory component `php83`/`php84`, which is where
     Alpine's `/usr/lib/php84/libphp.so` actually states its minor. A path that states two
     different minors resolves to neither.
  3. **The object's own bytes**, when the path states nothing: the `.dynsym` walk already
     reads the file (`zend_abi` → `ZendAbi`), so record **whether `php_version` is exported** —
     its absence is the 8.2-and-earlier signature and is what the refusal must name — and, if
     the minor is still unknown, read the compile-time version string the object carries (the
     constant `php_version()` returns is static data in its sections) inside the same
     `MAX_SECTION_BYTES`-bounded walk. Two candidates that disagree, or none, is a refusal that
     names the path, the `.dynsym` facts and `BLASTIT_LIBPHP`. No heuristic that could pick a
     neighbouring library's version.
  4. **Companion discovery fix, same task**: `library_dirs()` (`stage0.rs:602-617`) gains the
     embed directories Alpine installs into (`/usr/lib/php84`, `/usr/lib/php83`, and the
     `/usr/local/lib/php8x` twins) in the `ADR-0010` §3 preference order — without them an
     unset `BLASTIT_LIBPHP` never joins a candidate name to `/usr/lib/php84` at all.
- **If step 3 cannot identify the minor statically** on a real host, the honest answer is a
  refusal that names the missing static fact and `BLASTIT_LIBPHP`, *not* a stage-0 load and
  *not* a probe-first bootstrap: the shim must be chosen before the probe starts, so the probe
  cannot supply the minor it is given. Whatever the choice, write it into `ADR-0010`
  §3/§4.1's wording in the same change so the invariant and the code agree.
- **Out of scope**: the *shim* side is untouched (`crates/blastit-php-sapi/**`); a wrong static
  answer stays a refusal in stage 1, never a crash.
