# Result MB.3.9

## Summary

`blastit dev` owns embedded PHP worker discovery, spawning, supervision, recycling, and cleanup end to end with zero external PHP daemon and zero `sudo`. Children `MB.3.9.1` and `MB.3.9.2` landed (`done`), while the `php-fpm` pool children `MB.3.9.3`–`MB.3.9.5` were superseded by `ADR-0010` (`MB.3.17`) and fulfilled over `view.chan` by `MB.3.13`, `MB.3.7`, `MB.15`, `MB.24`, and `MB.36.5` (`crates/blastit-view-php`).

## Evidence

- Two-step zero-`sudo` local PHP workflow documented in `docs/frameworks/php/index.md` (retargeted in `MB.3.10`, restructured in `MB.20`, replayed in `e2e/php2shell.py`).
- Missing/invalid NTS `libphp` boot refusal (`critical_php_embed`, `no_python_build`), immediate worker exit/bailout containment (`critical_php_embed`, `critical_charge`), per-route worker isolation and parallel on-demand worker pool (`critical_php_backend`, `critical_parallel_views`), and `SIGTERM` process-tree + run-dir cleanup (`dev_signal_reap`, `critical_php_backend`) all pass in `make check` (204 suites ok) and `make e2e` (411 checks ok).

## Files

- `docs/frameworks/php/index.md`
- `crates/blastit-view-php/**`
- `crates/blastit/tests/critical_php_embed.rs`
- `crates/blastit/tests/critical_php_backend.rs`
- `crates/blastit/tests/critical_parallel_views.rs`
- `crates/blastit/tests/dev_signal_reap.rs`

## Follow-ups

- None (`WorkerRuntime` superseded by `ADR-0016` in `MB.36`).
