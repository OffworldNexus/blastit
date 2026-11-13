---
id: MA.3.6
title: "Bump 0.1.1 (Cargo.toml/Cargo.lock), changelog entry and wheel metadata proof"
kind: task
status: done
owner: implementer
priority: 1
depends_on: [MA.3.1, MA.3.2, MA.3.3, MA.3.4, MA.3.5]
paths: [Cargo.toml, Cargo.lock, CHANGELOG.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-12T19:16"
updated: "2026-11-13T03:10"
---

# Goal

Bump the workspace to `0.1.1` (`Cargo.toml` + `Cargo.lock`), write the `0.1.1` changelog entry, and prove the release metadata (wheel version and PyPI description rendering) before the tag.

# Context

- `RELEASING.md`: a release *is* the tag `v` + `[workspace.package] version`; `scripts/check-release-tag.sh` refuses a mismatch, and `scripts/dev-version.sh` is for CI development wheels only — never run it here.
- This task is strictly sequential: nothing else may run while `Cargo.toml`/`Cargo.lock` change, so no parallel Rust task and no concurrent `make check`.
- Everything it depends on is already green (`MA.3.1`–`MA.3.5`); only the version string and the changelog are new.
- Load skills `rust-standards`, `dx-first`.

# Acceptance

```console
$ grep -n 'version = "0.1.1"' Cargo.toml
$ CARGO_NET_OFFLINE=1 cargo metadata --locked --format-version 1 > /dev/null
$ scripts/check-release-tag.sh v0.1.1
$ git diff --stat Cargo.lock        # only the workspace crates' versions
$ sed -n '1,12p' CHANGELOG.md       # ## 0.1.1 (2026-11-12) first
$ scripts/build-wheel.sh
$ unzip -p dist/blastit-0.1.1-*.whl '*/METADATA' | head -4
$ uv run --no-project scripts/check-long-description.py dist/*
$ CARGO_NET_OFFLINE=1 UV_OFFLINE=1 make check
```

- `Cargo.lock` records `0.1.1` for every workspace crate with no dependency churn (`--locked` metadata passes).
- The wheel is `blastit-0.1.1-*` and its `METADATA` `Version:` is `0.1.1`.
- `CHANGELOG.md` opens with `## 0.1.1 (2026-11-12)`: user-visible changes only (zero-`sudo` local run, single-pass manifest fill, `blastit check`, lean docs), one line each, linking the DX pages.
- `make check` (including `docs-test`) is green after the bump.

# Notes

- Never `git tag`, `git push`, or touch `.github/workflows/`; the `MA` milestone gate and `HUMAN.md` own tagging.
- `make e2e` already ran in `MA.3.4` on this code; the bump changes no behaviour, so do not re-run Docker here.
