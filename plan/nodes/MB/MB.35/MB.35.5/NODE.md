---
id: MB.35.5
title: "Build, smoke and release blastit-lsp on five targets from platform_matrix, with workflow and packaging gates kept honest"
kind: task
status: review
owner: implementer
priority: 1
depends_on: [MB.35.4]
paths:
  - .github/workflows/ci.yml
  - ".github/workflows/publish-pypi.yml"
  - scripts/platform_matrix.py
  - scripts/check_workflows.py
  - scripts/test_packaging_gates.py
  - "scripts/build-lsp.sh"
  - scripts/lsp_smoke.py
attempts: 2
max_attempts: 3
escalation: none
created: "2026-11-17T13:17"
updated: "2026-11-18T07:22"
---

# Goal

Build, smoke-test and release `blastit-lsp` on all five targets from one table, and keep the
workflow/matrix gates honest while doing it.

1. **The matrix, once.** `scripts/platform_matrix.py` gains an `lsp` family beside
   `wheels`/`binaries`/`shims`/`images`: five legs
   `(target, runs_on, archive, executable)`, the archive name
   `blastit-lsp-<target>.tar.gz|.zip`, `--matrix lsp`, `--summary`, and the declared/proved
   split (a leg is `PROVEN` only after its smoke ran). A dropped target carries
   `dropped_because` in one sentence, exactly like the existing legs.
2. **The build script.** `scripts/build-lsp.sh` builds `cargo build -p blastit-lsp --release
   --locked --target <target>`, stages `blastit-lsp-<target>/blastit-lsp(.exe)` and archives it
   (`tar -czf` on Linux/macOS, `zip` or `python -m zipfile` on Windows — no new dependency).
   It must not need Python at runtime and must not embed a libpython (the LSP has none).
3. **The smoke.** `scripts/lsp_smoke.py` (stdlib, cross-platform) spawns the produced binary and
   drives framed JSON-RPC: `initialize` → assert the capability document (and, after `MB.34.5`,
   `codeActionProvider`) → `didOpen` a `Blastfile` with `query 'SELCT id FROM t'` → assert one
   severity-1 diagnostic whose `range.start` is the exact line/character → `shutdown` → `exit`,
   status `0`, and no stderr byte. Exit `0`/`1` with one line; `--binary <path>` is the only
   argument. It runs on Windows without a shell (`subprocess` with a list).
4. **CI.** `.github/workflows/ci.yml` gains one `lsp` job over `strategy.matrix:
   ${{ fromJSON(platform_matrix.py --matrix lsp) }}` that builds + smokes on the native runner;
   `.github/workflows/publish-pypi.yml` gains the same build in the release path, uploads the
   five assets as its own artifact family, and its `release` job includes them in `dist/` and in
   the count check (the current `standalone-*` count must not silently start counting LSP
   archives, and the upload glob must be explicit). `LIBCLANG_PATH` on `windows-latest` comes
   from `MB.35.1`'s finding, with a comment naming it.
5. **The gates learn the facts.** `scripts/check_workflows.py` validates the new family's
   `--matrix/--leg` calls, every `needs.<job>.outputs`, `if-no-files-found: error`, and the
   download/upload pairing for the new artifacts; `scripts/test_packaging_gates.py` gains one
   mutation per new class (drop `--matrix lsp` → red; rename an asset in one place → the
   download pairing is red; count the wrong family → red).

# Context

- **The one-table rule.** `platform_matrix.py` exists because a leg declared twice drifts
  (`MB.3.18`). Do not write five literal target strings into two workflows; both jobs resolve
  the matrix through the script, as `binaries_gnu`/`binaries_musl` already do.
- **Runner facts from research.** `MB.35.1` names the macOS runner (`macos-latest` arm64 vs
  `macos-15-intel`) and whether the x86_64 Apple leg is native or a documented cross-build. A
  native runner is preferred: an artifact that was never executed is a claim, not a proof.
- **`pg_query` needs a C toolchain on every host.** On Windows that means the MSVC C compiler
  and `LIBCLANG_PATH` (bindgen); on macOS the Xcode CLT LLVM. The job installs nothing else and
  caches `~/.cargo` + `target` — but never a `target/<triple>` directory across runners.
- **The release job's count is a claim.** `publish-pypi.yml:743+` downloads
  `pattern: standalone-*` and asserts the number of archives; a sixth family means either a
  second explicit count or a renamed pattern with a reason. `scripts/platform_matrix.py --check`
  also scans the pages for "N archives" claims (`MB.35.2` wrote them) — update the page or the
  matrix so the numbers are equal, and paste both outputs.
- **Do not touch** `crates/**`, `editors/**`, `docs/**` (except a version/count sentence that
  `--check` proves inconsistent — list the page in `paths` if so), `Makefile`'s existing targets,
  or `scripts/check_zed_extension.py`.
- **Offline checks.** `make workflow-check` and `make packaging-gates-test` are offline
  (`UV_OFFLINE=1`, hash-locked); run both locally. The CI legs themselves can only be proven on
  push — record what is local and what is the first-CI-run obligation.

# Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
$ scripts/platform_matrix.py --matrix lsp | python3 -c 'import json,sys; m=json.load(sys.stdin); assert len(m)==5; print([e["target"] for e in m])'
$ scripts/platform_matrix.py --summary
$ uv run --no-project scripts/lsp_smoke.py --binary target/release/blastit-lsp
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project scripts/check_workflows.py
$ UV_OFFLINE=1 uv run --no-project --locked scripts/check_workflows.py --quiet
$ UV_OFFLINE=1 PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project \
    scripts/test_packaging_gates.py
$ make workflow-check platform-matrix-check packaging-gates-test
$ bash scripts/build-lsp.sh --target "$(rustc -vV | sed -n 's/^host: //p')" --out /tmp/opencode/lsp-dist
$ tar -tzf /tmp/opencode/lsp-dist/blastit-lsp-*.tar.gz        # one dir, one executable
```

The five asset names must be exactly:

```text
blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
blastit-lsp-aarch64-unknown-linux-gnu.tar.gz
blastit-lsp-aarch64-apple-darwin.tar.gz
blastit-lsp-x86_64-apple-darwin.tar.gz
blastit-lsp-x86_64-pc-windows-msvc.zip
```

Paste into `RESULT.md`: the `--summary` table, the local smoke output, the `check_workflows`
tail, the `build-lsp.sh` archive listing, and the first-CI-run checklist (which job proves which
target). If `MB.35.1` dropped a target, the four remaining names are the contract and the
`dropped_because` is quoted.

# Notes

- **No `sudo`, no new action.** Use `actions/checkout`, `dtolnay/rust-toolchain`,
  `actions/cache` and the shell; a new third-party action is a supply-chain review, not this
  leaf.
- **Windows archive.** `python -m zipfile -c out.zip dir` is stdlib and available on the runner;
  prefer it to an `Expand-Archive`-only flow so the artifact is deterministic.
- **Zip layout must match the extension's expectation** (`MB.33`): one top-level
  `blastit-lsp-<target>/` directory holding `blastit-lsp.exe`, never the exe at the archive root.
- The smoke script is the same one every leg runs; a platform with a different framing bug is
  found on the first CI run, not by a per-platform copy.
