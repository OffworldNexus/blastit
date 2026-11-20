---
id: MB.58
title: "Fix Alpine libgcc in run_packaging.py/install.sh and stage-1 child reaping in rs-test-php-image"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths:
  - e2e/run_packaging.py
  - docs/install.sh
  - scripts/install.sh
  - scripts/test_install.sh
  - "crates/blastit/src/**"
  - "crates/blastit-sys/src/**"
  - crates/blastit/tests/e2e_dev.rs
  - "crates/blastit/tests/common/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T23:56"
updated: "2026-11-21T00:17"
---

# Goal
Close the final 2 red items across the entire PR #64 matrix (`ab50fe0` — `CI`, `Python SAPI`, `Docs`, and 26/28 `Publish` jobs are 100% GREEN!):

1. **`Publish` `§3 install.sh --user (gnu) + Alpine musl install` (`e2e/run_packaging.py` `--leg alpine` + `docs/install.sh`):**
   - Bare `alpine:3.21` lacks `libgcc_s.so.1` (`apk add --no-cache libgcc`, documented in `docs/install.md` & `docs/index.md`!).
   - In `e2e/run_packaging.py` (`--leg alpine`), install `libgcc` alongside `curl`/`ca-certificates` when provisioning the `alpine:3.21` container (just as a user follows the documented `apk add libgcc` step, and also verify `/work/fixture` directory creation doesn't get skipped when `install.sh` fails!).
   - In `docs/install.sh`: if running on Alpine (`apk` present):
     * When running as root (not `--user`), install `libgcc` (`apk add --no-cache libgcc`) if `/usr/lib/libgcc_s.so.1` and `/lib/libgcc_s.so.1` are absent!
     * When `--user` is passed on Alpine and `"$prefix/bin/blastit" --version` fails while `libgcc_s.so.1` is absent, include the actionable hint `run: apk add libgcc` in the error message!
   - Prove `uv run --no-project e2e/run_packaging.py --require-legs --leg alpine --musl-archive <musl.tar.gz>` passes locally in Docker!

2. **`PHP SAPI` (`make rs-test-php-image`: `sigterm_and_sigint_reap_the_tree_close_the_port_and_remove_the_root` at `e2e_dev.rs:3114`):**
   - Inside `make rs-test-php-image` (`docker run` where PID 1 is `sh`/`e2e_dev`, NOT `systemd` which reaps orphaned zombies immediately on bare hosts!), `pid 180` (`scheduler` or `http`) either hadn't finished `waitpid` reaping inside `blastit`'s shutdown drain (`stage1` / `supervise`) OR sat in zombie state `Z` (`State: Z` in `/proc/<pid>/stat`) under non-reaping PID 1!
   - Inspect `crates/blastit/src/stage1/` shutdown reaping AND `proc::starttime` in `crates/blastit/tests/e2e_dev.rs:3114`:
     * Ensure `blastit`'s `dev`/production shutdown `waitpid`s ALL forked children (`http`, `scheduler`, and any live views) before exiting `0` (not just the first child!), AND ensure `proc::starttime` / `is_alive` treats `Z` (zombie reparented to container PID 1) or brief kernel exit teardown accurately!
   - Prove `make rs-test-php-image` passes 100% (`12 passed; 0 failed`) locally in Docker!

# Acceptance
- `make rs-test-php-image` passes `12/12` locally in Docker
- `e2e/run_packaging.py --require-legs --leg alpine` passes locally in Docker
- `make fast-check` && `scripts/test_install.sh` green
