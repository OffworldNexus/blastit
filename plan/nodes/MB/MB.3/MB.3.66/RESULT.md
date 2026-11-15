# MB.3.66 — RESULT (attempt 1)

## Summary

**Choice: skip the ABI-node loop when the musl evidence is already set** (the first
option `NODE.md` offers). A `GLIBC_ABI_*` node names a glibc *feature*, so it can only
be a glibc floor for a file that loads glibc; when `INTERP`/`NEEDED` already say musl,
the "which glibc does this file need?" question is answered — none — and no version name
from a non-glibc script can answer it otherwise. The two spellings are now handled by one
rule: `GLIBC_<x.y>` **and** `GLIBC_ABI_*` names are both collected into `ignored_names`,
printed, and never used as a floor. `required` stays empty, so `alpine:3.21` accepts.

`scripts/check-exec-env.sh` only: `INTERP`/`NEEDED` derivation and the contradiction
detection from `MB.3.63` are untouched; the ABI block (`abi_nodes`/`abi_floor`/the
floor comparison) moved inside an `else` of `[ -n "$musl_evidence" ]`, so a **glibc** ELF
keeps exactly the old behaviour (`GLIBC_ABI_DT_RELR` → 2.36, compared as a version).
Nothing ABI-related is blanket-exempted.

Deliberate consequence, stated plainly: an ABI node this check has *never heard of* on a
musl ELF is now skipped too (`exit 0`, name printed) instead of `exit 2 "cannot decide"`.
That is the loop being skipped, not a whitelist of known nodes — refusing it would be the
same false refusal wearing exit 2 — and a test pins it. On a glibc ELF an unknown
`GLIBC_ABI_*` still refuses with exit 2.

## Evidence

### 1. New cases are red against the current (pre-fix) code — `got 1`

The fixed script was backed up (`sha256 01f2b0b9…`), `git show fa91776:scripts/check-exec-env.sh`
was restored, the suite re-run, then my file put back (`sha256` identical afterwards).

```console
$ uv run --no-project scripts/test_packaging_gates.py     # with fa91776's script
AssertionError: a musl artifact whose version needs carry only a GLIBC_ABI_* node must pass in a musl image, not be read as a 2.36 glibc floor (MB.3.66); got 1
check-exec-env: FAIL — blastit needs glibc 2.36, and alpine:3.21 provides musl
  a glibc ELF cannot start where there is no glibc: the loader is missing, not older
  the binary's own ELF: machine AArch64, interpreter /lib/ld-musl-aarch64.so.1, NEEDED: libgcc_s.so.1 libc.musl-aarch64.so.1
RED-RUN exit=1
```

### 2. Then green, existing 7 exec-env cases included

```console
$ uv run --no-project scripts/test_packaging_gates.py
packaging-gates-test: ok — 46 cases passed (workflow=15, matrix=11, wheel-tag=4, exec-env=10, test-seam=6)
exit=0
```

`exec-env=10` = the 7 `MB.3.63`/`MB.3.61` cases plus 3 new: (d) musl + `GLIBC_ABI_DT_RELR`
accepted, (d′) musl + an *unknown* ABI node accepted and named, (e) the same node on a
glibc ELF refused by a 2.31 image.

### 3. Real image, crafted musl ELF with `GLIBC_ABI_DT_RELR` and no `GLIBC_<x.y>` — accepted

Real `docker` (29.9.3) against the real `alpine:3.21`; only `readelf` is a fixture serving
the crafted transcript (musl loader, `NEEDED libc.musl-aarch64.so.1`, one version need
`GLIBC_ABI_DT_RELR` from `libgcc_s-…so.1`):

```console
$ PATH=/tmp/opencode/mb366/bin:$PATH scripts/check-exec-env.sh --image alpine:3.21 --binary blastit-crafted-musl
check-exec-env: blastit-crafted-musl links musl (interpreter /lib/ld-musl-aarch64.so.1), so GLIBC_ABI_DT_RELR in .gnu.version_r is not a glibc requirement
  those names are the version script of a library that is not glibc (libgcc_s/libunwind); machine AArch64
check-exec-env: ok — blastit-crafted-musl requires no glibc symbol version (musl or static), image alpine:3.21 provides musl
exit=0
```

### 4. glibc counterpart still compares its floor and still fails (not a blanket exemption)

Same transcript, glibc loader + `NEEDED libc.so.6`, real images:

```console
$ ... --image debian:bullseye-slim --binary blastit-crafted-musl      # glibc 2.31
check-exec-env: FAIL — blastit-crafted-musl needs glibc 2.36, debian:bullseye-slim has 2.31
  running it there fails at execve with:
    version 'GLIBC_ABI_DT_RELR' not found (required by blastit-crafted-musl)
  fix: build the binary inside debian:bullseye-slim (or a builder no older than it), or
       raise the image to glibc >= 2.36 — never the binary's floor
exit=1

$ ... --image debian:bookworm-slim --binary blastit-crafted-musl      # glibc 2.36
check-exec-env: ok — blastit-crafted-musl needs glibc 2.36, debian:bookworm-slim provides 2.36
exit=0
```

The same pair is a suite case (fake docker answering `2.31`, `exit 1`, message names
`GLIBC_ABI_DT_RELR`), so it cannot regress silently.

### 5. `make -k check` — one red, and it is not mine

```console
$ make -k check   # exit=2; full log /tmp/opencode/mb366/make-check-final.log
guard: OK (10 staged/untracked file(s) checked)
No issues found                     # docs-check (zensical --strict)
packaging-gates-test: ok — 46 cases passed (…, exec-env=10, …)
doc-tone: ok — 26 user-facing page(s), no banned phrase
plan OK
FAILED python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_php_vocabulary
1 failed, 95 passed, 65 subtests passed in 1.03s
make: *** [Makefile:307: docs-unit] Error 1
```

The only failure is `docs-unit`, and its only finding is another agent's **untracked**
brief — `plan/nodes/MB/MB.3/MB.3.68/NODE.md:31: 'fall back'`. That path is `??` in `git
status` (never committed, present before this node started), `scripts/` is not in that
guard's candidate set (`product_doc_files` = README + `docs/` pages, `live_briefs` =
`plan/nodes/**/NODE.md`), and the failure reproduces/survives entirely independently of my
diff. Proven directly, my files included:

```console
$ uv run --no-project /tmp/opencode/mb366/prove_red.py
real tree findings: ["plan/nodes/MB/MB.3/MB.3.68/NODE.md:31: 'fall back': …"]
same tree without MB.3.68 (my scripts/ included): []
scripts/ in scanned candidates: False
```

Every other target in `check` ran green under `-k` (rs-fmt, rs-lint, rs-doc, rs-test,
test-critical, py-lint, docs-check, docs-test, rs-no-python, seam-check,
platform-matrix-check, workflow-check, packaging-gates-test, doc-tone, plan-validate).
I did not silence or edit it — `plan/` and `python/tests/` are outside this node's paths.

### 6. Lint/format and scope

```console
$ uvx --offline ruff check scripts/test_packaging_gates.py && uvx --offline ruff format --check …
All checks passed!
1 file already formatted
$ bash -n scripts/check-exec-env.sh && echo "bash -n ok"
bash -n ok

$ git status --short
 M plan/BUDGET.md
 M plan/JOURNAL.md
 M plan/nodes/MB/MB.3/{MB.3.62,MB.3.63,MB.3.64,MB.3.65}/LOG.md
 M scripts/check-exec-env.sh
 M scripts/test_packaging_gates.py
?? plan/nodes/MB/MB.20/  ?? plan/nodes/MB/MB.3/{MB.3.66,MB.3.67,MB.3.68,MB.3.69}/
```

Nothing outside `scripts/**` was written by this node; the `plan/` entries above are other
agents' in-flight files (and this node's own untracked folder). No `git add`/`commit`.

## Files

- `scripts/check-exec-env.sh` — ABI-node loop and floor comparison now run only when the
  ELF's libc is not musl; `GLIBC_ABI_*` names join the ignored-names line (printed, never a
  floor). Header documents the rule.
- `scripts/test_packaging_gates.py` — `MUSL_ABI_NODE_TRANSCRIPT` fixture (non-glibc
  `File:` declaring `GLIBC_ABI_DT_RELR`) and three exec-env cases; docstring count 7→10.

## Follow-ups

- `make check` is red on `docs-unit` because `plan/nodes/MB/MB.3/MB.3.68/NODE.md:31` uses
  "fall back" (a retired PHP vocabulary the guard bans). Owner of MB.3.68 must reword it;
  not editable from this node (`paths: scripts/**`).
- Worth a `docs/knowledge/LESSONS.md` line (also outside my paths): **one false-refusal
  class has more than one spelling** — `MB.3.63` taught the judge to ignore `GLIBC_<x.y>`
  names on a musl ELF but left `GLIBC_ABI_*` out, so fixing a refusal class means listing
  every spelling the parser can produce, not the one the bug report quoted.
- Both aarch64 legs remain CI-only; nothing here proves one.
