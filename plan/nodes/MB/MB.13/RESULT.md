# Result MB.13

## Summary

Delivered in commit `aa94c58` (`[MB.13] Rewrite the deployments index page for scannability`), and further distilled into the model-first `/deploy` landing page (`93` lines, well under the `120`-line cap) in `MB.18` (`6ba10d3`). Left in `ready` only due to `release/0.2.1` -> `release/0.2.0` merge bookkeeping (`8e657ca`).

## Evidence

- `git show --stat aa94c58`: rewrote `docs/deploy/index.md`, passed `dx` (`docs/design/deploy-index.review-dx.md`) and `ai-ergonomics` (`docs/design/deploy-index.review-ai-ergonomics.md`) goldfish critics, updated `python/tests/runnable_docs/inventory.toml` and `e2e/docs_deploy.py`.
- Verified green at `HEAD` (`9784965`): `make check` EXIT=0, `e2e/docs_deploy.py` (`make e2e`) EXIT=0.

## Files

- `docs/deploy/index.md`
- `python/tests/runnable_docs/inventory.toml`

## Follow-ups

- None.
