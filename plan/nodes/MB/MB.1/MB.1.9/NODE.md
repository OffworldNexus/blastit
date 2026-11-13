---
id: MB.1.9
title: "Align intra-workspace version requirements and harden scripts/dev-version.sh"
kind: task
status: active
owner: implementer
priority: 1
depends_on: []
paths: ["scripts/dev-version.sh"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T14:44"
updated: "2026-11-13T14:44"
---

# Goal

Repair the dev-wheel release pipeline: align every intra-workspace `version` requirement with `[workspace.package] version`, and make `scripts/dev-version.sh` fail loudly instead of silently producing `0.1.1-dev<N>` manifests that no requirement accepts.

# Context

- **Failing job (owner-reported):** publish run `43426326826`, job `131888508825` — `wheel (CPython 3.13)`, step `./.github/actions/build-wheel`:
  `error: failed to select a version for the requirement 'blastit-dbproxy = "^0.1.0"'` / `candidate versions found which didn't match: 0.1.1-dev14` / `required by package 'blastit v0.1.1-dev14'`.
- **Root cause, re-verified on `release/0.2.0` at `cab377e`:** `Cargo.toml` `[workspace.package] version = "0.1.1"` but all **28** intra-workspace requirements still read `version = "0.1.0", path = "../..."` (`crates/blastit/Cargo.toml:18-28`, `crates/blastit-dbproxy/Cargo.toml:23-25`, `crates/blastit-http:14-15`, `crates/blastit-manifest:14-15`, `crates/blastit-py:33,38-40`, `crates/blastit-scheduler:14-17`, `crates/blastit-view:14`). A release build works because `0.1.1` satisfies `^0.1.0`; a **pre-release** does not, because a prerelease only matches a requirement that itself names a prerelease.
- **Why it went silent:** `scripts/dev-version.sh` rewrites `s/version = "<workspace version>", path/.../`, so with the workspace at `0.1.1` it matched nothing, left `^0.1.0` in place, and still exited 0 (its only proof, `cargo metadata --locked --no-deps`, does not resolve requirement compatibility).
- **Blast radius:** every `develop` dev wheel (3.13, 3.14, `sdist`, the `uv virtualenv` job) is unbuildable, so it blocks `0.2.0` verification.
- Release-infrastructure only: **no product behaviour change**, no public docs.

# Acceptance

- [ ] Every intra-workspace requirement names the workspace version (`grep -rn 'version = "0\.1\.0", path' crates/*/Cargo.toml` -> no matches; `grep -c 'version = "0\.1\.1", path' crates/*/Cargo.toml` -> sum 28) and `cargo metadata --locked --no-deps --format-version 1 > /dev/null` passes on the untouched tree (`make check` green proves `--locked` agreement).
- [ ] `scripts/dev-version.sh` hardened (bash + stdlib only, no new deps): the requirement rewrite is tolerant of a lagging requirement (matches any release version before `, path = "../`), and after rewriting it verifies that **no** intra-workspace requirement still names a bare release version — failing loudly with `file:line`. Existing Cargo.lock line-rewrite behaviour and the final `cargo metadata --locked --no-deps` proof are kept.
- [ ] Proof on a **throwaway copy** (the script mutates manifests — copy the tree, e.g. `cp -a` into `/tmp/opencode/mb1-9/`, never run it in the repo): `scripts/dev-version.sh 14` on the copy exits 0, `cargo metadata --locked --no-deps` succeeds there, and `cargo build --locked -p blastit-py` succeeds with the `0.1.1-dev14` manifests (paste tails; also state whether a full `scripts/build-wheel.sh` run was attempted).
- [ ] Guard firing demonstrated on the copy: revert one requirement to `version = "0.1.0", path = "../blastit-events"` and paste the script's non-zero failure output naming the offending `file:line`.
- [ ] `make check` exit 0; `uv run --no-project tools/plan.py validate` -> `plan OK`; `uv run --no-project tools/critical_guard.py --base cab377e --node <ID>` clean (no protected test touched).
- [ ] `RESULT.md` states what the next publish/dev-wheel run on `develop` must show: `wheel (CPython 3.13)` and `wheel (CPython 3.14)` green, artifact named `blastit-0.1.1.dev<N>-*.whl` (`sdist` and the `uv virtualenv` job green too), and that no `Cargo.toml` requirement may lag the workspace version again.

# Notes

- **SEQUENTIAL, never parallel:** this node and `MB.1.7` both rewrite `Cargo.lock`; never run either concurrently with another Rust leaf.
- Do not bump the workspace version, do not cut a release, do not touch `CHANGELOG.md`.
- If a full wheel build is genuinely too slow, `cargo build --locked -p blastit-py` plus the metadata check is the stated minimum — say exactly what was proved.
- Any further release-pipeline drift discovered goes under `## Follow-ups` in `RESULT.md`.
