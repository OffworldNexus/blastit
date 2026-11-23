---
id: MB.74
title: One bounded dnf retry helper for every image that installs build tooling
kind: task
status: done
owner: implementer
priority: 2
depends_on: []
paths:
  - "scripts/build-package-bundle.sh"
  - "scripts/build-standalone-in-image.sh"
  - "scripts/build-wheel.sh"
  - "scripts/*.sh"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T11:05"
updated: "2026-11-23T11:30"
---

# Goal

The `v0.2.1` `Packages` run failed one job (`§3 build the deb packages for aarch64`) for an **upstream** reason, not ours:

```
Errors during downloading metadata for repository 'appstream':
Error: Failed to download metadata for repo 'appstream': Yum repo downloading error:
  Downloading error(s): repodata/67327f57…-primary.xml.gz
```

This is the AlmaLinux 8 mirror mid-publish (the same window that once killed `standalone (aarch64-gnu)`). It will recur. `scripts/build-wheel.sh` already survives it with a bounded retry around its `dnf install` (3 attempts + `dnf clean all`), but the other two places that install build tooling are single-shot:

- `scripts/build-package-bundle.sh` (the §6.1 bundle assembly, ~line 161)
- `scripts/build-standalone-in-image.sh` (the inner build, ~line 235)

# Change

Give both the same bounded, self-healing shape `build-wheel.sh` uses — and factor it so there is **one** implementation, not three copies:

1. Add the retry helper in one place (a small function sourced by the three scripts, or an existing shared helper if there is one). Shape: up to 3 attempts, `dnf clean all` between, a short sleep, and a final failure that names the repository error rather than a generic exit.
2. Use it in `build-package-bundle.sh` and `build-standalone-in-image.sh`; **replace** `build-wheel.sh`'s inline loop with the shared helper so the retry cannot drift.
3. Transport/retry only: a genuinely missing package must still fail, and the retry must not swallow a real `dnf` error (no `|| true`).
4. Note in each call site why: the distro mirror can serve an index its own metadata does not match while it publishes.

# Acceptance

- A negative proof that the helper still fails: point it at a deliberately broken repo (or a bogus package name) and show it exits non-zero after the bounded attempts, with the repository error in the message.
- A positive proof: run one real `dnf install` of the toolchain through the helper in the `manylinux_2_28` image and show it succeeding.
- `bash -n`/`sh -n` on the changed scripts, the workflow-syntax gate, `make fast-check`.
- `grep` shows exactly one retry implementation across the three scripts.
