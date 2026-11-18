# RESULT — MB.35.5: build, smoke and release `blastit-lsp` on five targets from one table

STATUS: done (attempt 2 — one table restored; Linux x86_64 proved; four legs declared, unproven)

## attempt 2 — the seven reviewer items

| # | Finding | Fix |
|---|---|---|
| 1 | **MAJOR** `build-lsp.sh` re-listed the five triples + exe/format, and both workflows re-spelled `blastit-lsp-<target>` | `build-lsp.sh:95-137` now matches **shapes** (`*-unknown-linux-gnu`, `*-apple-darwin`, `*-pc-windows-msvc`) plus an arch prefix (`x86_64|aarch64`), and takes the three names from the table: `--archive`, `--dir`, `--executable` (defaults keep the developer path working). `platform_matrix.py` gained `dir`, `ext` and `ci_command`; both workflows pass `--archive "${{ matrix.archive }}" --executable "${{ matrix.executable }}" --dir "${{ matrix.dir }}"` and unpack `${{ matrix.dir }}`, so the asset name, the top-level directory and the executable exist once (`LSP_LEGS` + `lsp_legs()`). The error that used to list five triples now points at `--matrix lsp` |
| 2 | **MAJOR** `publish-pypi.yml:881` hardcoded `blastit-x86_64-…`/`blastit-aarch64-…` | the release list is `mapfile -t archives < <(printf '%s\n' blastit-*.tar.gz \| grep -v -e '^blastit-lsp-' -e '^$')` (`publish-pypi.yml:880-891`) compared with `--count binaries`; `lsp=(blastit-lsp-*)`. No target is named in the workflow |
| 3 | **MINOR** `--summary lsp` exited 2 | `--summary` is `nargs="?"` with the family choices (`wheels/binaries/shims/images/lsp`); `summary(family)` filters the family row, the artifact rows and the PHP sections, and `--summary lsp` prints the editor family only |
| 4 | **MINOR** the NOT RUN cell named no command | every lsp leg carries `ci_command` (derived in `lsp_legs()` from its own fields), printed in the summary cell for proved and unproven legs alike |
| 5 | **MINOR** globs were not nullglob-safe | `shopt -s nullglob` before `SHA256SUMS.*`, the `mapfile` and the two arrays; the `^$` filter drops the empty line `printf` emits when nothing matched (verified in a scratch dir: 0 entries, then 1 after touching a standalone archive) |
| 6 | **MINOR** four Python files unformatted | `uvx --offline ruff format` on all four → `4 files already formatted`; `ruff check` now reports **6** findings on them, all pre-existing `ISC004` in `SHIM_REACHES` (HEAD's copies report 10, including the `F841` this node removed). `EXE001` on `test_packaging_gates.py` is gone too |
| 7 | **MINOR** `NODE.md:85` asserts `len(m)==5` on the `{"include": …}` dict | recorded below: the acceptance line is pasted as-written (it fails by construction) and the contract-correct form is pasted with its output. Node text untouched |

### The one-table guard (item 1's "no second copy")

`check_workflows.py` gained **rule 18**, self-tested before every run: no workflow and no
`scripts/build-lsp.sh` may spell an editor target (`\b(x86_64|aarch64)-(unknown-linux|apple-darwin|pc-windows)`),
because the five live in `scripts/platform_matrix.py`. Rule 17 also refuses a shipped `build-lsp.sh`
step that does not pass the table's three names, per leg, after expansion. Two mutations in
`scripts/test_packaging_gates.py` keep both honest (a triple added to `build-lsp.sh` → red; the
`lsp` matrix output dropped → red; an upload renamed → the pairing red; the builder image dropped →
red; `lsp` counted as `archives` → red; a design review quoting a count → green, a page → red).

## What landed

| Path | Change |
|---|---|
| `scripts/platform_matrix.py` | `lsp` family: five legs `(target, runs_on, archive, executable)` in `LSP_LEGS`, and **derived** `dir`, `ext`, `executable`, `python`, `builder_image`, `glibc_floor`, `arch`, `ci_command`; `--matrix/--count/--leg/--summary [FAMILY] lsp` (+ `--target`); `PROVEN[("lsp","x86_64-unknown-linux-gnu")]`; page noun `lsp assets?` + key `lsp asset` + `declared_keys` + a `PINNED` line for ADR-0010's `5`; leg validation; counts only in documents `zensical.toml` publishes (`*.review-*.md` and the site-excluded directories skipped, the two pinned records kept) |
| `scripts/build-lsp.sh` (new, 463 lines) | `cargo build -p blastit-lsp --release --locked --target <target>`; names from the table (`--archive/--dir/--executable`) with the same rule as a default; `tar -czf` / `python -m zipfile -c` (no new dependency); measured glibc floor + `--max-glibc`; `--image` builds in `quay.io/pypa/manylinux_2_28_<arch>`; `getcap` empty; `check-portable-elf.sh`; 0 `pyo3`; refuses a foreign triple |
| `scripts/lsp_smoke.py` (new, 330 lines) | stdlib framed JSON-RPC: `initialize` (capability document incl. `codeActionProvider: true`) → `didOpen` a Blastfile with `query 'SELCT id FROM t'` → one severity-1 diagnostic at `4:15–4:20` → `shutdown` → `exit`, status 0, 0 stderr bytes; `--binary` only; exit 0/1 with one line |
| `.github/workflows/ci.yml` | `matrix` job exporting `lsp`; `lsp` job over `fromJSON(needs.matrix.outputs.lsp)` that builds the table's names, unpacks `matrix.dir` and smokes the extracted file on each native runner; `LIBCLANG_PATH` pinned on Windows with the research §Q1 comment |
| `.github/workflows/publish-pypi.yml` | `lsp` output + job (build → smoke extracted → `SHA256SUMS.lsp-<target>` → `lsp-<target>`); `release` needs it, downloads `pattern: lsp-*`, counts `--count binaries`/`--count lsp`/`--count shims` with `nullglob` and a family-safe standalone list |
| `scripts/check_workflows.py` | `lsp` in `OUTPUT_SPECS`; `--matrix/--leg` family validation (`--leg lsp` needs `--target`); rule 17 (floor's image + the table's three names) and rule 18 (no re-spelled target), both with a self-test run first |
| `scripts/test_packaging_gates.py` | 6 new mutations: 19 workflow cases (was 17), 14 matrix cases (was 13) — 56 in total |

## Acceptance

```console
$ PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH" uv run --no-project tools/plan.py validate
plan OK
```

```console
# NODE.md:85, as written — the generator emits {"include": [...]} (as it does for wheels/binaries/shims,
# and as `fromJSON` needs), so `m` is a dict and this line fails by construction. See Follow-ups.
$ scripts/platform_matrix.py --matrix lsp | python3 -c 'import json,sys; m=json.load(sys.stdin); assert len(m)==5; print([e["target"] for e in m])'
AssertionError
$ scripts/platform_matrix.py --matrix lsp | python3 -c 'import json,sys; m=json.load(sys.stdin)["include"]; assert len(m)==5; print([e["target"] for e in m])'
['x86_64-unknown-linux-gnu', 'aarch64-unknown-linux-gnu', 'aarch64-apple-darwin', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc']
$ scripts/platform_matrix.py --count lsp
5
```

```console
$ scripts/platform_matrix.py --summary lsp        # one family, now that the option takes one
| family | declared | built & exercised | dropped (reason) |
|---|---|---|---|
| lsp assets (editor) | 5 | 1 of 5 | — |

Declared is what CI builds on the release PR; built & exercised is what has been run (PROVEN).

| artifact | leg | status |
|---|---|---|
| `blastit-lsp-x86_64-unknown-linux-gnu.tar.gz` | ubuntu-24.04 | proved by MB.35.5; the command CI runs is `bash scripts/build-lsp.sh --target x86_64-unknown-linux-gnu --image quay.io/pypa/manylinux_2_28_x86_64 --max-glibc 2_28 --archive blastit-lsp-x86_64-unknown-linux-gnu.tar.gz --executable blastit-lsp --dir blastit-lsp-x86_64-unknown-linux-gnu --out dist && tar -xzf dist/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz -C dist/smoke-x86_64-unknown-linux-gnu && python3 scripts/lsp_smoke.py --binary dist/smoke-x86_64-unknown-linux-gnu/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp` |
| `blastit-lsp-aarch64-unknown-linux-gnu.tar.gz` | ubuntu-24.04-arm | declared, unproven — NOT RUN (no CI runner or cross toolchain on this host); the command CI runs is `bash scripts/build-lsp.sh --target aarch64-unknown-linux-gnu --image quay.io/pypa/manylinux_2_28_aarch64 --max-glibc 2_28 --archive blastit-lsp-aarch64-unknown-linux-gnu.tar.gz --executable blastit-lsp --dir blastit-lsp-aarch64-unknown-linux-gnu --out dist && tar -xzf dist/blastit-lsp-aarch64-unknown-linux-gnu.tar.gz -C dist/smoke-aarch64-unknown-linux-gnu && python3 scripts/lsp_smoke.py --binary dist/smoke-aarch64-unknown-linux-gnu/blastit-lsp-aarch64-unknown-linux-gnu/blastit-lsp` |
| `blastit-lsp-aarch64-apple-darwin.tar.gz` | macos-latest | declared, unproven — NOT RUN (no CI runner or cross toolchain on this host); the command CI runs is `bash scripts/build-lsp.sh --target aarch64-apple-darwin --archive blastit-lsp-aarch64-apple-darwin.tar.gz --executable blastit-lsp --dir blastit-lsp-aarch64-apple-darwin --out dist && tar -xzf dist/blastit-lsp-aarch64-apple-darwin.tar.gz -C dist/smoke-aarch64-apple-darwin && python3 scripts/lsp_smoke.py --binary dist/smoke-aarch64-apple-darwin/blastit-lsp-aarch64-apple-darwin/blastit-lsp` |
| `blastit-lsp-x86_64-apple-darwin.tar.gz` | macos-15-intel | declared, unproven — NOT RUN (no CI runner or cross toolchain on this host); the command CI runs is `bash scripts/build-lsp.sh --target x86_64-apple-darwin --archive blastit-lsp-x86_64-apple-darwin.tar.gz --executable blastit-lsp --dir blastit-lsp-x86_64-apple-darwin --out dist && tar -xzf dist/blastit-lsp-x86_64-apple-darwin.tar.gz -C dist/smoke-x86_64-apple-darwin && python3 scripts/lsp_smoke.py --binary dist/smoke-x86_64-apple-darwin/blastit-lsp-x86_64-apple-darwin/blastit-lsp` |
| `blastit-lsp-x86_64-pc-windows-msvc.zip` | windows-latest | declared, unproven — NOT RUN (no CI runner or cross toolchain on this host); the command CI runs is `bash scripts/build-lsp.sh --target x86_64-pc-windows-msvc --archive blastit-lsp-x86_64-pc-windows-msvc.zip --executable blastit-lsp.exe --dir blastit-lsp-x86_64-pc-windows-msvc --out dist && python -m zipfile -e dist/blastit-lsp-x86_64-pc-windows-msvc.zip dist/smoke-x86_64-pc-windows-msvc && python scripts/lsp_smoke.py --binary dist/smoke-x86_64-pc-windows-msvc/blastit-lsp-x86_64-pc-windows-msvc/blastit-lsp.exe` |

An lsp leg is proved only after `scripts/lsp_smoke.py` drove a real session against **the archive it publishes** on that leg's native runner; a built-but-unsmoked leg is declared, unproven, and carries the command that would prove it.
```

```console
$ bash scripts/build-lsp.sh --target x86_64-unknown-linux-gnu --image quay.io/pypa/manylinux_2_28_x86_64 --max-glibc 2_28 --archive blastit-lsp-x86_64-unknown-linux-gnu.tar.gz --dir blastit-lsp-x86_64-unknown-linux-gnu --executable blastit-lsp --out target/lsp-dist
...
ok   /src/target/lsp-2_28/x86_64-unknown-linux-gnu/release/blastit-lsp: NEEDED libgcc_s.so.1 libpthread.so.0 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
build-lsp: getcap empty (supported unprivileged)
build-lsp: glibc floor 2.28 (readelf --version-info)
build-lsp: floor 2.28 <= --max-glibc 2.28
build-lsp: 2000669 bytes -> target/lsp-dist/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
build-lsp: entries -> blastit-lsp-x86_64-unknown-linux-gnu/{blastit-lsp,LICENSE,SHA256SUMS}
build-lsp: checksum -> target/lsp-dist/SHA256SUMS
```

```console
$ tmp=$(mktemp -d); mkdir -p "$tmp/blastit-lsp-0.2.0"
$ tar -xzf target/lsp-dist/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz -C "$tmp/blastit-lsp-0.2.0"
$ ( cd "$tmp/blastit-lsp-0.2.0/blastit-lsp-x86_64-unknown-linux-gnu" && ls | sort | tr '\n' ',' )
LICENSE,SHA256SUMS,blastit-lsp,
$ ( cd "$tmp/blastit-lsp-0.2.0/blastit-lsp-x86_64-unknown-linux-gnu" && sha256sum -c SHA256SUMS )
blastit-lsp: OK
LICENSE: OK
$ uv run --no-project scripts/lsp_smoke.py --binary "$tmp/blastit-lsp-0.2.0/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp"
lsp_smoke: ok — …/blastit-lsp-x86_64-unknown-linux-gnu/blastit-lsp: initialize + codeActionProvider, one severity-1 diagnostic at 4:15, shutdown/exit 0, 0 bytes on stderr
```

Same asset inside **Debian 12** (`glibc 2.36`), driven through a `docker run -i` wrapper handed to the
smoke script (so the container's own stderr counts against the 0-byte assertion):

```console
$ uv run --no-project scripts/lsp_smoke.py --binary /tmp/opencode/lsp-in-debian12.sh
lsp_smoke: ok — /tmp/opencode/lsp-in-debian12.sh: initialize + codeActionProvider, one severity-1 diagnostic at 4:15, shutdown/exit 0, 0 bytes on stderr
$ docker run --rm … debian:bookworm-slim sh -c 'getconf GNU_LIBC_VERSION; ldd …'
glibc 2.36
        libgcc_s.so.1 => /lib/x86_64-linux-gnu/libgcc_s.so.1 …
```

Denied paths:

```console
$ uv run --no-project scripts/lsp_smoke.py --binary /bin/true
lsp_smoke: FAIL — /bin/true: stdout closed while waiting for the reply to `initialize`
exit=1
$ bash scripts/build-lsp.sh --target aarch64-unknown-linux-musl --archive blastit-lsp-aarch64-unknown-linux-musl.tar.gz
build-lsp.sh: aarch64-unknown-linux-musl is not an OS this archive ships a language
  server for (the shapes are *-unknown-linux-gnu, *-apple-darwin and
  *-pc-windows-msvc; the five targets are the table:
  uv run --no-project scripts/platform_matrix.py --matrix lsp)
exit=2
$ bash scripts/build-lsp.sh --archive wrong.zip
build-lsp.sh: --archive 'wrong.zip' does not end in .tar.gz, the format
  linux packages (its asset is `blastit-lsp-<target>.tar.gz`)
exit=2
$ bash scripts/build-lsp.sh --image quay.io/pypa/manylinux_2_28_x86_64
error: --image needs --max-glibc (e.g. 2_28): the image is the floor, and a
  build inside it that never asserts the floor is a container with extra steps
exit=2
```

```console
$ make workflow-check platform-matrix-check packaging-gates-test
workflow-check: rule 15 self-test ok — `make standalone` is a host build
workflow-check: rule 16 self-test ok — a CARGO_HOME symlink is a read-only write
workflow-check: rule 17 self-test ok — a shipped blastit-lsp is built in its floor's image
workflow-check: rule 18 self-test ok — an editor target triple outside the table is a copy
workflow-check: ok — 447 checks over 6 workflows and 1 actions, 0 problem(s)
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; reaches: served 6 of 12 shims, 0 probed, 0 refused, 6 unbuilt; 12 served once the aarch64 legs run; 0 leg(s) dropped
packaging-gates-test: ok — 56 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7)
```

```console
$ make check          # full: fmt, clippy, doc, every workspace suite, docs, all guards
…
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; …
workflow-check: rule 15 self-test ok — `make standalone` is a host build
workflow-check: rule 16 self-test ok — a CARGO_HOME symlink is a read-only write
workflow-check: rule 17 self-test ok — a shipped blastit-lsp is built in its floor's image
workflow-check: rule 18 self-test ok — an editor target triple outside the table is a copy
workflow-check: ok — 447 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 56 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7)
…
zed-check: OK (47 properties checked)
lsp-portable: ok — crates/blastit-lsp: [dependencies] = ['blastit-manifest', 'serde_json'] with no build script, no target table and no feature; 3 source file(s) with none of 10 banned spellings and no `cfg` but `#[cfg(test)]`; 60 packages in the normal closure, none of 12 forbidden; one home; `make check` runs this target
lsp-portable self-test: ok — 47 assertions over 15 fixtures
plan OK
$ echo $?
0
```

### The drift guard, both directions

One edit to the table, in a throwaway copy (`/tmp/opencode/drift/repo`), and nothing else:

```console
# LSP_LEGS gains {"target": "aarch64-pc-windows-msvc", "runs_on": "windows-11-arm", "platform": "windows"}
$ scripts/platform_matrix.py --matrix lsp
6 legs
x86_64-unknown-linux-gnu   | ubuntu-24.04      | blastit-lsp-x86_64-unknown-linux-gnu.tar.gz   | blastit-lsp-x86_64-unknown-linux-gnu   | blastit-lsp
aarch64-unknown-linux-gnu  | ubuntu-24.04-arm  | blastit-lsp-aarch64-unknown-linux-gnu.tar.gz  | blastit-lsp-aarch64-unknown-linux-gnu  | blastit-lsp
aarch64-apple-darwin       | macos-latest      | blastit-lsp-aarch64-apple-darwin.tar.gz       | blastit-lsp-aarch64-apple-darwin       | blastit-lsp
x86_64-apple-darwin        | macos-15-intel    | blastit-lsp-x86_64-apple-darwin.tar.gz        | blastit-lsp-x86_64-apple-darwin        | blastit-lsp
aarch64-pc-windows-msvc    | windows-11-arm    | blastit-lsp-aarch64-pc-windows-msvc.zip       | blastit-lsp-aarch64-pc-windows-msvc    | blastit-lsp.exe
x86_64-pc-windows-msvc     | windows-latest    | blastit-lsp-x86_64-pc-windows-msvc.zip        | blastit-lsp-x86_64-pc-windows-msvc     | blastit-lsp.exe
$ check_workflows.py --repo /tmp/opencode/drift/repo     # the same two workflows, unchanged
workflow-check: ok — 452 checks over 6 workflows and 1 actions, 0 problem(s)
# ... and build-lsp.sh, unchanged, already understands the new leg by its OS shape
$ bash scripts/build-lsp.sh --target aarch64-pc-windows-msvc --archive blastit-lsp-aarch64-pc-windows-msvc.zip --executable blastit-lsp.exe --dir blastit-lsp-aarch64-pc-windows-msvc --out /tmp/opencode/never
build-lsp.sh: aarch64-pc-windows-msvc is not this host (x86_64-unknown-linux-gnu), so it cannot be smoked here
exit=2   # the host refusal, not an unknown target: the shape, the .zip format and the names were accepted
```

The other direction — a triple re-appearing outside the table — is a red (gates case 19):

```console
$ printf '\n# legacy: bash build-lsp.sh --target x86_64-apple-darwin\n' >> scripts/build-lsp.sh   # fixture copy
$ scripts/check_workflows.py --repo /tmp/opencode/rule18/repo
FAIL scripts/build-lsp.sh: line 465: spells the editor target 'x86_64-apple-darwin', a second copy of the table (the five targets are `scripts/platform_matrix.py --matrix lsp`): take the leg from `${{ matrix.* }}` or match an OS shape instead
workflow-check: FAIL — 448 checks over 6 workflows and 1 actions, 1 problem(s)
```

### The review-file gate, both directions (kept from attempt 1)

```console
$ printf 'The matrix declares 9 archives today, and 3 proven wheels.\n' > "$tmp/docs/design/portable-lsp-arch.review-implementability.md"
$ uv run --no-project python "$tmp/scripts/platform_matrix.py" --check --quiet
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; …
exit=0
$ printf '\nThe matrix declares 9 archives today.\n' >> "$tmp/docs/getting-started.md"
$ uv run --no-project python "$tmp/scripts/platform_matrix.py" --check --quiet
platform-matrix: docs/getting-started.md:72: claims 9 archives, the matrix declares 4: a page may not promise a leg that is not attempted
exit=1
```

`ruff` (item 6):

```console
$ uvx --offline ruff format --check scripts/platform_matrix.py scripts/check_workflows.py scripts/test_packaging_gates.py scripts/lsp_smoke.py
4 files already formatted
$ uvx --offline ruff check scripts/platform_matrix.py scripts/check_workflows.py scripts/test_packaging_gates.py scripts/lsp_smoke.py
scripts/platform_matrix.py:244:9: ISC004 … (×6, all in SHIM_REACHES — HEAD's copies report 10 findings)
Found 6 errors.        # none new; the F841 and EXE001 HEAD carried are gone
```

## NOT RUN — exact `ci_command` per leg

| Target | Runner | `ci_command` (also printed by `--summary lsp`) |
|---|---|---|
| `aarch64-unknown-linux-gnu` | `ubuntu-24.04-arm` | `bash scripts/build-lsp.sh --target aarch64-unknown-linux-gnu --image quay.io/pypa/manylinux_2_28_aarch64 --max-glibc 2_28 --archive blastit-lsp-aarch64-unknown-linux-gnu.tar.gz --executable blastit-lsp --dir blastit-lsp-aarch64-unknown-linux-gnu --out dist && tar -xzf dist/blastit-lsp-aarch64-unknown-linux-gnu.tar.gz -C dist/smoke-aarch64-unknown-linux-gnu && python3 scripts/lsp_smoke.py --binary dist/smoke-aarch64-unknown-linux-gnu/blastit-lsp-aarch64-unknown-linux-gnu/blastit-lsp` |
| `aarch64-apple-darwin` | `macos-latest` | `bash scripts/build-lsp.sh --target aarch64-apple-darwin --archive blastit-lsp-aarch64-apple-darwin.tar.gz --executable blastit-lsp --dir blastit-lsp-aarch64-apple-darwin --out dist && tar -xzf dist/blastit-lsp-aarch64-apple-darwin.tar.gz -C dist/smoke-aarch64-apple-darwin && python3 scripts/lsp_smoke.py --binary dist/smoke-aarch64-apple-darwin/blastit-lsp-aarch64-apple-darwin/blastit-lsp` |
| `x86_64-apple-darwin` | `macos-15-intel` | `bash scripts/build-lsp.sh --target x86_64-apple-darwin --archive blastit-lsp-x86_64-apple-darwin.tar.gz --executable blastit-lsp --dir blastit-lsp-x86_64-apple-darwin --out dist && tar -xzf dist/blastit-lsp-x86_64-apple-darwin.tar.gz -C dist/smoke-x86_64-apple-darwin && python3 scripts/lsp_smoke.py --binary dist/smoke-x86_64-apple-darwin/blastit-lsp-x86_64-apple-darwin/blastit-lsp` |
| `x86_64-pc-windows-msvc` | `windows-latest` | `echo 'LIBCLANG_PATH=C:\Program Files\LLVM\bin' >> $GITHUB_ENV` then `bash scripts/build-lsp.sh --target x86_64-pc-windows-msvc --archive blastit-lsp-x86_64-pc-windows-msvc.zip --executable blastit-lsp.exe --dir blastit-lsp-x86_64-pc-windows-msvc --out dist && python -m zipfile -e dist/blastit-lsp-x86_64-pc-windows-msvc.zip dist/smoke-x86_64-pc-windows-msvc && python scripts/lsp_smoke.py --binary dist/smoke-x86_64-pc-windows-msvc/blastit-lsp-x86_64-pc-windows-msvc/blastit-lsp.exe` |

`--summary lsp` prints each with `declared, unproven — NOT RUN (no CI runner or cross toolchain on
this host)`. The zip *layout* was checked on this host with the same archiver the script picks
(synthetic stage): `python -m zipfile -l` and a round-trip extraction give exactly
`blastit-lsp-x86_64-pc-windows-msvc/{blastit-lsp.exe,LICENSE,SHA256SUMS}` — the layout, not a Windows run.

## First-CI-run checklist

| What | Job |
|---|---|
| five legs resolve, build and smoke with the table's names | `ci.yml` `lsp` (needs `ci.yml` `matrix`) |
| the four unproven targets | `ci.yml` `lsp`: `ubuntu-24.04-arm`, `macos-latest`, `macos-15-intel`, `windows-latest` |
| the published assets, `lsp-<target>` artifacts, `SHA256SUMS.lsp-<target>` | `publish-pypi.yml` `lsp` |
| the three counts, one release `SHA256SUMS`, the two families counted separately | `publish-pypi.yml` `release` |
| `LIBCLANG_PATH` is what the Windows leg needed | `ci.yml`/`publish-pypi.yml` `lsp`, `windows-latest` only |

## attempt 2, addendum — the checksum tool is the leg's, not the script's

**The blocker.** `scripts/build-lsp.sh` ran GNU `sha256sum` on every leg. The `macos-*` runner images
ship no coreutils — `sha256sum` does not exist there — so both Apple legs would have died at the
checksum step, before the smoke, and the release job (which needs `lsp`) would never have run.
`--summary` would have kept calling them "declared, unproven" for a reason that was a script bug.

**The fix.**
- One selector, three tools, one format (`build-lsp.sh:325-380`):
  `sha256sum` when it exists (Linux, Windows/Git Bash, manylinux images), else `shasum -a 256`
  (macOS, from Perl), overridable with `BLASTIT_SHA_TOOL=sha256sum|shasum` so both branches are
  testable on one host. The written line is `<hex>  <name>` for both tools, so a macOS-written
  `SHA256SUMS` verifies on Linux and the reverse; `shasum -c` alone means **SHA-1**, so `-a 256` is
  part of every verify command. The stage loop no longer pipes through `xargs`: it hashes each
  staged file with the selected tool and then **verifies the file it just wrote**, so a tool that
  cannot read its own output fails at build time, not on a user's machine.
- The matrix derives `sha_verify` per leg (`sha256sum -c SHA256SUMS`, or
  `shasum -a 256 -c SHA256SUMS` on macOS); both workflows now run it against the unpacked
  directory, and `ci_command` includes the same `( cd dist/smoke-<target>/<dir> && <sha_verify> )`
  step, so `--summary` shows exactly what CI runs.
- A host with neither tool is refused before the compile, naming both fixes; an unavailable forced
  tool and a nonsense value are refused too.

**Proof — the same build twice on this host (Linux), branching on the tool alone:**

```console
$ BLASTIT_SHA_TOOL=sha256sum bash scripts/build-lsp.sh --target x86_64-unknown-linux-gnu \
    --archive blastit-lsp-x86_64-unknown-linux-gnu.tar.gz --dir blastit-lsp-x86_64-unknown-linux-gnu \
    --executable blastit-lsp --out /tmp/opencode/sha-coreutils
blastit-lsp: OK
blastit-lsp-x86_64-unknown-linux-gnu.tar.gz: OK
build-lsp: 2000283 bytes -> /tmp/opencode/sha-coreutils/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
build-lsp: entries -> blastit-lsp-x86_64-unknown-linux-gnu/{blastit-lsp,LICENSE,SHA256SUMS}
build-lsp: checksums written and verified with sha256sum -> /tmp/opencode/sha-coreutils/SHA256SUMS
$ BLASTIT_SHA_TOOL=shasum bash scripts/build-lsp.sh --target x86_64-unknown-linux-gnu \
    --archive … --dir … --executable blastit-lsp --out /tmp/opencode/sha-shasum
blastit-lsp: OK
blastit-lsp-x86_64-unknown-linux-gnu.tar.gz: OK
build-lsp: 2000299 bytes -> /tmp/opencode/sha-shasum/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
build-lsp: entries -> blastit-lsp-x86_64-unknown-linux-gnu/{blastit-lsp,LICENSE,SHA256SUMS}
build-lsp: checksums written and verified with shasum -> /tmp/opencode/sha-shasum/SHA256SUMS
```

Both written files, each verified by **both** tools (an extracted tree, so the in-archive list is what
a consumer sees; then the release-level digest):

```console
### sha-coreutils: in-archive SHA256SUMS
a6cba85bc92e0cff7a450b1d873c0eaa2e9fc96bf472df0247a26bec77bf3ff9  LICENSE
ca1b9eac5faa0f07bb669bc985a11a8853587f3df717592cbb187e540ac76cce  blastit-lsp
-- verified with sha256sum -c --            -- verified with shasum -a 256 -c --
LICENSE: OK                                 LICENSE: OK
blastit-lsp: OK                             blastit-lsp: OK
### sha-coreutils: release-level SHA256SUMS
3bb2d6ad84e5fbaebc072e73891648679be8690bf25fb0ed8d6c8fc2cb067837  blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
blastit-lsp-x86_64-unknown-linux-gnu.tar.gz: OK      (sha256sum -c and shasum -a 256 -c)
### sha-shasum: in-archive SHA256SUMS        (byte-identical digests to the coreutils leg)
a6cba85bc92e0cff7a450b1d873c0eaa2e9fc96bf472df0247a26bec77bf3ff9  LICENSE
ca1b9eac5faa0f07bb669bc985a11a8853587f3df717592cbb187e540ac76cce  blastit-lsp
-- verified with sha256sum -c --            -- verified with shasum -a 256 -c --
LICENSE: OK                                 LICENSE: OK
blastit-lsp: OK                             blastit-lsp: OK
### sha-shasum: release-level SHA256SUMS
1f62bb81923c056e0c6e41fafff2a4379a232147ae5e8e0fa46b37e7c0e703ec  blastit-lsp-x86_64-unknown-linux-gnu.tar.gz
blastit-lsp-x86_64-unknown-linux-gnu.tar.gz: OK      (sha256sum -c and shasum -a 256 -c)
```

(The two archives differ because gzip stamps its own mtime; the in-archive lists do not, because the
staged bytes are identical. Nothing here claims a reproducible archive.)

Refusals, all loud and naming the fix (a PATH with neither tool, then a forced-but-absent tool, then
an unknown value):

```console
$ PATH=/tmp/opencode/sha-none/bin RUSTC=$HOME/.cargo/bin/rustc bash scripts/build-lsp.sh --out …
build-lsp.sh: neither 'sha256sum' nor 'shasum' is on PATH, so the
  archive's SHA256SUMS cannot be written (an empty one verifies nothing)
  fix: install coreutils (sha256sum), or perl's shasum (macOS has it:
  /usr/bin/shasum). BLASTIT_SHA_TOOL=sha256sum|shasum forces one
exit=1
$ … BLASTIT_SHA_TOOL=sha256sum bash scripts/build-lsp.sh --out …
build-lsp.sh: BLASTIT_SHA_TOOL=sha256sum but 'sha256sum' is not on PATH
  install it, or unset BLASTIT_SHA_TOOL to use whichever tool this host has
exit=1
$ … BLASTIT_SHA_TOOL=md5 bash scripts/build-lsp.sh --out …
build-lsp.sh: BLASTIT_SHA_TOOL='md5' is not one of sha256sum, shasum
exit=2
```

The shipped Linux asset, rebuilt through the image path after the change, still unpacks to three
entries, verifies with both tools and smokes clean:

```console
$ tar -xzf target/lsp-dist/blastit-lsp-x86_64-unknown-linux-gnu.tar.gz -C …
$ ( cd …/blastit-lsp-x86_64-unknown-linux-gnu && sha256sum -c SHA256SUMS && shasum -a 256 -c SHA256SUMS )
blastit-lsp: OK
LICENSE: OK
blastit-lsp: OK
LICENSE: OK
$ uv run --no-project scripts/lsp_smoke.py --binary …/blastit-lsp
lsp_smoke: ok — …: initialize + codeActionProvider, one severity-1 diagnostic at 4:15, shutdown/exit 0, 0 bytes on stderr
```

`ci_command` now carries the verification, per leg, and the Apple legs name `shasum -a 256 -c`:

```console
$ scripts/platform_matrix.py --matrix lsp   # (apple leg)
bash scripts/build-lsp.sh --target aarch64-apple-darwin --archive blastit-lsp-aarch64-apple-darwin.tar.gz --executable blastit-lsp --dir blastit-lsp-aarch64-apple-darwin --out dist && tar -xzf dist/blastit-lsp-aarch64-apple-darwin.tar.gz -C dist/smoke-aarch64-apple-darwin && ( cd dist/smoke-aarch64-apple-darwin/blastit-lsp-aarch64-apple-darwin && shasum -a 256 -c SHA256SUMS ) && python3 scripts/lsp_smoke.py --binary dist/smoke-aarch64-apple-darwin/blastit-lsp-aarch64-apple-darwin/blastit-lsp
```

```console
$ make workflow-check platform-matrix-check packaging-gates-test
workflow-check: rule 15 self-test ok — `make standalone` is a host build
workflow-check: rule 16 self-test ok — a CARGO_HOME symlink is a read-only write
workflow-check: rule 17 self-test ok — a shipped blastit-lsp is built in its floor's image
workflow-check: rule 18 self-test ok — an editor target triple outside the table is a copy
workflow-check: ok — 449 checks over 6 workflows and 1 actions, 0 problem(s)
platform-matrix: ok — 8 wheels, 4 archives, 12 shims, 5 lsp assets declared; built and exercised: 4/8 wheels, 2/4 archives, 6/12 shims, 1/5 lsp assets; reaches: served 6 of 12 shims, 0 probed, 0 refused, 6 unbuilt; 12 served once the aarch64 legs run; 0 leg(s) dropped
packaging-gates-test: ok — 56 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7)
$ make check && echo $?
… platform-matrix: ok … workflow-check: ok — 449 checks … packaging-gates-test: ok — 56 cases … lsp-portable: ok … plan OK
0
```

What remains a CI-only fact: that `macos-latest`/`macos-15-intel` really do have `/usr/bin/shasum`
(Perl is in the base image and `shasum` is a base Perl script) — the first run of the `lsp` job on
those two legs is the proof, and it is the same command the summary prints.

## Follow-ups

1. **`NODE.md:85`'s acceptance line indexes the wrong level** (item 7, recorded as the note it
   carries): `--matrix lsp` emits `{"include": [...]}` because `fromJSON` and the three existing
   families require that shape, so `json.load(sys.stdin)` is a dict and `len(m)==5` fails by
   construction. The intended assertion is `json.load(sys.stdin)["include"]` (pasted above, 5
   targets). Fix the node text, not the contract.
2. `publish-pypi.yml`'s release job prefixes the editor family's per-leg checksum
   `SHA256SUMS.lsp-<target>`: `merge-multiple: true` would otherwise collide with the standalone
   job's `SHA256SUMS.<target>` for the two shared Linux triples. Cross-step collisions are invisible
   to rule 10, which only sees one download step — so if a later leaf renames either family's file,
   both must move together and neither rule will say so.
3. `docs/**` was not edited (out of `paths`): ADR-0010's `5` lsp assets row already existed and is
   now *checked* (`PINNED`); nothing in the pages had to move for the count to agree.
