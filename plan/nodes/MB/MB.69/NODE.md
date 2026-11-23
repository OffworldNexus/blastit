---
id: MB.69
title: "Generated RPM spec must build non-root: no chown in %install, no live macros in comments"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - scripts/rpm/gen_spec.py
  - scripts/rpm/blastit.spec
  - "scripts/build-rpm.sh"
  - .github/workflows/packages.yml
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-23T09:30"
updated: "2026-11-23T09:43"
---

# Goal

The `Packages` run on `v0.2.1` went red **only** on the two RPM builds (`§3 build the rpm packages for x86_64` / `aarch64`, jobs 137570493074/137570493050), 18 other jobs green. Two genuine defects in the **generated** `scripts/rpm/blastit.spec` — both invisible locally because we built as root in a container, and this is the first time the RPM path ever ran on a GitHub runner:

```
chown: changing ownership of '…/BUILDROOT/blastit-0.2.1-1.x86_64/usr/share/blastit/…': Operation not permitted
error: Bad exit status from /var/tmp/rpm-tmp.… (%install)
```

1. **`%install` chowns the whole buildroot to root** (`scripts/rpm/gen_spec.py:180` emits `chown -R 0:0 %{buildroot}`). `rpmbuild` on a runner is non-root, so every `chown` fails. Ownership belongs in the package metadata, not the filesystem: drop the `chown` and put `%attr(<mode>,root,root)` on the `%files` entries (or a `%defattr` that already covers them), which is what makes the *installed* files root-owned regardless of who built the package.

2. **Generated comments contain unescaped macros**: `Macro expanded in comment on line 17: %{blastit_glibc_core}`. Comments must not carry a live `%{…}`; escape as `%%{…}` (or reword to avoid the macro). A comment that RPM expands is a comment that can break the build later.

Both are in the generator, so fix `scripts/rpm/gen_spec.py` and regenerate; never hand-edit the committed spec.

# Acceptance

- A non-root `rpmbuild` proves the fix, which is the whole point: run the exact `scripts/build-rpm.sh` (or the workflow's command) **as the current non-root user** on this host, and show all seven `.rpm` files produced with `rpm -qpR`/`--recommends` still correct.
- `rpm -qp --dump <rpm>` (or `rpm2cpio | cpio -tv`) shows the payload files recorded as `root:root` **despite** the non-root build — the `%attr` route must actually pin ownership.
- `rpmbuild` emits **no** "Macro expanded in comment" warning.
- `scripts/rpm/gen_spec.py --check` fresh, `package-set.sh --check` fresh, `make fast-check` green.
- If a small guard is cheap and honest, make the RPM build in `packages.yml`/`wheels`-style jobs run as a non-root user so this class cannot silently return; if that is already true in CI, say so explicitly in `RESULT.md` (it is the reason CI caught it).
