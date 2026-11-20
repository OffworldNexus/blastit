---
id: MB.42.14
title: "rs-no-python asserts the manylinux 2.28 floor on a host build (structurally red; blocks make check)"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MB.42.13]
paths: [Makefile]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T05:37"
updated: "2026-11-20T05:48"
---

# Goal
`make check` cannot pass on any modern host: `rs-no-python` builds `target/release/blastit` **on the host** (glibc 2.41 here) and then asserts the `manylinux_2_28` floor:

```
error: …/target/release/blastit-lsp needs glibc 2.34, above the 2.28 this artifact must run on
make: *** [Makefile:182: rs-no-python] Error 1
```

That assertion can never hold unless the binary is compiled in the manylinux image — so the leg is structurally red on every developer machine and CI host, and it blocks `make check` / the `MB.42` release gate. The 2.28 claim is real, but it belongs to the **container** target that actually compiles there.

# Exact Deliverables
1. Split the two claims in `Makefile`'s `rs-no-python`:
   - keep, on the **host** build, exactly what a host build can prove: zero `libpython`, zero `libphp`, zero `RUNPATH` (`scripts/check-portable-elf.sh` **without** a `--max-glibc` ceiling);
   - move the `--max-glibc 2_28` assertion to the target that builds inside `quay.io/pypa/manylinux_2_28_<arch>` (the `standalone-glibc`/packaging path), where it is true by construction.
2. Make the host leg **say** that it is host-built and therefore asserts no floor (`# host build: no glibc ceiling (that is standalone-glibc's job)`), so a reader is not misled into thinking 2.28 was proved here.
3. Confirm `rs-no-default` (the `--no-default-features` leg) has the same distinction and does not smuggle a ceiling in.
4. Prove: `make rs-no-python` green on this host; the manylinux target still asserts 2.28 and still refuses a shim above it (do not weaken it); `make check` reaches past `rs-no-python`.

# Notes
- Do not delete the 2.28 assertion — relocate it. The release floor is a real, measured claim (`MB.42.3.1`: measured 2.34 declared, delivered inside `manylinux_2_28`).
- `Makefile` only. Never write `/home/remy/.local` or `/usr/bin/blastit`.

# Acceptance
- `make rs-no-python` green, with its output naming what it did and did not prove
- the manylinux/`standalone-glibc` path still fails on an ELF above 2.28 (paste a negative)
- `make check` gets past `rs-no-python` (report the next failure if any, naming the command)
- `make fast-check`
