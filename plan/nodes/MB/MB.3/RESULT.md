# Result MB.3

## Summary

Shipped first-class embedded PHP support for `0.2.0` (`entrypoint php "public/index.php"` in `Blastfile`, `ADR-0010` + `ADR-0012` + `ADR-0015` + `ADR-0016`): host NTS `libphp` for **PHP 8.3, 8.4, and 8.5** (`8.2` explicitly refused) runs inside unprivileged per-route `blastit-view-php` worker processes over `view.chan` via per-minor Apache-2.0 C SAPI shims (`crates/blastit-php-sapi`), with zero external `php-fpm`/FastCGI daemon, zero CGI parameter stream, boot-time `(st_dev, st_ino, st_size, st_ctime)` entry/shim/`libphp` pinning, production provenance gates, on-demand multi-process per-route concurrency (`MB.24` + `MB.26.8`), `fanotify` live reload of `EG(included_files)` (`MB.23.4.2`), `INFO runtime` diagnostics (`MB.25`), wire-level `blastit dev --learn`, and automated 24-probe `php2shell` exploit neutralization in `e2e/`.

## Architectural Supersessions Recorded Honestly

1. **Manifest surface (`[route.php]` -> `Blastfile` `entrypoint php`):** `MB.3.5` and its remaining TOML leaves (`MB.3.5.2`, `MB.3.5.3`) were superseded by `MB.21` (`ADR-0012`), which retired TOML and the per-route `workers` knob in favour of `entrypoint php "public/index.php"` and scheduler-managed parallel worker pools (`MB.24`, `ADR-0015`).
2. **Backend seam (`Upstream`/`WorkerRuntime` -> four view crates over `view.chan`):** `MB.3`'s original Acceptance bullet 2 (`Upstream` trait in `MB.9`) and `MB.3.6` were superseded by `MB.36` (`ADR-0016`: pure-`std` `blastit-view` transport + `blastit-view-{static,python,php}`).
3. **PHP CI completeness scope in `0.2.0`:** `blastit dev --learn` records PHP routes and PostgreSQL statements end to end, and `blastit check` validates the `Blastfile`, static roots, and `libphp` boot gate (`blastit fmt --check` is the `0.2.0` PHP CI gate, stated explicitly in `docs/frameworks/php/index.md:90-99` and `CHANGELOG.md`). Non-Python `blastit check -- <suite>` request-level suite execution (`MB.3.6.4`) is superseded against `MB.9`'s retired types and carried forward into `MB.5` (`0.2.3` Unified Multi-Backend Gate across Python, PHP, Node, and JVM).
4. **CI-only `aarch64` legs:** All 6 `x86_64` `(8.3, 8.4, 8.5) × (gnu, musl)` C SAPI shims and standalone `x86_64` `gnu`/`musl` archives were built and proved `served` locally (`MB.3.18`, `MB.3.65`); the 6 `aarch64` shim legs and 2 `aarch64` standalone archives are declared in `scripts/platform_matrix.py` and executed in GitHub Actions (`php-sapi.yml`, `publish-pypi.yml` on `ubuntu-24.04-arm`), not locally.

## Evidence

- `make check` EXIT=0 (`204` test suites ok, 0 failed, incl. `critical_php_embed`, `critical_php_backend`, `critical_parallel_views`, `critical_shim_contract`, `critical_file_sink`, `critical_php_manifest`, `no_python_build`, `platform-matrix-check`, `workflow-check`, `packaging-gates-test`).
- `make e2e` EXIT=0 (`411` checks ok, 0 failed, incl. `e2e/php2shell.py` 24-probe matrix across CVE-2012-1823, CVE-2024-4577, CVE-2024-8926, CVE-2024-8927, `PHP_VALUE`/`PHP_ADMIN_VALUE`, `httpoxy`, `.user.ini`, `.php`/`.phar`/polyglot static `404`, and `e2e/rootless/php_entry_swap.py` live symlink/mode/in-place-overwrite pinning).
- Audits `docs/testing/AUDIT-MB.3.13.md`, `AUDIT-MB.3.13-CLOSURE.md`, and `AUDIT-MB.36.md`: all `MAJOR` findings (`G1`–`G7`, `N1`) closed.

## Files

- `crates/blastit-php-sapi/**`, `crates/blastit-view-php/**`
- `crates/blastit/tests/{critical_php_embed,critical_php_backend,critical_parallel_views,no_python_build}.rs`
- `crates/blastit-manifest/tests/critical_php_manifest.rs`
- `docs/frameworks/php/{index,symfony,laravel}.md`, `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md`
- `e2e/php/**`, `e2e/php-sapi/**`, `e2e/php2shell.py`, `e2e/php_sapi_probe.py`, `e2e/rootless/php_entry_swap.py`
- `scripts/platform_matrix.py`, `scripts/build-standalone*.sh`, `scripts/collect-shim.sh`, `.github/workflows/{php-sapi,publish-pypi}.yml`

## Follow-ups

- Deferred hardening & coverage re-filed under `MB.26`: `MB.26.30`–`MB.26.37` (from `MB.3.15`, `MB.3.16`, `MB.3.22`, `MB.3.26`, `MB.3.34`, `MB.3.55`, `MB.3.68`, `MB.3.69`), `MB.26.41` (`MB.27`), `MB.26.43` (`MB.29`), `MB.26.44` (`MB.30`), and `MB.26.56` (`blastit-php-sapi` mutation campaign).
- Cross-language `blastit check -- <suite>` request-level suite runner tracked in `MB.5` (`0.2.3`).
