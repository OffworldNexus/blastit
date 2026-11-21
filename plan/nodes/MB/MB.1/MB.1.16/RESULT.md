# RESULT MB.1.16 — Pin the rooted production missing-manifest message in the Docker e2e

## Summary

`docs/testing/AUDIT-MB1.md` `C5`/`G5`: `MB.1.6` reordered the missing-`./blastit.toml` message before
the `BLASTIT_APP` message in **both** entry points, but only `stage0::run_dev` was pinned — by the
protected `critical_config_and_presets.rs`, because an unprivileged production boot is refused by the
capability gate before the manifest is read. The production half (`stage0::run`) rested on one manual
`sudo -n` run (`MB.1.6` RESULT leg 3). It is now automated, against the **same prebuilt image** the D6
refusal uses.

`e2e/rootless/run.py` gains `check_rooted_startup(image)` next to `check_negative_boot`, called from
`local()` right after it (the `--local` driver is the CI path; the droplet twin stays out of scope).
It boots the image as `--user 0:0` in a bind-mounted workdir, both directions:

* **(a) empty workdir** — exits `1`, and **stdout's `app.output` events** name
  `./blastit.toml: No such file or directory`, with `BLASTIT_APP` nowhere in stdout *or* stderr;
* **(b) the same workdir with** `version = 1` + one route and no `app` — exits `1`, names
  `BLASTIT_APP`, and nothing claims the file is missing.

Three properties make it an oracle rather than a smoke test:

1. **The stream is the assertion.** `app_output_lines()` parses only `completed.stdout`, so a message
   that regressed to fd 2 leaves the list empty and the check fails (the protected dev test's
   `stdout.is_empty()` is the mirror image). The run prints `stderr carries N bytes` next to it.
2. **Each direction asserts the absence of the other message**, so a swap back (`app` read first)
   cannot pass on the exit code alone — the exit code is `1` either way.
3. **The image's own environment is cleared** (`unset BLASTIT_APP BLASTIT_MANIFEST` in `sh -c`, not
   `-e NAME=`): `Dockerfile.prebuilt` sets `BLASTIT_APP=demo.wsgi:application` and
   `BLASTIT_MANIFEST=/app/blastit.toml`, so a bare `docker run` never consults the working directory.
   `-e NAME=` is not an `unset` — an empty `BLASTIT_APP` is still a set variable (it wins over the
   manifest, `critical_config_and_presets.rs`'s empty-variable test), and an empty `BLASTIT_MANIFEST`
   is refused as a mistake of its own (`config.rs::manifest_from_env`) — so neither would be the boot
   this check is about.

Determinism: two fixed container names (`blastit-d6-rooted-boot-missing` / `-no-app`, each
`docker rm -f`'d first, `--rm`'d after), a `mkdtemp` workdir with a fixed prefix removed in a
`finally`, `timeout=180` per boot, no `sleep`, no network. `check_negative_boot` and its D6 output are
untouched.

## Evidence

### 1. `make e2e` green, both new PASS blocks present

```console
$ CARGO_NET_OFFLINE=true UV_OFFLINE=1 make e2e > /tmp/opencode/mb116-e2e.log 2>&1; echo "make e2e rc=$?"
make e2e rc=0

$ tail -20 /tmp/opencode/mb116-e2e.log
  ok   uid 21007 (pid 55) holds nothing and has no_new_privs (inh=0000000000000000 prm=0000000000000000 eff=0000000000000000 bnd=0000000000000000 amb=0000000000000000 nnp=1)
  ok   http runs under uid 20001 (D6 keeps the layout)
  ok   scheduler runs under uid 20002 (D6 keeps the layout)
  ok   dbproxy runs under uid 20003 (D6 keeps the layout)
  ok   a plain python3, as the worker's uid (10001) (uid 10001) holds nothing and cannot become root (inh=0000000000000000 prm=0000000000000000 eff=0000000000000000 amb=0000000000000000 nnp=0 setuid=refused)
  ok   a manage.py, as the worker's uid (10001) (uid 10001) holds nothing and cannot become root (inh=0000000000000000 prm=0000000000000000 eff=0000000000000000 amb=0000000000000000 nnp=0 setuid=refused)
  ok   the capable copy is mode 0500 and owned by init's uid (mode=500 uid=10000)
  ok   init's own uid can run it — which is why nothing else may use that uid (Python 3.14.10)
  ok   the worker's uid cannot run it (sh: 1: /usr/local/bin/python3.14-init: Permission denied)
D6: an unprivileged boot with no capabilities is refused
  ok   the boot exits 1 (1)
  ok   and the message names the uid and every capability it lacks (init runs as uid 10000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid)
D6: the rooted production boot names the missing manifest first
  ok   the rooted boot exits 1 (1)
  ok   and stdout's `app.output` names the file and the reason (./blastit.toml: No such file or directory); stderr carries 0 bytes
  ok   and `BLASTIT_APP` is not blamed for the missing file ([])
D6: the rooted production boot with a manifest names BLASTIT_APP, not the file
  ok   the rooted boot exits 1 (1)
  ok   and stdout's `app.output` names `BLASTIT_APP` (['BLASTIT_APP: the WSGI callable (`module:attribute`) is required']); stderr carries 0 bytes
  ok   and nothing claims the manifest is missing ([])
```

```console
$ grep -E "rooted|No such file|BLASTIT_APP" /tmp/opencode/mb116-e2e.log | tail -10
D6: the rooted production boot names the missing manifest first
  ok   the rooted boot exits 1 (1)
  ok   and stdout's `app.output` names the file and the reason (./blastit.toml: No such file or directory); stderr carries 0 bytes
  ok   and `BLASTIT_APP` is not blamed for the missing file ([])
D6: the rooted production boot with a manifest names BLASTIT_APP, not the file
  ok   the rooted boot exits 1 (1)
  ok   and stdout's `app.output` names `BLASTIT_APP` (['BLASTIT_APP: the WSGI callable (`module:attribute`) is required']); stderr carries 0 bytes

$ grep -c '^  FAIL' /tmp/opencode/mb116-e2e.log        # 0
$ grep -c '^  ok'   /tmp/opencode/mb116-e2e.log        # 356
$ wc -l             /tmp/opencode/mb116-e2e.log        # 620
$ ls dist/                                             # exactly one wheel, as `make e2e` requires
blastit-0.1.2-cp314-cp314-manylinux_2_39_x86_64.whl
```

### 2. The existing negative boot is untouched and still passes

```console
$ grep -n 'check_negative_boot' e2e/rootless/run.py
574:def check_negative_boot(image, uid=APP_UID):
774:        rc |= check_negative_boot(env["E2E_IMAGE"])

$ grep -A3 'D6: an unprivileged boot' /tmp/opencode/mb116-e2e.log | tail -4
D6: an unprivileged boot with no capabilities is refused
  ok   the boot exits 1 (1)
  ok   and the message names the uid and every capability it lacks (init runs as uid 10000 and is missing cap_chown, cap_dac_override, cap_fowner, cap_kill, cap_setgid, cap_setuid)
D6: the rooted production boot names the missing manifest first
```

(Definition and call only; `MISSING_CAPABILITIES`, the `--cap-drop ALL`/shared-interpreter logic and
its output are byte-identical — `git diff` has no `-` lines.)

### 3. The dev assertion is untouched

```console
$ git diff --stat -- crates/blastit/tests/critical_config_and_presets.rs
$                                                  # empty: the protected file is not in this node's diff
$ git diff --stat -- e2e/
 e2e/rootless/README.md |   8 ++++
 e2e/rootless/run.py    | 124 +++++++++++++++++++++++++++++++++++++++++++++++++
 2 files changed, 132 insertions(+)
```

### 4. Gates

```console
$ uv run --no-project e2e/scenario.py --self-test 2>&1 | tail -2
  ok   and it is not a pass
PASS: 0 failure(s)

$ uv run --no-project tools/plan.py validate
plan OK
```

`critical_guard.py` on this node's own paths: **MB.1.16 adds no protected change**. The single-node
run is red because the range also carries the siblings' commits, and it names them:

```console
$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.16
critical-guard: 51dced9..working tree, 60 changed file(s)
node MB.1.16: paths ['e2e/rootless/run.py', 'e2e/rootless/README.md', 'e2e/README.md']
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [VIOLATION: not covered by any node path]
  A  crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs  [added (always allowed)]
  A  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs  [added (always allowed)]
  M  crates/blastit/tests/critical_check_engine.rs  [VIOLATION: not covered by any node path]
critical-guard: FAILED: 2 protected critical test(s) changed or deleted without a covering path in node(s) MB.1.16. …

$ uv run --no-project tools/critical_guard.py --base 51dced9 --node MB.1.16 --node MB.1.15 --node MB.1.13
…
  M  crates/blastit-manifest/tests/critical_manifest_edit.rs  [authorized by crates/blastit-manifest/tests/critical_manifest_edit.rs]
  A  crates/blastit-sqlpolicy/tests/critical_lexer_boundaries.rs  [added (always allowed)]
  A  crates/blastit-sqlpolicy/tests/critical_policy_surface.rs  [added (always allowed)]
  M  crates/blastit/tests/critical_check_engine.rs  [authorized by crates/blastit/tests/critical_check_engine.rs]
critical-guard: OK: all 4 protected change(s) are authorized by node(s) MB.1.16, MB.1.15, MB.1.13
```

Both violations are sibling work in the `51dced9..HEAD` range: `critical_manifest_edit.rs` is in
MB.1.15's `paths` (committed as `c65289b` while this node ran; it was an uncommitted worktree edit at
the time of the run above, which is why the guard lists it as `M`), and `critical_check_engine.rs` is
MB.1.13's. Neither file appears in this node's diff, and re-running the guard at `c65289b` gives the
same two names.

## How the assertion was proved able to fail

Two mutations, each applied to `e2e/rootless/run.py`, run, then reverted (`git diff` afterwards is the
same `+124` and no `-`):

**(i) the wrong stream** — `app_output_lines(completed.stdout)` → `app_output_lines(completed.stderr)`
in both directions:

```console
D6: the rooted production boot names the missing manifest first
  ok   the rooted boot exits 1 (1)
  FAIL and stdout's `app.output` names the file and the reason (./blastit.toml: No such file or directory); stderr carries 0 bytes
  info the boot said: {…"event":"app.output",…,"line":"./blastit.toml: No such file or directory (os error 2)"…}
  ok   and `BLASTIT_APP` is not blamed for the missing file ([])
D6: the rooted production boot with a manifest names BLASTIT_APP, not the file
  ok   the rooted boot exits 1 (1)
  FAIL and stdout's `app.output` names `BLASTIT_APP` ([]); stderr carries 0 bytes
rc=1
```

Only the message half fails — the `ok   the boot exits 1 (1)` line stays green in both directions,
which is exactly why the exit code alone is not the oracle.

**(ii) the swapped message** — leg (a)'s `MISSING_MANIFEST in line` → `"BLASTIT_APP" in line`
(i.e. what a swap back of the two checks would look like):

```console
D6: the rooted production boot names the missing manifest first
  ok   the rooted boot exits 1 (1)
  FAIL and stdout's `app.output` names the file and the reason (./blastit.toml: No such file or directory); stderr carries 0 bytes
  ok   and `BLASTIT_APP` is not blamed for the missing file ([])
D6: the rooted production boot with a manifest names BLASTIT_APP, not the file
  ok   the rooted boot exits 1 (1)
  ok   and stdout's `app.output` names `BLASTIT_APP` (['BLASTIT_APP: the WSGI callable (`module:attribute`) is required']); stderr carries 0 bytes
  ok   and nothing claims the manifest is missing ([])
rc=1
```

Leg (b) stays green — the negative direction is independent, as it must be.

## Files

- `e2e/rootless/run.py` (+124, 0 deletions): `MISSING_MANIFEST`, `MANIFEST_WITHOUT_APP`, `ROOTED_BOOT`,
  `app_output_lines()`, `rooted_boot()`, `check_rooted_startup()`; one added call in `local()`.
- `e2e/rootless/README.md` (+8): one bullet in the assertion list — the ordering, why it is rooted, and
  where the `dev` half lives.
- `/tmp/opencode/mb116-e2e.log` — the full `make e2e` log quoted above.

`e2e/README.md` needed no line: it documents the app e2e and the three stdlib drivers, and the D6
assertions are listed in `e2e/rootless/README.md`.

## Follow-ups

- **`LESSONS.md` had no line added** (`docs/**` is outside this node's `paths`), though two facts
  belong there. (1) In this tree the editor tool runs `ruff format` with **default** settings on write
  (there is no `[tool.ruff]` in `pyproject.toml`, and `make py-lint` covers only `python tools`), so
  editing `e2e/rootless/run.py` silently reformats the whole file to 88 columns — `+703/-173` instead
  of `+124/-0`. Writing the block through a shell heredoc keeps the diff minimal. (2) `docker run` on
  an image whose `ENV` names `BLASTIT_APP`/`BLASTIT_MANIFEST` inherits them even with `--workdir`
  changed, and `-e NAME=` is *not* an unset: an empty `BLASTIT_APP` is still a set variable, and an
  empty `BLASTIT_MANIFEST` is refused as a mistake of its own (`config.rs`'s `manifest_from_env`). Only
  `unset` in the entrypoint gives a boot that reads `./blastit.toml`.
- **The droplet route does not run the refusal checks** — neither `check_negative_boot` (accepted in
  AUDIT-MB1) nor this one (explicitly out of scope). A droplet twin would need the image on the
  droplet, not on this host.
- **`check_rooted_startup` inherits the driver's one-image assumption**: `E2E_IMAGE` must be the
  prebuilt rootless image. A run with `--dockerfile Dockerfile` (in-image wheel) would work too, but no
  run in `make e2e` exercises that path for this check.
- `critical-guard` at the gate must pass `--node` for **every** leaf that touches a protected file in
  the range (MB.1.13/15/17 as of this tree); MB.1.16's own paths authorize none, correctly.
