# Result MB.12

## Summary

Delivered and accepted in commit `5fab787` (`[MB.12] Accept the runtime-provider architecture: ADR-0009, ADR-0010, design, lenses`): established the multi-backend runtime taxonomy (`WSGI`, embedded NTS `libphp` over `view.chan` via per-minor C SAPI shims with zero FastCGI fallback, static mounts, Node/Vite, and JVM), capability-gated Python linkage, `/proc`-floored CPU fair queuing, and the artifact distribution matrix. The trait-based `WorkerRuntime`/`Upstream` boundary in `ADR-0009` was subsequently superseded in `0.2.0` by `ADR-0016` (`MB.36`: four view crates `blastit-view` + `blastit-view-{static,python,php}` composed by Cargo features over `view.chan`).

## Evidence

- `git show --stat 5fab787`: landed `docs/design/backend-runtimes.md`, `ADR-0009`, `ADR-0010`, `ADR-0008` supersession, `docs/research/php-embed-sapi.md`, and four goldfish critic lenses (`security`, `implementability`, `testability`, `consistency`) with 0 BLOCKERs.
- `docs/design/backend-runtimes.md` header carries the `ADR-0016` trait-supersession banner from `MB.36.6`.
- `make docs-check` and `make check` EXIT=0 at `9784965`.

## Files

- `docs/design/backend-runtimes.md`
- `docs/decisions/ADR-0008-fastcgi-backend-and-param-lockdown.md`
- `docs/decisions/ADR-0009-runtime-providers-and-capability-gating.md`
- `docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md`
- `docs/decisions/README.md`
- `docs/research/php-embed-sapi.md`

## Follow-ups

- Node/Vite integration topology (`0.2.2`) continues under `MB.4.1`.
