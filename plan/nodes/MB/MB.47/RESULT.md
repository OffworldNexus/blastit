# RESULT — MB.47 (epic gate)

Two sequential slices, both reviewed by `review-arch` + `review-ux` and fixed to
zero genuine `[BLOCKER]`/`[MAJOR]` findings:

| Slice | Commit | What |
|---|---|---|
| `MB.47.1` | `2c31404` | Rootless e2e → `_bin/blastit-init`; `venv-smoke` `py3-none` glob; root-safe critical tests |
| `MB.47.2` | `0d3f434` | `ci.yml`/`live.yml` shims + root provenance; `--require-shims py` everywhere; CPython shims in the `.tar.gz` |

## Acceptance

| Check | Result |
|---|---|
| `e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt --port 28080 --tls-port 28443` | **rc=0, 249 ok, 0 FAIL** |
| `critical_handoff` + `critical_python_shim` as uid 0 in `debian:trixie-slim` | **15 passed / 5 passed** |
| `make fast-check && make workflow-check && make packaging-gates-test && platform_matrix.py --check` | **all green** (workflow-check 713 checks/0 problems; packaging-gates 128 cases; matrix ok) |
| `critical_guard --node MB.47` | **OK — all 2 protected changes authorized** |

Epic gate, run once:

```
$ make check
... zed-check: OK (59 properties checked)
Ran 44 tests ... OK
plan OK
```

`e2e/` changed, so both e2e halves were run. `make e2e` cannot run verbatim on
this host — its defaults are `18080/18081/18443` and the foreign `n2f-vuln`
container holds `18081` and `18443` (never touched); the same two runs with
`E2E_PORT`/`E2E_SQLALCHEMY_PORT`/`--port`/`--tls-port` moved to `28080+`:

```
$ E2E_PORT=28080 E2E_SQLALCHEMY_PORT=28081 E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
PASS: 0 failure(s)      (rc=0; docs_deploy 46/46)
$ uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt --port 28080 --tls-port 28443
  (rc=0; 249 ok, 0 FAIL)
```

## Notes

- `critical_guard --base origin/develop` is unusable on this branch: `develop` is
  `ec47752 Release v0.1.2`, 1999 files behind, so it reports other nodes'
  history. The meaningful base is the epic's start, `9066d28` (recorded in the
  slice logs), where both protected edits are authorized by node paths.
- The capability probe had to resolve `command -v python3` before `getcap`:
  `getcap` on a symlink prints nothing and exits 0, so the B1 assertion on the
  shared interpreter could never fail. It now also covers `_bin/blastit-dbproxy`
  and every `_bin/libblastit_*.so`, and a `MISSING`/empty line is a FAIL.
- The `.tar.gz` README states the real WSGI contract (host CPython 3.13/3.14 with
  a shared `libpython3.1x.so`, per-family remedies) — the archive removes the
  "copy the shim beside the binary" step, not the interpreter requirement.

## Follow-ups (outside every MB.47 slice's `paths`)

- **`docs/deploy/compose-traefik.md` §4/§5 is broken for a reader who copies it.**
  §4 uses `lib=/usr/share/blastit/lib` but `docs/install.sh` defaults to
  `/usr/local`, and §5 starts `blastit-init` with no `BLASTIT_EXEC`, so
  `Host::placement` refuses a capable `/proc/self/exe` (a child `--role=dbproxy`
  cannot `execve` a capable file) and the boot exits 1. The e2e is right; the doc
  needs a docs node before 0.2.0 ships.
- `crates/blastit/src/host.rs`: `Host::binary()` should name
  `<dir of /proc/self/exe>/blastit` when it exists beside a `*-init`. That would
  delete `BLASTIT_EXEC` (and the `python3 -I -c` resolution) from all three e2e
  start commands.
- `crates/blastit/src/host.rs`: refuse a **stale** capable init — after
  `pip install --force-reinstall blastit` the old `_bin/blastit-init` stays behind
  and silently pins a pre-upgrade binary as init.
- `e2e/run.py:168`, `e2e/Dockerfile`, `e2e/Dockerfile.prebuilt`,
  `e2e/compose.yaml` still state the retired "not abi3, so the image is the
  interpreter the wheel targets" rule for the root (non-rootless) prebuilt image.
- `blastit-scheduler/tests/critical_rewrite.rs` showed one timing flake in an
  intermediate full run (green 3/3 standalone and in the final `make check`).
