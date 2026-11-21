# MB.40.7 — the three PR #64 CI failures

## What changed

1. **Ubuntu 24.04 unprivileged user namespaces (`.github/workflows/ci.yml`, `test` job).**
   A step before `cargo test --workspace --all-targets --locked --features python`
   writes `kernel.apparmor_restrict_unprivileged_userns=0` when the sysctl exists
   (`sudo --non-interactive`, `set -euo pipefail`), so
   `critical_fanotify::an_overlay_parent_handle_is_the_at_handle_fid_handle` can
   write `/proc/self/setgroups` instead of dying with `EACCES` (errno 13). The
   `-e` keeps older images untouched.
2. **Git Bash has no `/var/tmp` (`scripts/build-lsp.sh`, staging).**
   `tmp_base` now resolves `$TMPDIR` when it is a directory, else `/var/tmp`
   when `[ -d /var/tmp ]`, else `/tmp`; `mktemp -d "$tmp_base/blastit-lsp-stage.XXXXXX"`.
   Every `/var/tmp` occurrence in the script is either a comment or on the same
   line as `[ -d /var/tmp ]`.
3. **AlmaLinux 8 Bash 4.4 heredoc (`scripts/build-standalone-in-image.sh` and
   `scripts/build-lsp.sh`).** Both container bodies are now written to
   `/tmp/blastit-inner-build.sh` with `cat <<\BUILD_BODY > /tmp/blastit-inner-build.sh`
   (no `$(cat <<...)` command substitution around the heredoc, which Bash 4.4
   misparsed into `unexpected EOF while looking for matching ')'`), then
   `chmod 0755` and `exec setpriv --reuid "$HOST_UID" --regid "$HOST_GID"
   --clear-groups env HOME=/tmp/build-home bash -euo pipefail /tmp/blastit-inner-build.sh`.
   The delimiter is backslash-quoted rather than apostrophe-quoted: the whole
   container script is one single-quoted `bash -c '...'` argument, so a literal
   `'` would end that argument early — the same trap the old `<<\BUILD_BODY`
   avoided. Semantics are unchanged (a quoted delimiter still disables expansion).

Regression tests added to `python/tests/test_release_scripts.py` (run by
`make docs-unit`): no `$(cat <<`/`bash -c "$build_body"` anywhere in either
script; the body is written to the file, chmodded and executed; both scripts and
both extracted inner bodies pass `bash -n`; and every `/var/tmp` in
`build-lsp.sh` is guarded by `[ -d /var/tmp ]`.

`scripts/check_workflows.py` rules 14/15/16/17/18 were re-read: the copy-seeded
`CARGO_HOME` text, the `setpriv` line and the absence of re-spelled editor
targets are all preserved, so no gate change was needed.

## Evidence

```
$ bash -n scripts/build-standalone-in-image.sh scripts/build-lsp.sh
bash -n: clean
```

```
$ make workflow-check packaging-gates-test docs-unit py-lint
workflow-check: ok — 450 checks over 6 workflows and 1 actions, 0 problem(s)
packaging-gates-test: ok — 56 cases passed (workflow=19, matrix=14, wheel-tag=4, exec-env=12, test-seam=7)
python -m pytest python/tests/test_runnable_docs.py python/tests/runnable_docs/test_harness.py \
    python/tests/test_docs_assets.py python/tests/test_release_scripts.py -q
116 passed, 1 skipped, 65 subtests passed in 1.48s
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
67 files already formatted
```

```
$ uv run --no-project tools/guard.py
guard: OK (3 staged/untracked file(s) checked)
$ uv run --no-project tools/plan.py validate
plan OK
```

Note: `scripts/build-standalone-in-image.sh` did not even pass `bash -n` before this
change (`line 299: syntax error near unexpected token ')'`): the container script's
own heredoc comment carried an apostrophe first and `)` second. Both are gone now.

## Follow-ups

- `Makefile` (`docs-test`) still spells `mktemp -d "$${TMPDIR:-/var/tmp}/blastit-saved-dist.XXXXXX"`;
  Git Bash / macOS hosts without `/var/tmp` would hit the same `No such file or
  directory`. `Makefile` is outside this node's `paths`.
- No Docker daemon / cached `quay.io/pypa/manylinux_2_28_x86_64` was available
  here, so the Bash 4.4 parse is proven structurally (`bash -n` on the extracted
  body plus the zero-`$(cat <<` assertion) rather than by running the image.
