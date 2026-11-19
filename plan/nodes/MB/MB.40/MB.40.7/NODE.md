---
id: MB.40.7
title: "Fix the 3 PR #64 CI failures: Ubuntu 24.04 AppArmor userns sysctl, Windows build-lsp mktemp, and Bash 4.4 heredoc in manylinux_2_28"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - .github/workflows/ci.yml
  - "scripts/build-lsp.sh"
  - "scripts/build-standalone-in-image.sh"
  - scripts/check_workflows.py
  - scripts/test_packaging_gates.py
  - python/tests/test_release_scripts.py
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T09:57"
updated: "2026-11-19T10:04"
---

# Goal
Fix all 3 GitHub Actions CI failures observed on PR #64 (`release/0.2.0`):
1. `CI / test` (`ubuntu-latest` = Ubuntu 24.04): `critical_fanotify::an_overlay_parent_handle_is_the_at_handle_fid_handle` failed with `/proc/self/setgroups: errno 13` (`EACCES`) because Ubuntu 24.04 enables `kernel.apparmor_restrict_unprivileged_userns=1` by default.
2. `CI & Publish / blastit-lsp (x86_64-pc-windows-msvc)`: `cargo build` succeeded in 2m23s, then `scripts/build-lsp.sh:466` failed with `mktemp: failed to create directory via template '/var/tmp/blastit-lsp-stage.XXXXXX': No such file or directory` because Git Bash on Windows has `/tmp` (and `$RUNNER_TEMP`), not `/var/tmp`.
3. `Publish / standalone (x86_64-gnu & aarch64-gnu)`: `scripts/build-standalone-in-image.sh:274-299` failed inside `quay.io/pypa/manylinux_2_28_*` (AlmaLinux 8, Bash 4.4) with `capabilities: -c: line 49: unexpected EOF while looking for matching ')'` because Bash 4.4 misparses parentheses/quotes inside comments inside `$(cat <<\BUILD_BODY ... BUILD_BODY)` command-substitution heredocs.

# Exact Fixes (Zero Re-Research)
1. **`.github/workflows/ci.yml` (`test` job, before `cargo test`):**
   - Add a step enabling unprivileged user namespaces on Ubuntu 24.04+ runners when the sysctl exists:
     ```yaml
     - name: Allow unprivileged user namespaces for fanotify overlay test (Ubuntu 24.04+)
       run: |
         if [ -e /proc/sys/kernel/apparmor_restrict_unprivileged_userns ]; then
           sudo --non-interactive sysctl -w kernel.apparmor_restrict_unprivileged_userns=0
         fi
     ```
   - Verify `scripts/check_workflows.py` and `scripts/test_packaging_gates.py` pass with this step!
2. **`scripts/build-lsp.sh:466`:**
   - Resolve a portable existing temp base directory instead of assuming `/var/tmp` exists on Windows Git Bash: use `${TMPDIR:-}` if set and a directory, else `/var/tmp` if `[ -d /var/tmp ]`, else `/tmp`!
3. **`scripts/build-standalone-in-image.sh:274-303` (and `scripts/build-lsp.sh:296-318` for symmetry):**
   - Replace `build_body=$(cat <<\BUILD_BODY ... BUILD_BODY)` + `bash -euo pipefail -c "$build_body"` with writing directly to `/tmp/blastit-inner-build.sh` via `cat <<'BUILD_BODY' > /tmp/blastit-inner-build.sh` (NO `$()` command substitution wrapping the heredoc!), `chmod 0755 /tmp/blastit-inner-build.sh`, and `exec setpriv --reuid "$HOST_UID" --regid "$HOST_GID" --clear-groups env HOME=/tmp/build-home bash -euo pipefail /tmp/blastit-inner-build.sh`.
   - Ensure `scripts/test_packaging_gates.py` and `python/tests/test_release_scripts.py` still pass 100% (check any regexes in those two test files that inspect `build-standalone-in-image.sh` or `build-lsp.sh`!).

# Acceptance
- `make workflow-check packaging-gates-test docs-unit`
- `bash -n scripts/build-standalone-in-image.sh scripts/build-lsp.sh`
- `uv run --no-project tools/plan.py validate`
