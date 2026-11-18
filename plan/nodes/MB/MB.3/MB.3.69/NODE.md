---
id: MB.3.69
title: "Run the rewritten two-package witness, and correct the grepped-set list"
kind: task
status: superseded
owner: implementer
priority: 2
depends_on: []
paths: ["e2e/php-sapi/**", plan/nodes/MB/MB.3/MB.3.65/RESULT.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-16T00:19"
updated: "2026-11-18T20:36"
---

# Goal

Prove the rewritten two-package witness by running it, and correct `MB.3.65`'s grepped-set list.

# Context

`MB.3.65` (fa91776) rewrote `e2e/php-sapi/Dockerfile.two-package` and `run-two-package.sh` for the
8.3+8.4 pair (8.2 is gone) and its RESULT admits in Follow-ups that the image was never built or run
afterwards, so the new "both objects usable, newest wins" assertions are **unproven**. Its reviewer
also found that `RESULT.md:184` (A7) says "all accounted for" while omitting the `8.2` hits at
`docs/knowledge/LESSONS.md:54,55,60` and `docs/research/php2shell.md:214`, which are inside the
grepped set.

# Acceptance

```console
$ make php-sapi-e2e PHP_SAPI_KEEP=1 PHP_SAPI_IMAGE=blastit-php-two:local   # or the documented build
$ docker run --rm --cap-add SYS_PTRACE blastit-php-two:local
$ make check
```

- The two-package image **builds and its own command exits 0** on the machine here (alpine:3.21,
  both packages), pasting the tail that shows the served branch and the resolved 8.4 library. If it
  cannot run here for a stated reason, say exactly what blocks it and mark the script's header
  "rewritten, not yet executed" — but prefer running it, because an unexecuted witness is how a
  caption drifts from what it proves (`MB.3.62`).
- The witness must still refuse when `BLASTIT_LIBPHP` is exported (the `MB.3.62` guard) and still
  fail if the boot refuses or nothing serves.
- `MB.3.65/RESULT.md` A7 lists the two omitted files with the reason each is acceptable (historical
  lesson / research note), so "all accounted for" is true rather than tidy.
- `make check` exit 0; nothing outside `e2e/php-sapi/**` and `plan/nodes/MB/MB.3/MB.3.65/RESULT.md`.

# Notes

- Editing a `done` node's RESULT is allowed for a factual correction; do not change its verdict.
- Both aarch64 legs stay CI-only; this image is x86_64 here.
