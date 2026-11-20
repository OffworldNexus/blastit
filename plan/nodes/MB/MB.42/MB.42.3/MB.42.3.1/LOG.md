# Log MB.42.3.1

## 2026-11-20T01:33 [unknown]

created (task) Native packages: build-deb.sh + build-rpm.sh + BLASTIT_DEB/BLASTIT_RPM container smoke

## 2026-11-20T01:33 [lead]

status ready -> active

## 2026-11-20T01:33 [lead]

attempt 1/3 started

## 2026-11-20T01:33 [lead]

base 107ab5c9a1c59ebc73522770c5a4d54cbb81713f

## 2026-11-20T01:49 [lead]

status active -> review

## 2026-11-20T01:51 [lead]

REVIEW FIX (filtered) - the top finding is a FUNCTIONAL bug, split to MB.42.11: bundled_python() demands _bin/blastit-dbproxy which no bundle ships, so blastit check exits 3 ('No module named blastit') from every .deb/.rpm/tar.gz/make-install tree (both reviewers reproduced). Remaining for this node: BLOCKER payload not asserted (a package with zero shims passes) -> run check-bundle-layout.sh with BUNDLE_REQUIRE_*; BLOCKER make packages stages a HOST build not manylinux_2_28 so the floor is per-machine and stale shims raise it; BLOCKER test_packages never loads a .so (static-only fixture) so the floor+shims are untested; MAJOR packages.yml/nightly.yml do not exist but the Makefile claims they run these; MAJOR AutoReqProv:no + no NEEDED allowlist; MAJOR rpm payload ownership unasserted; MAJOR --max-glibc has no lab test; minors: 3-component GLIBC_2.2.5 invisible, xargs -0 needs -r, no SOURCE_DATE_EPOCH, dpkg-deb preflight, ceiling error omits the weak rule, packages help hides Docker.
