# MB.44 — CHANGELOG.md / RELEASING.md sync for 0.2.0

## What changed

- `CHANGELOG.md`: heading restamped `## 0.2.0 (2026-11-20)`; intro and the PHP
  bullet now say `8.3, 8.4, 8.5 and 8.6` (with the release's `16` per-minor
  shims `libblastit_php{83,84,85,86}-<arch>-<libc>.so`; PHP 8.2 and older still
  refused at boot); the superseded "Four view crates…" bullet was replaced by
  **One universal `blastit` binary and signed repositories (`ADR-0017`)** —
  `libc`/`libm`/`libgcc_s` only, CPython 3.13/3.14 `(st_dev, st_ino)`-pinned
  shims, `4` `py3-none` wheels via the `--internal-json` NDJSON bridge,
  GPG-signed Cloudsmith `main`/`nightly` repos + `install.sh` + Composer wrapper,
  zero-TOCTOU CLI handoff — keeping the surviving `--learn`, `body raw` gate and
  pooler facts; the retired TOML manifest is now described without naming it.
- `RELEASING.md`: pre-merge list adds the `Python SAPI` workflow and rewrites the
  `Publish` rehearsal as `wheels_gnu`/`wheels_musl` building `4` universal
  `py3-none` wheels via `scripts/build-wheel.sh` (`maturin build --bindings bin`
  + `scripts/repack_universal_wheel.py`, no extension module compiled;
  `manylinux` smoked on CPython `3.12`/`3.13`/`3.14`, `musllinux` on `3.13`) and
  `16` probed PHP `8.3`–`8.6` legs; the post-merge step adds **Packages** (tag
  push, shared `release-${{ github.ref }}` group, `4` `.deb`/`.rpm`, each on the
  legs of its own format — `8` `deb` + `4` `rpm` over `6` distributions × `2`
  architectures, Cloudsmith `main` + Forgejo, waits for `Publish`, reseals one
  `SHA256SUMS`, `workflow_dispatch` rehearsal unless `publish: true`) and
  **Nightly** (`03:17 UTC`, `~nightly<date>.r<run>+g<sha>`, `nightly` component,
  newest seven); the matrix's Linux serving line now names `4` wheels, `4`
  archives, `16` PHP shims and `8` CPython `3.13`/`3.14` shims. `blastit-lsp` and
  "what floats" prose unchanged.

## Review fixes

- `CHANGELOG.md`: restored the per-minor PHP package hints
  (`libphp8.3`–`libphp8.6`-embed / stock `libphp-embed`, `php83`–`php86`-embed,
  `php-embedded`) and the "CGI/FastCGI parameter surface" wording; the CLI
  handoff now reads `$VIRTUAL_ENV/bin/blastit` **or** `./.venv/bin/blastit`.
- `RELEASING.md`: wheels are built by `scripts/build-wheel.sh` (no repack claim
  about `blastit`/`blastit-lsp`/shims), musl smoke limited to CPython `3.13`,
  and the packages install on their own format's legs (`8` `deb` + `4` `rpm`).

## Evidence

Command (single run):

```console
$ scripts/release-notes.sh --require-dated "v0.2.0" > /dev/null && uv run --no-project scripts/platform_matrix.py --check && make docs-check && make docs-unit && make workflow-check
```

Tail:

```text
platform-matrix: ok — 4 wheels, 4 archives, 16 shims, 8 python shims, 5 lsp assets declared;
  built and exercised: 1/4 wheels, 2/4 archives, 8/16 shims, 2/8 python shims, 1/5 lsp assets;
  4 packages installed in 12 container legs; ... 0 leg(s) dropped
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
sh -n docs/install.sh
118 passed, 65 subtests passed in 1.43s
workflow-check: ok — 674 checks over 9 workflows and 1 actions, 0 problem(s)
```

Exit 0 for the whole `&&` chain (re-run after the review fixes).
