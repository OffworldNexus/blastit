---
id: MB.3.66
title: "The judge must not read GLIBC_ABI_* from a non-glibc version script as a floor"
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths: ["scripts/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T22:52"
updated: "2026-11-16T00:33"
---

# Goal

Close the last false-refusal shape in `scripts/check-exec-env.sh`: a **non-glibc** version script
that declares a `GLIBC_ABI_*` node must not be read as a glibc floor.

# Context

`MB.3.63` (fedd78e) rewrote the judge so a musl ELF is classified from `INTERP`/`NEEDED` instead of
from version-node names, and taught it to exit 2 on contradictory evidence. Its reviewer PASSed and
left one MINOR, the same class the node fixed:

> `scripts/check-exec-env.sh:261-272` the musl exemption clears only `GLIBC_<x.y>` names; a
> non-glibc library declaring `GLIBC_ABI_DT_RELR` on a musl ELF (crafted `musl-with-abi-node`)
> still sets `required=2.36` and false-refuses `alpine:3.21` with exit 1.

The ABI-node loop runs even when the libc evidence already says musl, so a `GLIBC_ABI_*` name from a
non-glibc script becomes a floor that cannot exist. That is a confident wrong verdict in the gate
whose whole point (`MB.3.60`, `MB.3.63`) is that a judgement must name its evidence.

# Acceptance

```console
$ uv run --no-project scripts/test_packaging_gates.py
$ make check
```

- A musl ELF carrying a `GLIBC_ABI_DT_RELR` node (no `GLIBC_<x.y>` names) is **accepted** in
  `alpine:3.21` (exit 0), not refused with a 2.36 floor. Skip the ABI-node loop when the musl
  evidence is set, or ignore `GLIBC_ABI_*` names from a non-glibc version script — say which.
- A **glibc** ELF that genuinely needs `GLIBC_ABI_DT_RELR` (and a floor) keeps its floor
  comparison and is still refused where the floor is too high. This must not become a blanket
  exemption: paste the glibc case that still fails.
- One new case in `scripts/test_packaging_gates.py` for the musl + `GLIBC_ABI_*` shape, shown **red
  against fedd78e's code** (paste `got 1`), then green. The existing 7 exec-env cases stay green.
- `make check` exit 0; nothing outside `scripts/**`.

# Notes

- Do not re-open the `INTERP`/`NEEDED` derivation from `MB.3.63`; only the ABI-node loop changes.
- Both aarch64 legs stay CI-only; nothing here proves one.
