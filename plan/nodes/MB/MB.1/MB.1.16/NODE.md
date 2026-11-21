---
id: MB.1.16
title: "Pin the rooted production missing-manifest message in the Docker e2e"
kind: task
status: done
owner: implementer
priority: 3
depends_on: []
paths: [e2e/rootless/run.py, e2e/rootless/README.md, e2e/README.md]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-13T18:44"
updated: "2026-11-13T19:54"
---

# Goal

Pin the **rooted production** half of the missing-`./blastit.toml` ordering in the Docker e2e, so a regression in `stage0::run` alone (the `dev` path is already pinned) cannot ship.

# Context

- `docs/testing/AUDIT-MB1.md` (`C5`/`G5`). `MB.1.6` reordered both `stage0::run` (production) and `stage0::run_dev`; the protected `critical_config_and_presets.rs` test drives only `blastit dev`, because an unprivileged production boot is refused by the capability gate before config is read. The rooted leg was proven once by hand (`sudo -n`, `MB.1.6` RESULT leg 3) and by no automated test.
- Production surfaces startup output as `app.output` events on **stdout** (`stage0.rs:424`), never on fd 2 — so the assertion belongs on stdout and must not look for stderr text.
- `e2e/rootless/run.py` already builds the prebuilt image (`E2E_IMAGE`) and has `check_negative_boot(image)`; the same image run as root (`--user 0:0`, no capability drop needed) reaches configuration in an empty working directory.

# Acceptance

```console
# 1. A new check next to check_negative_boot, both directions:
#    (a) empty workdir, root: exits 1, stdout has an app.output line naming
#        `./blastit.toml: No such file or directory`, and no `BLASTIT_APP` anywhere;
#    (b) same workdir with `version = 1` + a route but no app: exits 1, names `BLASTIT_APP`,
#        and contains no "No such file".
#    No `sleep`, deterministic container name/temp dir, timeout bounded.
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make e2e 2>&1 | tee /tmp/opencode/mb116-e2e.log | tail -30
$ grep -E "rooted|No such file|BLASTIT_APP" /tmp/opencode/mb116-e2e.log | tail -10
# pasted lines show both PASS entries for the rooted boot; make e2e exit 0.

# 2. The existing negative boot is untouched and still passes:
$ grep -n 'check_negative_boot' e2e/rootless/run.py
$ grep -A3 'D6: an unprivileged boot' /tmp/opencode/mb116-e2e.log | tail -4

# 3. The dev assertion is untouched (protected file not in paths):
$ git diff --stat -- crates/blastit/tests/critical_config_and_presets.rs   # empty

# 4. Gates.
$ uv run --no-project e2e/scenario.py --self-test
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node <ID>
$ uv run --no-project tools/plan.py validate
```

# Notes

- Docker is required; if it is unavailable, report `STATUS: blocked` with `docker info` — never stub, skip, or `sleep` the container checks (`make e2e` fails on a skip in this tree).
- Assert the **message**, not only the exit code; and assert the absence of the wrong message, or a swapped-order regression still passes.
- Keep the check in the `--local` driver (the CI rootless job runs it). A droplet-image twin is optional and out of scope.
- `e2e/README.md` / `e2e/rootless/README.md` may need one line for the new check.
