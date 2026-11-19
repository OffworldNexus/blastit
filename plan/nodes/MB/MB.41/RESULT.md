# MB.41 — Gate evidence

Global multi-language golden-path documentation overhaul. Three slices, each reviewed by
`review-ux` + `review-arch` with fixes folded in before its commit.

| Node | Commit | What landed |
|---|---|---|
| `MB.41.1` | `48f2952` | Onboarding spine (`zensical.toml`, `README`, Home, Getting started, Blastfile, Local, CI/CD): Linux stated first, Blastit framed as a Linux application server **behind** the cloud ingress (zero edge-proxy apologies), and the linked `Python (WSGI)` / `PHP` / `Static & SPA` golden path on every page. |
| `MB.41.2` | `ba347dd` | Deployments, Frameworks, Reference, How-it-works, Security, Troubleshooting aligned to the same model; interface-keyed capability table; complete runnable per-runtime Dockerfiles whose archive install keeps `bin/`→`share/blastit/lib` and the `libblastit_php*.so` shims. |
| `MB.41.3` | `953cefe` | `docs_fences` + `runnable_docs` de-indent and validate fences inside `=== "…"` tabs (per-document `Shape` pin, `PAGES`/`TOTAL` 17/63), `fmt-check`/`dev-roots`/`denial` modes execute what the pages promise, and `--security-opt seccomp=unconfined` unconfines Docker's outer profile on the PHP suites (blastit's inner `seccomp(SECCOMP_SET_MODE_FILTER)` still sandboxes every view). |

## Full gate (this tree)

```
$ make check
… rs-fmt / rs-lint / rs-doc / rs-test / test-critical / py-lint /
  docs-unit / docs-check / docs-test / rs-no-python / rs-no-default /
  seam-check / platform-matrix-check / workflow-check /
  packaging-gates-test / zed-check / plan-validate …
zed-check: OK (59 properties checked)
Ran 44 tests in 8.526s
OK
plan OK
```

```
$ make php-sapi-e2e PHP_MINOR=8.4 PHP_SAPI_KEEP=1 && make rs-test-php-image
ok  php 8.4 abi 1: status 200, cpu_ns 8088871 <= wall_ns 11736993, exit()+fatal contained, next request served
test result: ok. 29 passed; 0 failed
rs-test-php-image: PR_SET_DUMPABLE(0) witness — 2 role(s) denied (EACCES) to uid 1000
```

Leaf evidence, per-fence counts and the review reports live beside each node
(`MB.41.1/`, `MB.41.2/`, `MB.41.3/`).

## Rendered-site QA

`visual-checker` on the built `site/` (light + dark + 390x844): every one of the eight tabbed
pages switches all groups on one click and persists across navigation, tab-internal fences render
as titled highlighted code, all eight Mermaid diagrams render unclipped, no horizontal scroll on
mobile. Report: `plan/nodes/MB/MB.41/visual-check.md`.

## Follow-ups (out of this epic's scope)

- `docs/deploy/compose-traefik.md` grew 308→345 lines to hold three complete images; a future
  slice could split the image recipes onto their own page (`zensical.toml` nav edit).
- `e2e/static_assets.py`'s docstring still says it compares "the first three lines" of the banner;
  it now matches all four contiguous lines.
- `docs/frameworks/python/django.md` is 139 lines, just over the 130-line target.
