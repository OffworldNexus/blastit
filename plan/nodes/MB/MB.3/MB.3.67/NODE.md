---
id: MB.3.67
title: "A check must not claim a verdict it cannot support: unreadable artifacts, unclassifiable ELFs, and the wheel"
kind: task
status: review
owner: implementer
priority: 2
depends_on: []
paths: ["scripts/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T23:18"
updated: "2026-11-16T00:55"
---

# Goal

Make the checks fail when they cannot read or classify their input, and cover the wheel the same way.
A check that reports "absent" because it could not read the file, or "too old" because it could not
classify the file, is the false pass `MB.3.64` and `MB.3.66` exist to prevent.

# Context

`MB.3.64` (2419971) added `scripts/check-no-test-seam.sh` (release artifact must contain **no**
`BLASTIT_TEST_SWAP_POINT`, debug must contain it) and wired it into `scripts/build-standalone.sh`
and `make check`. Its reviewer PASSed and left two MINORs:

1. `scripts/check-no-test-seam.sh:60` — an **unreadable** artifact is indistinguishable from zero
   hits: `grep` exits 2, `wc -l` yields 0, `--absent` prints `ok … absent` with rc 0 (reproduced with
   a `chmod 000` file containing the name). Fix: `[ -r "$artifact" ] || exit 2`, and treat a `grep`
   status > 1 as exit 2 instead of folding it into `|| true`.
2. The wheel `_blastit*.so` is not measured, although it is a release build with the same
   `debug_assertions` gate; `scripts/build-wheel.sh` could call the script.

# Acceptance

```console
$ uv run --no-project scripts/test_packaging_gates.py
$ make check
```

- `scripts/check-no-test-seam.sh --absent <unreadable file>` exits **2** with a sentence naming the
  unreadable path; a `grep` error (status > 1) is likewise exit 2, never "absent, rc 0". Paste the
  `chmod 000` reproduction before/after.
- The wheel edge is measured: the built `_blastit*.so` is checked the same way (call the script from
  `scripts/build-wheel.sh`), or the node states plainly why it cannot be and files it — but do not
  leave the same gate unmeasured in one artifact family and measured in another without saying so.
- The existing two directions still hold: a release artifact with the name → rc 1; a debug artifact
  without it → rc 1; clean release → rc 0; clean debug → rc 0. Existing `test-seam` cases stay green,
  and the new unreadable case is added to `scripts/test_packaging_gates.py` and shown red against
  `2419971`'s script.
- `make check` exit 0; nothing outside `scripts/**`.

# Notes

- Do not touch the seam itself (`crates/blastit/src/**`) or `critical_php_embed.rs`; this node is the
  checker only.
- Both aarch64 legs remain CI-only; nothing here proves one.

# Owner addition (2026-11-16): two more "no evidence, confident verdict" shapes from MB.3.66's review

`MB.3.66` (f1f933a) closed the `GLIBC_ABI_*` false refusal and its reviewer left two MINORs of the
same family — a verdict printed where the evidence only supports "cannot decide":

3. **An ELF with no `INTERP` and no libc `NEEDED`** (e.g. static-pie) still reads `GLIBC_ABI_DT_RELR`
   as a 2.36 floor and exits 1 on `alpine:3.21`, printing "a glibc ELF cannot start where there is no
   glibc" beside "interpreter none" — a contradiction in one message. Gate the floor comparison on
   glibc evidence (`interp_glibc` or `needs_glibc`) and treat *neither* evidence as exit 2/indecidable
   rather than a refusal. Add the case (red against f1f933a, then green).
4. **The suite pins the glibc 2.31 refusal but not the 2.36 acceptance**, so a future "always exit 1"
   regression would pass. Add the one-line accept case.

