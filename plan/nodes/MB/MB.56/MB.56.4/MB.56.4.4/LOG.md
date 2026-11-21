# Log MB.56.4.4

## 2026-11-20T21:07 [lead]

created (task) Wire workflows: drop the host-tree smoke (root), run every §2/§3/§6 story on the matrix

## 2026-11-20T22:45 [lead]

status ready -> active

## 2026-11-20T22:45 [lead]

attempt 1/3 started

## 2026-11-20T22:45 [lead]

base c90b03deb0721e5f2aba7486f91973e477e2f968

## 2026-11-20T22:53 [lead]

REVIEW FIX: (B) restore the R42 critical-skip-check job (nothing in CI runs it now); (M) restore one bounded ROOT job with BLASTIT_REQUIRE_ROOT_TESTS=1 running --test e2e_cli (the 'binary owned by another uid' rule can otherwise never fail); (M) add --require-legs to run_packaging.py (any SKIP -> exit 1) and use it in ci.yml wheel-smoke, build-wheel action, wheels.yml, packages.yml composer, publish-pypi wheel/alpine; (m) packages.yml §3 install.sh --user leg feeds a host binary - pass the downloaded standalone or rename.

## 2026-11-20T22:53 [lead]

edited fields

## 2026-11-20T22:53 [lead]

REVIEW FIX 2 (ux): (B) make packages-test expands $(deb)/$(rpm) at make level -> runs all six legs and passes no artifact (fix with set --); (M) e2e/README + 'make packages' artifact dir mismatch (PACKAGES_OUT is $TMPDIR/opencode/blastit-packages, discover() scans target/packages + dist) -> align or export BLASTIT_DEB/RPM/WHEEL; (M) docs/testing/README.md gate table missing make e2e-prod/e2e-packaging + the docs-unit prerequisite; (M) Makefile:13 help text for make e2e is stale.

## 2026-11-20T22:55 [lead]

status active -> done
