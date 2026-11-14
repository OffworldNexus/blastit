# Log MB.3.13.2

## 2026-11-14T17:04 [lead]

created (task) MB.3.13 part 2: stage0 BLASTIT_LIBPHP / distro NTS libphp discovery and the NTS+minor ABI boot gate, with ADR-0010 s4's two exact messages

## 2026-11-14T19:28 [lead]

INPUT from MB.3.13.1 (2026-11-14): the loader and Config binding expect (libphp, shim) as either path or fd-pinned SharedObject; production passes OwnedFd via ChildIo.keep (fix in .13.1). Your Stage-0 discovery must produce that binding: BLASTIT_LIBPHP or the distro candidate list, static ELF .dynsym NTS check (executor_globals present, tsrm_startup absent, zero dlopen), shim selection by (host minor, target arch, target libc), the two exact ADR-0010 §4 messages (missing / ZTS-or-mismatch) naming the route, the package and BLASTIT_LIBPHP, and (path, st_dev, st_ino) recorded in Config for the Stage-1 re-open. docs/reference/settings.md's empty '## Backends' section gets only the BLASTIT_LIBPHP row (MB.3.10 owns the rest).

## 2026-11-14T19:29 [lead]

edited fields

## 2026-11-14T21:04 [lead]

SCOPE ADD (2026-11-14, MB.3.10 review): ADR-0010 §2's downward workers bound belongs to this Stage-0 gate: workers > config.view_worker_cap() must be a boot error naming both numbers (and 1..=64 is already parsed). No code reads PhpPolicy.workers today, so the docs may not claim it until this lands. Add it here with the accepted/denied twins.
