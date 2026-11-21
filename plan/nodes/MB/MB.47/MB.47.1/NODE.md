---
id: MB.47.1
title: "Rootless Docker e2e blastit-init alignment, venv-smoke py3-none glob & root-safe critical tests"
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: ["e2e/**", crates/blastit/tests/critical_handoff.rs, crates/blastit/tests/critical_python_shim.rs]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-20T07:50"
updated: "2026-11-20T08:15"
---

# Goal

Make the rootless Docker e2e, the venv smoke glob and the two root-sensitive critical tests match the shipped `ADR-0017` `blastit-init` layout, so they pass on a real host and as `uid 0` in a container. Three concrete fixes, no production code:

1. **`e2e/rootless/{Dockerfile,Dockerfile.prebuilt,compose.yml,run.py,README.md}` — stop `setcap`-ing a copy of `python3`.**
   Today the image copies the shared interpreter to `python3.X-init` and starts `python3.X-init -m blastit`. Under `ADR-0017` that boot `execve`s `.../site-packages/blastit/_bin/blastit`, and the `execve` drops every file capability — so the e2e proves the wrong thing. Mirror `docs/deploy/compose-traefik.md` lines 129–160 instead:

   ```dockerfile
   RUN lib="$(python3 -c 'import blastit, pathlib; print(pathlib.Path(blastit.__file__).parent / "_bin")')" \
       && cp "$lib/blastit" "$lib/blastit-init" \
       && chown "$BLASTIT_UID:$BLASTIT_UID" "$lib/blastit-init" \
       && chmod 0500 "$lib/blastit-init" \
       && setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep' \
            "$lib/blastit-init"
   ```

   - `blastit-init` sits beside the shims and `blastit-dbproxy` in `_bin/`, i.e. the directory the installed package resolves to; **the basename `blastit-init` is not a name the shim/self-handoff logic re-execs** — verify that (a `blastit-init` that still `execve`d `_bin/blastit` would silently drop the capabilities again; if the resolver keys off the file name, the check is that boot refuses/handles it and the driver's `getcap` assertion below still holds).
   - `chown` **before** `chmod`/`setcap`: `chown` clears a file capability, so nothing re-owns the file afterwards.
   - Apply the same block to **both** Dockerfiles; update the header comments that still explain "a dedicated copy of the interpreter" to describe a dedicated copy of the **init binary** (`_bin/blastit`), and drop the CPython-prefix rationale that no longer applies.
   - Replace every start of the old copy with `blastit-init` directly — no `-m blastit`: the `CMD` in both Dockerfiles **and** the `app` service `command:` in `compose.yml` (`command: ["sh", "-c", 'exec "<…>/_bin/blastit-init"']`, resolved from the installed package path, not from `command -v python3`).
   - `run.py` checks:
     - the `stat`/mode assertions inspect `_bin/blastit-init` (`0500`, owned by `BLASTIT_UID`);
     - the `getcap` lines (currently around lines 534–547, both the in-container and `compose exec` variants) print `getcap` of `_bin/blastit-init`, `_bin/blastit` **and** `python3`, and assert the first is non-empty while **both** `_bin/blastit` and `python3` carry **zero** `cap_` entries;
     - the worker/migrations uid-refusal check targets `_bin/blastit-init` (`EACCES`), not `python3.X-init`;
     - the comment block near line 71 ("the image's `setcap` line, the compose `cap_add` list and this list are one set") is updated to the binary.
   - `README.md`: the capability paragraph explains the capable copy of `_bin/blastit`, still one set with the compose `cap_add` list, still a dedicated uid.

2. **`e2e/venv-smoke.sh` wheel glob.** Line 29's `blastit-*cp314*.whl` is stale (the wheel is now `py3-none`). Use `blastit-*-py3-none-*.whl` with a `blastit-*.whl` fallback, and fix the header comment at line 11 that still describes a `cp314-cp314-manylinux` wheel.

3. **Root/container-safe critical tests (authorized: only these two files).**
   - `crates/blastit/tests/critical_handoff.rs`, `a_candidate_that_is_this_image_does_not_hand_off`: copy `CARGO_BIN_EXE_blastit` to `fixture/global/blastit` **first**, then `fs::hard_link("fixture/global/blastit", "fixture/project/.venv/bin/blastit")`, and exec `global/blastit`. Under `sudo` (`$HOME=/root`) `target/` and the fixture live on different filesystems, so hard-linking the cargo artifact directly fails with `EXDEV (os error 18)`.
   - `crates/blastit/tests/critical_python_shim.rs`, `a_php_only_boot_never_opens_libpython`: only expect `200 OK` when **both** `php_shim()` **and** a usable host `libphp` are present; otherwise assert the PHP-terms refusal. A runner with `target/php-sapi` mounted/cached but no `libphp-embed` must not panic.

# Context

- Reference layout: `docs/deploy/compose-traefik.md` § "4. The image" (lines 129–160) — `lib/blastit` → `lib/blastit-init`, `chown`, `chmod 0500`, `setcap ...+ep`, started directly.
- Host requirement: Docker Compose on this machine; **never touch the foreign container `n2f-vuln`**; ports `28080`/`28443` are required by the acceptance run.
- `tools/critical_guard.py` protects these two files; this node's `paths` authorize the edits above and nothing else.

# Acceptance

```bash
# 1. rootless e2e, prebuilt wheel path, 100%
uv run --no-project e2e/rootless/run.py --local \
    --dockerfile Dockerfile.prebuilt --port 28080 --tls-port 28443

# 2. both critical binaries as uid 0
cargo test -p blastit --test critical_handoff --test critical_python_shim
# then the same binaries under:
docker run --rm --security-opt seccomp=unconfined \
    -v "$PWD/target:/target:ro" -v "$PWD:/src:ro" debian:trixie-slim \
    /target/debug/deps/critical_handoff-* /target/debug/deps/critical_python_shim-*

# 3. gates
make fast-check
uv run --no-project tools/critical_guard.py --base origin/develop --node MB.47.1
```

# Notes

- Reserve `MB.47.2` for `Makefile`/`.github/workflows/**`/`scripts/**` — **do not** edit those here, even to fix a CI comment you notice. Record such finds under `## Follow-ups` in `RESULT.md`.
- Keep `RESULT.md` short: command + output tail per acceptance item.
