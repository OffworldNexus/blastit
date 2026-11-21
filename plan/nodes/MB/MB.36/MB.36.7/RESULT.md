# RESULT — MB.36.7 (prove MB.36 end to end)

Attempt 1. Every command below ran in `.worktrees/release-0.2.0` (branch `release/0.2.0`), with
`export PATH="$HOME/.local/bin:$HOME/.cargo/bin:$PATH"`.

## 1. What this leaf landed

| Path | What |
|---|---|
| `docs/testing/AUDIT-MB.36.md` | the audit: tier table, every §4.5 source→destination pair with its `--list` count, the moved-case totals, the seven-row replay, the survivors, the gap nodes |
| `crates/blastit/tests/dx_capture.rs` | the capture normaliser + the design's two self-tests + the mask guard + the exit-ordering rule |
| `crates/blastit/tests/dx-mask.txt` | the design's eight masked fields (one file, two readers) |
| `crates/blastit/tests/dx-mask-host.txt` | the host/run additions, with the reason for each |
| `crates/blastit/tests/dx-parity.txt` + `dx-parity.expected` | the one golden vector `dx_capture.rs` and `e2e/dx_replay.py --self-test` both normalise |
| `crates/blastit/tests/dx_invariance.rs` | rows 1 and 3 in process (`row_3` behind `--features python`, the wheel's configuration) |
| `e2e/dx_replay.py` | the seven-row capture driver (stdlib only), reading the same mask file |

No existing test, protected file, or file outside the node's `paths` was modified.

## 2. The wheel

```console
$ scripts/build-wheel.sh --manylinux 2_28
…
wheel: blastit/_blastit.cpython-314-x86_64-linux-gnu.so
ok   …/_blastit0.so: NEEDED libgcc_s.so.1 libpthread.so.0 libm.so.6 libdl.so.2 libc.so.6 ld-linux-x86-64.so.2; no RPATH/RUNPATH
ok   …/_blastit0.so: no BLASTIT_TEST_SWAP_POINT (absent, as shipped)
EXIT:0
$ sha256sum dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
01dce9e178c47eab71a0cd452df830c5c12c6adf1123cc21931a34e00afe0061  dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
```

`make check`'s `docs-test` later builds a host wheel into a scratch directory and restores
`dist/`; §8 records the artifact the tree holds at the end.

### 2.1 Wheel smoke (ABI, `blastit-dbproxy`, one static route, one WSGI route)

```console
$ uv venv --python 3.14 /tmp/…/venv-dx
$ uv pip install --python /tmp/…/venv-dx/bin/python --offline dist/blastit-0.2.0-cp314-cp314-manylinux_2_28_x86_64.whl
 + blastit==0.2.0
$ grep 'Tag:' /tmp/…/venv-dx/lib/python3.14/site-packages/blastit-0.2.0.dist-info/WHEEL
Tag: cp314-cp314-manylinux_2_28_x86_64
$ readelf -d …/site-packages/blastit/_blastit.cpython-314-x86_64-linux-gnu.so | grep NEEDED
  Shared library: [libgcc_s.so.1] … [libc.so.6] [ld-linux-x86-64.so.2]      # no libpython, no libphp
$ …/site-packages/blastit/_bin/blastit-dbproxy --help
blastit-dbproxy: `--help` is not `--key=value`
usage:
  blastit-dbproxy --role=dbproxy --secrets-fd=N --events-fd=N --ready-fd=N [--seccomp=0|1]
$ …/venv-dx/bin/blastit dev --port 18291    # static "/" { root "public" }
HTTP/1.1 200 OK
Content-Type: text/html; charset=utf-8
Content-Length: 37
ETag: "25-18e930e3efac0f85"
Cache-Control: no-cache
<!doctype html><h1>wheel-static</h1>
$ …/venv-dx/bin/blastit dev --port 18292    # entrypoint python "app:application"
HTTP/1.1 200 OK
Content-Type: text/plain
chunked: a wheel-wsgi
```

The wheel's own banner for the WSGI route names its interpreter, so the route really ran in the
wheel's CPython: `<ts> INFO  runtime     python 3.14.10  environment /tmp/…/venv-dx`.

## 3. The two e2e legs, on the refactored tree

### 3.1 Docker (`E2E_DOCKERFILE=Dockerfile.prebuilt`, the wheel from §2)

```console
$ E2E_DOCKERFILE=Dockerfile.prebuilt uv run --no-project e2e/run.py
…
PASS: the php2shell matrix and docs/frameworks/php/index.md replayed against a real blastit dev (140 checks)
PASS: the matrix this run replayed is the 24-reachable-row one (25 rows)
PASS: `/assets/app.4f1c2a9b7d3e.js` answers `Cache-Control: public, max-age=31536000, immutable`
PASS: `uvx blastit check` exits 0 on an all-static manifest
…
  ok   the row the write route inserted is the one the read route reads ([{'body': 'sqlalchemy', 'id': 1}])
  ok   a statement no route grants is refused (500)
  ok   and nothing was deleted ([{'body': 'sqlalchemy', 'id': 1}, {'body': 'sqlalchemy', 'id': 2}])
  ok   exactly the statement no route grants was denied (…'sqlstate': '42501'…)
PASS: 0 failure(s)                              # 260 PASS lines, 0 failures
docs_deploy: 46/46 documented pooler promises hold
EXIT:0
```

### 3.2 Rootless

```console
$ uv run --no-project e2e/rootless/run.py --local --dockerfile Dockerfile.prebuilt
…
PASS: 0 failure(s)                              # helpers, then the app leg
  ok   one stage 0 boot event (1)
  ok   the stage 0 boot event says `privilege: capabilities` (capabilities)
  ok   and names exactly the capabilities it holds (['cap_chown', 'cap_dac_override', 'cap_fowner',
       'cap_kill', 'cap_setgid', 'cap_setuid', 'cap_setpcap'])
  ok   a restart serves the page as it is now
  ok   and the restarted boot serves it (200)
PASS: 0 failure(s)
EXIT:0
```

### 3.3 The standalone archive boots PHP with no interpreter

Not the ELF check and not the Docker leg: the **archive**, built, unpacked, and executed.

```console
$ scripts/build-standalone.sh --out /tmp/…/standalone
build-standalone: cargo build --release --no-default-features --features php,static
build-standalone: 0 pyo3 dependencies (no libpython linked)
build-standalone: blastit-view-static present (a static mount is served)
build-standalone: 1 shim(s) for x86_64-gnu from …/target/php-sapi
build-standalone: 5249444 bytes -> …/blastit-x86_64-unknown-linux-gnu.tar.gz
$ sha256sum …/blastit-x86_64-unknown-linux-gnu.tar.gz
496b5360c708daf3d370e342e6c5b73497fb13b0aec532d06985b15989a5e607  blastit-x86_64-unknown-linux-gnu.tar.gz
$ readelf -d blastit | grep NEEDED
  Shared library: [libgcc_s.so.1] [libm.so.6] [libc.so.6] [ld-linux-x86-64.so.2]   # no libpython, no libphp
$ sha256sum -c SHA256SUMS
LICENSE: OK  README.md: OK  blastit: OK  libblastit_php84-x86_64-gnu.so: OK
```

`debian:trixie-slim` (no CPython installed at all), the archive's own runtime check:

```console
$ docker run --rm --network none -e PATH=/usr/sbin:/usr/bin:/sbin:/bin \
    -v …/blastit-x86_64-unknown-linux-gnu:/a:ro -v …/scripts:/s:ro \
    debian:trixie-slim bash /s/standalone-runtime-check.sh --require-no-python /a/blastit
host: no CPython on PATH
ok   200  /                      Cache-Control: no-cache
ok   404  /mb17-does-not-exist
ok   2/2 static responses without touching an interpreter
ok   exit 1 for a WSGI manifest (capability gate, names 'uv add blastit')
standalone-runtime-check: 3/3 passed without touching an interpreter
```

And the PHP half — the same unpacked archive, `[route.php]`, `/usr/local/bin` (that image's CPython)
off `PATH`. The command exactly as run, mounts and both `-e` flags inline:

```console
$ docker run --rm --network none \
    -e PATH=/usr/sbin:/usr/bin:/sbin:/bin \
    -e BLASTIT_LIBPHP=/usr/lib/libphp8.4.so \
    -e BLASTIT_PHP_SHIM_DIR=/a \
    -e BLASTIT_MEM_HIGH_WATERMARK=100 \
    -v /tmp/…/blastit-x86_64-unknown-linux-gnu:/a:ro \
    -v /tmp/…/phpsite:/site:ro \
    --workdir /site blastit-e2e-php:latest \
    bash -c 'set -e; for c in python python3 python3.13 python3.14; do command -v "$c" >/dev/null && { echo "UNEXPECTED $c"; exit 1; }; done; echo "host: no CPython on PATH"; /a/blastit dev --port 18500 > /tmp/dev.log 2>&1 & pid=$!; trap "kill -TERM $pid 2>/dev/null || true" EXIT; for i in $(seq 1 300); do (exec 3<>/dev/tcp/127.0.0.1/18500) 2>/dev/null && { exec 3<&-; break; }; sleep 0.1; done; exec 4<>/dev/tcp/127.0.0.1/18500; printf "GET /hello HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n" >&4; cat <&4; echo "---- banner ----"; head -6 /tmp/dev.log'
host: no CPython on PATH
HTTP/1.1 200 OK
Content-type: text/plain;charset=UTF-8
Transfer-Encoding: chunked
Connection: close

1d
php from the archive: /hello

0

---- banner ----
  blastit dev · single-uid simulation · 127.0.0.1:18500
  Not a sandbox. Production `blastit` refuses to run like this.
  http · scheduler · dbproxy · 1 route
17:43:58.627 INFO  boot        stage0  seccomp=on  privilege=unprivileged-dev
17:43:58.642 INFO  runtime     php 8.4.27  libphp /usr/lib/libphp8.4.so  php.ini /etc/php/8.4/embed/php.ini
17:43:58.643 INFO  ready       serving  routes=1  workers=16
```

View `entrypoint php "public/index.php"` with `body form`; the flags above are the ones that
make the host library and the archive's shim discoverable, and the 200 is the PHP body.
The same transcript is `AUDIT-MB.36.md` §6.3.

## 4. The DX-invariance replay

```console
$ uv run --no-project e2e/dx_replay.py run --binary …/base-python --out …/final-base --rows 1,2,3,4,5,6,7 …
$ uv run --no-project e2e/dx_replay.py run --binary …/head-python --out …/final-head --rows 1,2,3,4,5,6,7 …
$ for f in …/final-base/*.normalised; do cmp -s "$f" "…/final-head/$(basename "$f")" || echo "DIFFERS: $f"; done
$ echo "captures compared: $(ls …/final-base/*.normalised | wc -l), differing: 0"
captures compared: 24, differing: 0
```

The baseline is `4aa39d9` (the epic's parent) from `git archive`, never this worktree: `develop`
is **not** this branch's ancestor (`merge-base` `2c05139`, 51 commits the other way), so a develop
capture could never be empty. The deviation is recorded in `AUDIT-MB.36.md` §3.1 with the
evidence. The control that the empty diff is not flake — head twice, compared with itself — is
also in §3, along with the three normalisation rules that diff forced (`mem_used`,
`max_view_processes`, `immortalized`, the concurrent-exit block, the pytest seconds), each with
its reason.

## 5. The docked tests and the normaliser

```console
$ cargo test -p blastit --test dx_capture --locked
test result: ok. 5 passed; 0 failed            # incl. the parity vector `dx_replay.py` also asserts
$ uv run --no-project e2e/dx_replay.py self-test
dx-replay self-test: parity with dx_capture.rs on dx-parity.txt, empty refused
$ cargo test -p blastit --test dx_invariance --locked
test result: ok. 1 passed; 0 failed            # row 1, the default-features artifact
$ cargo test -p blastit --test dx_invariance --locked --features python
test result: ok. 2 passed; 0 failed            # rows 1 and 3
```

The normaliser is proven able to fail: `ts` was deleted from `dx-mask.txt`, two self-tests went
red (`a_varying_ts_is_masked_while_a_stable_status_is_kept`,
`the_mask_is_the_designs_eight_fields_and_nothing_else`), and the file was restored
(`AUDIT-MB.36.md` §3.3).

## 6. The protected tier

```console
$ make critical-guard NODE=MB.36.7
critical-guard: HEAD..working tree, 5 changed file(s)
  no change to a protected critical test
$ make rs-test-critical
… every crates/*/tests/critical_*.rs suite green, 51 files
$ cargo test -p blastit-view-static --locked --test critical_static_assets -- --list | grep -c ': test$'
20   # floor 14, previous revision 20
$ cargo test -p blastit-view-python --locked --test critical_loaded_files -- --list | grep -c ': test$'
2    # floor 2
$ for t in critical_static_cli critical_php_backend critical_php_embed critical_parallel_views critical_dev_reload; do
    cargo test -p blastit --locked --features python --test $t -- --list | grep -c ': test$'; done
12  9  28  7  23                               # floor 61 in total, 79 listed (+9 helper, +9 reload growth)
```

`AUDIT-MB.36.md` §2 has the full table, the rename-detected line budgets and the source-absent
checks.

## 7. Mutation campaigns

One at a time, each `mutants.out` saved to `/tmp/opencode/mb367/mutants/<crate>/` before the next
started; ratios recomputed from `mutants.out/{caught,missed,timeout,unviable}.txt` (27.1.0 writes
no `outcomes.txt`). `make mutants-clean` ran at the end, and `git status --short` names no mutated
source.

```console
$ make mutants-rust PKG=blastit-view
232 mutants tested in 5m: 26 missed, 191 caught, 10 unviable, 5 timeouts
$ make mutants-rust PKG=blastit-view-static
274 mutants tested in 4m: 37 missed, 218 caught, 18 unviable, 1 timeouts
$ make mutants-rust PKG=blastit-view-php
447 mutants tested in 13m: 305 missed, 97 caught, 45 unviable, 0 timeouts
ERROR scenario execution internal error err=interrupted phase=Test
Error: interrupted                                            # twice, at the same mutant
$ make mutants-clean
mutants: clean: freed 27.8 MiB; /home/…/.cache/blastit-mutants now has 478.1 GiB free
```

| Crate | Tested | Caught | Missed | Timeout | Unviable | Killed |
|---|---|---|---|---|---|---|
| `blastit-view` | 222 | 191 | 26 | 5 | 10 | 86.0 % |
| `blastit-view-static` | 256 | 218 | 37 | 1 | 18 | 85.2 % |
| `blastit-view-php` (447/572 measured) | 402 | 97 | 305 | 0 | 45 | 24.1 % |

The third number is a **campaign-scoping** artefact, not a test gap: `discovery.rs` is 21 caught /
267 and its oracle is the root crate's `stage0::php_boot_gate` (27 cases) and
`critical_php_embed` (28) — neither runs under `cargo mutants -p blastit-view-php`. Per file:
`worker.rs` 80.3 % (57/71), `pinning.rs` 55.2 % (16/29), `discovery.rs` 7.9 % (21/267),
`probe.rs` 6.5 % (2/31). The `--test-package` rerun and the 125 mutants left by the deterministic
interrupt (`worker.rs` past `678:20`, next `699:5`) are `MB.26.26`.

The survivors the audit judged **equivalent** (disjoint `|` on bitflags, `supports_openat2 → true`
on a kernel that has it, one `classify` arm whose fallback agrees) are listed with their reason in
`AUDIT-MB.36.md` §4.2; the real ones are `MB.26.25`, `MB.26.28` and `MB.26.29`.

## 8. `make docs-test`, `make check`, plan

```console
$ make docs-check && make docs-test
…
48 fences: 30 executed, 18 accounted, 0 failed · 32.4s
EXIT:0
$ make check
…
plan OK
EXIT:0
$ uv run --no-project tools/plan.py validate
plan OK
```

## 9. Audit verdict

> **The epic's test story holds.** No protected case was lost, weakened, skipped or re-scoped;
> the relocated suites are the same files with their imports repointed (≤ 4 changed lines each)
> and their `--list` counts at or above the floor; the seven documented behaviours replay
> byte-identically after a mask that is itself tested and was proved able to fail; and the wheel,
> Docker and rootless artifacts are green on the refactored tree.

## Follow-ups

Created as `status: proposed` tasks, listed in `AUDIT-MB.36.md` §7:

- `MB.26.25` — a protected `blastit-view` transport suite (`percent_decode`, `Credit`, `max_payload`);
- `MB.26.26` — run the view-crate campaigns against the tests that cover them (`--test-package`)
  and finish `blastit-view-php`'s 125 interrupted mutants;
- `MB.26.27` — the `blastit-view-static` behavioural survivors (`declares_body`, pre-1970 dates,
  `trim_ows`);
- `MB.26.28` — the twelve `blastit-view/src/loaded.rs` survivors, row by row;
- `MB.26.29` — the `\| → &` open-flag cells that empty a set with no following check
  (`O_DIRECTORY` at the root, `O_NONBLOCK` against a FIFO swap, `CLOEXEC` across `execve`);
- `MB.36.11` — the wheel's `DT_NEEDED` set (R12) and the deleted trait names (R3) as tests.

### What happened to `MB.36.7.1`–`.4` (the journal lines at 18:05)

Those four were created **under this leaf**, and `plan.py validate` then refused the tree
("`MB.36.7: task has children; convert it with 'edit MB.36.7 --kind epic' first"): converting a
node to an epic is not a test-curator's call, so I removed the four directories I had just created
with `rm -rf plan/nodes/MB/MB.36/MB.36.7/MB.36.7.{1,2,3,4}` (no other file touched) and
re-created the same four bodies under the epic as `MB.36.8`–`MB.36.11` with
`plan.py --agent test-curator new MB.36 …` — each in the same minute, which is why the journal
shows both sets at 18:05. The lead has since re-filed `MB.36.8`/`.9`/`.10` as
`MB.26.25`/`.26`/`.27` and marked the originals `superseded`; `MB.36.11` stays in the epic. The
journal line naming nodes that do not exist is a defect of running `new` before `validate`; no
`plan.py` command deleted them (there is none that would).

`README.md` was left exactly as it was found: it is dirty by design, outside this node's `paths`,
and what a commit contains is the lead's call. This leaf ran no `git add`, `git commit`,
`git checkout` or `git restore`.
