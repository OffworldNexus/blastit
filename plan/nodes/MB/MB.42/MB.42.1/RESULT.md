# MB.42.1 Result — Universal Distribution ADR-0017 & Blueprint

## Files Written / Updated
- `docs/decisions/ADR-0017-universal-distribution.md` (`Status: Accepted`, 44 lines)
- `docs/decisions/README.md` (registered `ADR-0017`)
- `docs/design/universal-distribution.md` (141 lines, `<200`-line technical blueprint for `MB.42.2`–`MB.42.5`)
- `ARCHITECTURE.md` (§2, §2.1 Distribution, §2.7 crate table, §9.1 item 6, and PHP `8.3–8.6` range aligned with `ADR-0017`)

## Consolidated Review Resolution (`review-arch` + `review-ux`)
- **`B1` (Stated & measured glibc floors):** `blastit`, `blastit-lsp`, and `gnu` `libblastit_py*.so` build in `quay.io/pypa/manylinux_2_28_<arch>` (`--max-glibc 2_28`); `scripts/check-portable-elf.sh` gains `--max-glibc <ver>` (`readelf --version-info` `.gnu.version_r`); `build-{deb,rpm}.sh` check every ELF against `LIBCS["gnu"]["shim_floor"]` and stamp `Depends: libc6 (>= <measured>)` / `Requires: glibc >= <measured>` from the bundle's measured max `GLIBC_X.Y`, stating the 2.28-sysroot trade-off explicitly (`design §1.2, §6.1`, `ADR-0017 §6`).
- **`B2` (`py3-none-<platform>` wheel builder & consumer contract):** Specified `maturin build --bindings bin` + `scripts/repack_universal_wheel.py` relocating `blastit`, `blastit-lsp`, and all `6` shims into `blastit/_bin/` (`0755`) with `Root-Is-Purelib: false` + `Tag:` + `RECORD`, exact `wheel_legs()` keys, and multi-interpreter smoke testing across `python:3.12`, `3.13`, and `3.14` (`design §6.1`).
- **`UX-B1` (`install.sh` `/dev/tty` interactivity):** `stdin` is never consulted (`[ -t 0 ]` forbidden because fd `0` carries the `curl | sh` script); interactivity, confirmations, and `sudo` prompts use `/dev/tty` and notes write to `stderr` (`design §6.2`, `ADR-0017 §6`).
- **`M1` (Protected `test_lsp_stdio.py` & `test_check_contract.py`):** `Makefile` `test-critical` stages `target/debug/blastit` at `$(CRITICAL_STAGE)/blastit/_bin/blastit` (plus symlink `_bin/blastit-dbproxy -> blastit`) and `_blastit.main(argv, executable, dbproxy)` `os.execvpe`s `bin_path()` so every protected Python test passes with zero edits (`design §3`).
- **`M2` (12-op `--internal-json` contract table):** Added the full 12-row `{op, Python signature, JSON args keys, result type}` table + `ValueError` rule (`design §3`).
- **`M3` (Single audit owner):** Consolidated both `crates/blastit-py/src/audit.rs` and `crates/blastit/src/audit_v1.rs` into `crates/blastit-audit`, leaving `cli/internal_json.rs` as a `<150`-LOC dispatcher (`design §1.1`, `ADR-0017 §4`).
- **`M4` (`BLASTIT_RELEASE_BASE_URL` SHA-256 verification):** Default GitHub downloads verify against the embedded SHA-256 table; overridden `BLASTIT_RELEASE_BASE_URL` requires `BLASTIT_SHA256` or local `BLASTIT_SHA256SUMS_FILE` and fails closed when absent in both `install.sh` and Composer (`design §6.1, §6.2, R9`).
- **`UX-M2` (Machine install vs per-project pins):** Documented single machine install vs `uv add --dev` / `composer require --dev` per-project pins upgraded by `uv lock --upgrade` / `composer update` (`design §6.1`, `ADR-0017 §6`).
- **`UX-M3` (No `uv run blastit` rule):** Added `R12` forbidding `uv run blastit` / `uv run --project` across all user pages, enforced by `docs_fences.rs` / `runnable_docs` (`design §7`).
- **`UX-M4` (Distinct `BLASTIT_PYTHON` & free-threaded `3.13t`/`3.14t` refusals):** Specified separate exact sentences for invalid `BLASTIT_PYTHON`, free-threaded `3.13t`/`3.14t`, and unsupported minors `3.12`/`3.15` (`design §2.2, §2.3`).
- **`UX-M5` (Minor-aware `PHP_PACKAGES`):** `php_packages_hint(Option<PhpVersion>)` substitutes `<M.m>`/`<Mm>` (`8.6`/`86` default) in one line (`design §5.2`).
- **`UX-M6` (`ARCHITECTURE.md` alignment):** Updated `:62–66`, `:178–186`, `:561–576`, and `:1887–1894` (`blastit-py` row replaced by `blastit-py-sapi`, `blastit-py-shim`, `blastit-audit`; `blastit-dbproxy` library-only; shared `BLASTIT_EXEC` model).
- **Minors:** Auto-discovered unsafe handoff candidates emit 1 `stderr` warning and continue probing while explicit `$VIRTUAL_ENV` exits `1`, silent on success + user-predictable handoff sentence (`design §4`); documented `(st_dev, st_ino)` in `blastit_sys::pin` vs `EntryPin` in `blastit-view-php::pinning` (`design §1.1`); retired `blastit-dbproxy` `[[bin]]` (`design §1.1`); pinned `debian:bookworm-slim`/`alpine:edge` multi-arch digests and Surý GPG fingerprint `15058500A0235D97F5D10063B188E2B695BD4743` (`design §5.3`); removed review changelog residue from `design §7`; fixed `libblastit_php{83,84,85,86}` spelling in `ADR-0017:44`.

## Verification Evidence
```console
$ uv run --no-project tools/plan.py validate
plan OK
```

## Follow-ups
- **`critical_artifacts.rs` authorization if a `py3-none-*.whl` stays in `dist/`:** `MB.42.3` adds a new `crates/blastit/tests/critical_universal_bundle.rs`; if `lead` wants `critical_artifacts.rs::select_wheel` updated for `py3-none-*` wheels left in `dist/` after `make e2e`, add `crates/blastit/tests/critical_artifacts.rs` explicitly to `MB.42.3`'s `# Goal` so `tools/critical_guard.py` authorizes it.
