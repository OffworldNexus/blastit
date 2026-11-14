---
id: MB.1.7
title: "Build and brand hygiene: wheel README links, CARGO_NET_OFFLINE=true, events serde, logo contrast"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - "scripts/build-wheel.sh"
  - Makefile
  - "scripts/dev-version.sh"
  - "crates/blastit-events/Cargo.toml"
  - "crates/blastit-events/src/**"
  - Cargo.lock
  - docs/brand/logo.svg
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T15:08"
---

# Goal

Make a local wheel build carry the same absolute-link README PyPI renders, spell `CARGO_NET_OFFLINE=true` wherever `maturin >= 1.16` can read it, drop the unused `serde` dependency from `crates/blastit-events`, and give the light-scheme header logo real contrast — all in the one leaf that may touch the workspace lockfile.

# Context

- **SEQUENTIAL LEAF — dispatch it alone.** It rewrites root `Cargo.lock`; a parallel Rust leaf would contend on the lock. Run nothing else that builds the workspace at the same time.
- (a) `scripts/build-wheel.sh` builds the published wheel locally without `scripts/readme-absolute-links.py`, so a locally built `dist/*.whl` long description keeps GitHub-relative links that 404 on PyPI. Rewrite the README before `maturin build` and restore it on **every** exit path (`trap ... EXIT`), so `make check`/`docs-test` never leaves a dirty tree.
- (b) `Makefile` lines 108 and 112 use `CARGO_NET_OFFLINE=1`; cargo accepts `1`, maturin >= 1.16 parses the variable as a strict `"true"`/`"false"` and dies in `docs-test` -> `scripts/build-wheel.sh` (`LESSONS.md`). Use `true` everywhere a maturin run can inherit it.
- (c) `crates/blastit-events/Cargo.toml` declares `serde` with `derive`; the crate only uses `serde_json` (verified: no `serde::` path or `Serialize`/`Deserialize` derive in `src/`). Removing it must not change any behaviour, only the lockfile edge.
- (d) `docs/brand/logo.svg` hardcodes `fill="#fbfaf6"` for the head cutout; on the light header (`#f4efe3`) it is nearly invisible (`docs/testing/VISUAL-MA.3.md`, MINOR finding). The site loads the SVG as an `<img>`, so page CSS cannot reach it: put the light-scheme rule in the asset (inline `@media (prefers-color-scheme: light)` style block). Do not fork a second logo file.

# Acceptance

```console
# 1. No strict-bool-hostile spelling remains where maturin can read it.
$ ! grep -rn 'CARGO_NET_OFFLINE=1' Makefile scripts .github && echo "only true remains"

# 2. A local wheel build rewrites and restores the README, and the wheel is PyPI-correct.
$ ./scripts/build-wheel.sh
$ test -z "$(git status --porcelain README.md)" && echo "README restored"
$ uv run --no-project python - <<'PY'
import glob, zipfile
wheel = sorted(glob.glob("dist/blastit-*.whl"))[-1]
with zipfile.ZipFile(wheel) as z:
    meta = next(n for n in z.namelist() if n.endswith(".dist-info/METADATA"))
    body = z.read(meta).decode()
assert "](docs/" not in body, "a relative link survived into the wheel description"
assert "](ARCHITECTURE.md" not in body, "a relative link survived into the wheel description"
assert "https://github.com/OffworldNexus/blastit/blob/" in body
print("wheel description links are absolute")
PY

# 3. `serde` is gone from blastit-events and the lockfile still locks the workspace.
$ ! grep -qE '^serde *=' crates/blastit-events/Cargo.toml && echo "serde removed"
$ CARGO_NET_OFFLINE=true cargo build --workspace --offline
$ CARGO_NET_OFFLINE=true cargo build --workspace --locked --offline
$ git diff --stat Cargo.lock            # only the blastit-events/serde edges move

# 4. Light-scheme logo contrast (>= 3.0:1, WCAG non-text) against the light header.
$ grep -q 'prefers-color-scheme' docs/brand/logo.svg && echo "light-scheme aware"
$ LIGHT_FILL=$(grep -oE '#[0-9a-fA-F]{6}' docs/brand/logo.svg | sort | uniq -c | sort -rn | head -2 | tail -1 | awk '{print $2}') \
  uv run --no-project python -c '
import os
def lum(h):
    c=[int(h[i:i+2],16)/255 for i in (1,3,5)]
    c=[x/12.92 if x<=0.03928 else ((x+0.055)/1.055)**2.4 for x in c]
    return 0.2126*c[0]+0.7152*c[1]+0.0722*c[2]
a,b=lum(os.environ["LIGHT_FILL"]),lum("#f4efe3")
print(f"contrast {((max(a,b)+0.05)/(min(a,b)+0.05)):.2f}:1")'
# print the ratio; it must be >= 3.00 and be pasted in RESULT.md with the value used.

# 5. Docs site + full gate (docs-test exercises the wheel build, so the trap is proven there).
$ make docs-check
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make check
```

# Notes

- If the contrast snippet cannot find the shipped cutout colour (e.g. you moved it into a CSS custom property), replace step 4's `LIGHT_FILL` with the literal hex and paste the command you actually ran; the ratio, not the command shape, is the contract.
- `make check` runs `docs-test` -> `scripts/build-wheel.sh`: the README must be byte-identical afterwards, otherwise the run fails on `guard`/git status and you have a half-working trap.
- Do not touch `docs/troubleshooting.md` (owned by `MB.1.6`), any public doc page, `crates/blastit-events/src/**` semantics, or the workspace version.
- Removing the dependency is not a licence to add one elsewhere: `Cargo.lock` may only lose edges here.
- **Carried over from `MB.1.9` review (MINOR, fix both while you are in `scripts/dev-version.sh`):** (a) its post-rewrite guard only scans `crates/*/Cargo.toml`, so a future `path = "../..."` requirement declared in the workspace-root `Cargo.toml` would escape both the rewrite and the check — append `Cargo.toml` to the awk scan (no such requirement exists today); (b) its failure message says a requirement "names a release version" even when the real cause is a *versionless* path dependency — reword to "names a version other than `<dev>` (or none)". Re-run the script's own proof on a `/tmp` copy afterwards.
