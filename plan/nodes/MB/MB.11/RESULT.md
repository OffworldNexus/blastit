# Result MB.11

## Summary

Delivered in commit `f430031` (`[MB.11] Rewrite the DO roles/pooler section and the static-site Run-it`), and subsequently aligned with the structured `/deploy` model (`MB.18`, `6ba10d3`) and `Blastfile` syntax (`MB.21.5.1`, `MB.21.6`). Left in `ready` only due to `release/0.2.1` -> `release/0.2.0` merge bookkeeping (`8e657ca`).

## Evidence

- `git show --stat f430031`: restructured `docs/deploy/digitalocean-paas.md` into ordered subsections + role/URL table (app -> pool `25061`, migrator -> cluster `25060`), updated `docs/frameworks/static-and-spa.md` to `uvx blastit dev`, re-indexed `python/tests/runnable_docs/inventory.toml`, and verified `make docs-unit`, `make docs-check`, `make docs-test`, and `dx` + `ai-ergonomics` critics.
- Verified green at `HEAD` (`9784965`): `make check` EXIT=0, `e2e/docs_deploy.py` (`make e2e`) EXIT=0.

## Files

- `docs/deploy/digitalocean-paas.md`
- `python/tests/runnable_docs/inventory.toml`

## Follow-ups

- None.
