# Log MB.66

## 2026-11-23T00:23 [conductor]

created (task) Shim packages recommend (not depend on) the distro language runtime

## 2026-11-23T00:24 [lead]

status ready -> active

## 2026-11-23T00:24 [lead]

attempt 1/3 started

## 2026-11-23T00:24 [lead]

base 6f2f1c105bc5aba1ecb9d6b982e856736af0151e

## 2026-11-23T00:24 [unknown]

edited fields

## 2026-11-23T00:35 [lead]

status active -> review

## 2026-11-23T00:35 [lead]

proof: rpm python shims are symmetric with php: blastit-python-3.13-0.2.1-1.x86_64.rpm -qpR = blastit = 0.2.1-1, glibc >= 2.34, libgcc (no python), --recommends = python3.13; same for 3.14 -> python3.14; rpm -qp --suggests is empty for all seven rpms; deb 3.13/3.14 Recommends verified too

## 2026-11-23T00:39 [lead]

REVIEW: review-arch PASS zero blockers (6 MINORs rejected: core negative assertion sufficient, redundant table guard, substring recommends, spec-diff covered by artifact-level e2e, dead rpm_weak pre-existing, packaging/README follow-up); review-ux 2 MAJORs fixed (install.sh skip note -> name the shim; install.md pyenv clause names the shim + one-liner caveat) and out-of-scope sury hunk reverted

## 2026-11-23T00:39 [lead]

status review -> done
