# Log MB.3.63

## 2026-11-15T21:06 [lead]

created (task) Make the standalone build container own its caches (read-only CARGO_HOME in manylinux)

## 2026-11-15T21:51 [lead]

status ready -> active

## 2026-11-15T21:51 [lead]

attempt 1/3 started

## 2026-11-15T21:51 [lead]

attempt 1/3 started; base 637aa97

## 2026-11-15T21:54 [lead]

owner addition: the aarch64-musl red is NOT the manylinux CARGO_HOME — CI shows a musl artifact (NEEDED libc.musl-aarch64.so.1) called 'glibc 2.0' by check-exec-env.sh:254. Instrument the leg (readelf -h + raw --version-info), reproduce by parsing an aarch64 musl binary on x86_64, exit 2 not exit 1 when it cannot decide; do not touch the artifact

## 2026-11-15T22:10 [lead]

status active -> review

## 2026-11-15T22:18 [lead]

status review -> ready

## 2026-11-15T22:18 [lead]

attempt 2/3 started

## 2026-11-15T22:18 [lead]

REVIEW FAIL (attempt 1): (1) check-exec-env.sh:205-223 reverse contradiction is a false pass — glibc INTERP + NEEDED libc.musl-* exits 0, and musl INTERP + libc.so.6 without GLIBC names exits 0 because the branch is gated on non-empty `required`; derive the contradiction from elf_interp+elf_needed before the required guard, exit 2 both ways. (2) check-exec-env.sh:186-187 the INTERP evidence never fires — readelf yields /lib/ld-musl-<arch>.so.1 which does not match `ld-musl-*`; match */ld-musl-* and feed it into the contradiction check. (3) test_packaging_gates.py:867-926 new cases cannot catch either defect — every musl case carries NEEDED libc.musl-*, only one contradiction direction; add an interp-only musl case and the reverse contradiction. Everything else PASSed (cold gnu/musl, artefact not bent, rule 16 non-vacuous, make check exit 0).

## 2026-11-15T22:18 [lead]

status ready -> active

## 2026-11-15T22:18 [lead]

attempt 2/3 started; base 637aa97

## 2026-11-15T22:22 [lead]

status active -> review

## 2026-11-15T22:52 [lead]

status review -> done

## 2026-11-15T22:52 [lead]

commit fedd78e
