# Testability review (pass 2) — `ma-lean-dx` (0.1.1 design + ADR-0001..0006)

- **Lens:** `testability` (fresh-context goldfish critic, pass 2)
- **Documents:** `docs/design/ma-lean-dx.md`, `docs/decisions/ADR-0001..0006`
- **Cross-checked against the live tree:** `Makefile`, `plan/nodes/MA/MA.{1,2,3,5}/NODE.md`,
  `plan/STATE.md`, `.github/workflows/ci.yml`, `python/tests/**`, `python/blastit/**`,
  `crates/*/tests/**`, `pyproject.toml`, every `toml` fence in `README.md`/`docs/**`
- **Date:** 2026-11-12

Verdict: **changes required.** Pass 1's findings are *mostly* resolved honestly — the byte-exact
ingress matrix (§4.2 A), the 12-row `infer_routes` table (§4.2 G.4), the 9-kind interaction table
(§4.2 G.3), the allow/deny test twins (§7.2) and the `DynamicCheck` always-`Allow` rule (§4.2 F) are
real, checkable improvements, and I verified two claims independently: `uvx --offline ruff check
python` really is **72 errors** and `ruff format --check python` really is **12 files**. But three
residual holes are load-bearing, and one pass-1 BLOCKER was closed by *narrowing the oracle* instead
of fixing the artefact. Counts: **3 BLOCKER, 11 MAJOR, 8 MINOR**.

---

## BLOCKER

### T2-B1 — The docs-fence gate was narrowed to exactly the six fences that already pass, so the two documented grants that R24 now rejects are unmeasured — and no node may fix them

> §7.2 (line 576): "`(+)` … all 6 `` ```toml title="blastit.toml" `` doc fences parse (`count >= 6`,
> zero skips)".
> §9.2 T-B1 (line 710): "Scoped docs manifest extraction mechanically to … `count >= 6`, all 6 in
> `README.md`/`docs/**` verified complete, zero skips".

I extracted all 24 `toml` fences and the 11 titled ones from the real tree. Exactly six carry
`title="blastit.toml"`: `README.md:29`, `docs/frameworks/django.md:44`, `docs/frameworks/flask.md:45`,
`docs/getting-started.md:25`, `docs/index.md:20`, `docs/local.md:45`. None of the six contains a
quoted literal, a `…`, an `[route.ingress]` codec other than nothing, or a `raw`/`allow_raw` pair.
So the six-fence gate can only ever assert `version`/`app`/`preload`/`[[route]]`/`[proxy.*]` — it
cannot detect the exact class of drift it was created for.

The two artefacts it excludes are the ones that are now *wrong*:

- `docs/reference/manifest.md:48` — the canonical session-group grant:
  `queries = ["SELECT … FROM django_session WHERE session_key = $?"]`.
- `docs/local.md:114` — "`sql` is the statement as normalised, with quoted literals replaced by
  `…`: **exactly what a grant has to contain**, and safe to paste into a ticket" (followed by a
  paste example at `docs/local.md:122-129`).

R24 (line 127) makes `Manifest::validate` reject any `queries` entry containing `…`
(`ManifestError::RedactedQueryPlaceholder`). So the reference page's group example and the local
page's "paste this" instruction both produce a manifest that **aborts at boot**, and the new gate is
green because both blocks are excluded from it (`docs/frameworks/index.md:8,12,16` are also excluded
by title, and `docs/frameworks/django.md:84-92` is additionally indented inside an admonition, where
a naive fence scanner does not even see it).

**No planned node can repair this.** `plan/nodes/MA/MA.1/NODE.md` owns `README.md`/`docs/**` and is
`status: done`; `MA.5` owns only `docs/{design,decisions}/**`; `MA.2` owns `crates/**`,
`python/**`, `Makefile`; `MA.3` owns `docs/**` but is sequenced *after* MA.2. §2 (line 87) lists
only `ARCHITECTURE.md`, `.github/workflows/ci.yml` and `tools/guard.py` as follow-ups.

**Failure mode.** MA.2 ships a validator that rejects the reference documentation. `make check` is
green (the docs test reads only titled fences; `make docs-check` only checks links). A user follows
`docs/reference/manifest.md:48`, gets
`blastit.toml: <redacted placeholder>`, and the troubleshooting table (`docs/troubleshooting.md:30`)
tells them to "add it to `queries`" — the very thing that fails. Second failure mode: the assertion
lives in a **protected** file (`crates/blastit-manifest/tests/critical_manifest_011.rs`). MA.3 is
*expected* to edit docs and is forbidden by `tools/critical_guard.py` from editing that test, while
MA.2 (which may edit it) will be closed. A legitimate docs improvement at MA.3 turns a protected test
red with no legal remedy inside MA.3's paths.

**Suggest.** (a) Name the two docs edits as a hard prerequisite of MA.2 (reopen a one-line `MA.1`
task or give `MA.3` the fix and make the design's `R24` text say what a redacted denial must be
paired with: "bind server-side, then grant the parameterised form"); (b) move the fence test out of
the protected tier into a **standard** test that reads a checked-in list of `(path, line)` fixtures —
docs and their oracle then live in the same node; (c) add a negative canary fence that must *fail*
(R24 rejection) so "skip what does not parse" cannot pass; (d) assert `== 6` with the list named,
not `>= 6`, so a deleted fence does not shrink coverage silently.

### T2-B2 — No oracle in this epic runs the documented `blastit check` end to end; the mapped critical test can pass with `blastit check` completely broken

> §7.2 row 10 (line 584): "`(+)` `check --json` clean (`exit 0`, 1 JSON doc) even when app/test
> writes junk to `print()` and `os.write(1, b"noise")`".
> §7.1 (line 568): "deterministic stub `pytest` package fixtures allow multi-process `blastit dev`
> and `blastit check` critical suites to run … completely unprivileged and offline".
> §2 (line 87): "`.github/workflows/ci.yml` (`blastit check --json` CI step) … tracked as
> post-`MA.2` follow-ups".

Two things are true at once, and together they remove the green-path oracle:

1. **A stub `pytest` cannot produce the observation set that the positive case requires.** Exit `0`
   requires zero `error` findings (R4), and R37 makes `no_observations` fire when
   `routes.exercised == 0` or `statements == 0`. To reach `exit 0` the suite must issue a real HTTP
   request through `BlastitWsgiGate` *and* (if any proxy is declared) a real routed SQL statement
   through `dbproxy` in `DynamicCheck`. A stub `pytest` that "exits 0 without running anything"
   yields `no_observations` → exit `1`, not `0`. So the positive case is either unimplementable with
   a stub, or the stub grows into a mini-runner that imports the app — at which point the test no
   longer says anything about real pytest.
2. **A stub `pytest` also bypasses `-p blastit._check_pytest`.** The stub accepts its argv and does
   nothing, so the single most likely integration failure — the plugin is not on `sys.path`, the
   `conftest`/entry point is not loaded, the gate never wraps the app — is exactly what no offline
   test exercises. Under the stub, `requests: 0` is "correct".

And the one place a real run *would* happen is unowned. `MA.3`'s paths are `README.md`,
`CHANGELOG.md`, `ARCHITECTURE.md`, `Cargo.toml`, `Cargo.lock`, `docs/**`, `python/tests/**`,
`e2e/**` — **no `.github/**`** — so the deferred "`blastit check --json` CI step" cannot be added by
any node in the MA milestone. (The existing `pytest plugin (CPython 3.14, PostgreSQL)` job runs
`pytest python/tests -q` against a built wheel, so a real oracle *could* live at
`python/tests/test_check.py`, which is in Leaf 9's paths — but §7.2 maps R3..R5 only to the stub
file, and nothing in the design says `test_check.py` must drive the CLI end to end.)

**Suggest.** Explicitly split the oracle in §7.2: (a) `python/tests/critical/test_check_contract.py`
= fd-1 purity + exit-code mapping against the stub (cheap, offline, states its own limits);
(b) `python/tests/test_check.py` = **real** `pytest`, real PostgreSQL, one Django and one raw-WSGI
app, asserting `requests > 0`, `findings == []`, `exit 0`, and `-p blastit._check_pytest` actually
loaded (e.g. the app asserts a header the gate injected, or the run prints a plugin-version line).
Add "and one green-path real run" to MA.2's acceptance, and either give `MA.3` `.github/**` or drop
the deferred-CI-step claim in §2 and say the step lands in a node that owns it.

### T2-B3 — `plan_learn`'s proposal assembly is unspecified: nothing says how SQL observed on an unrouted request reaches the `[[route]]` that `--learn` is supposed to write, so R27 has no feasible oracle

> R27 (line 130): "`--learn` writes **only** inferred `[[route]]` blocks and exact normalized
> `queries`".
> §4.2 E (line 387): only in **`learn_mode`** (i.e. `dev`, via `scheduler`) does an unrouted request
> get `PUSH __unrouted__:<method>:<path>\n` → `OK\n` on `learn_ctl_path`.
> §4.2 G.1 step 5-6 (lines 445-448): `check` spawns `pytest -p blastit._check_pytest`,
> "joins `obs_pipe` …, reads `<runtime_dir>/http_obs.jsonl` …, and runs either `audit_v1::evaluate`
> or `audit_v1::plan_learn` + `apply_edits`".

The `check` path never states the equivalent attribution step. `WireSqlObs.target` is documented as
"route name or `__unrouted__:<METHOD>:<PATH>`" (§4.2 F, line 416), and `HttpObsLine` (line 453) has
no route field at all, so the only possible producer of an `__unrouted__` target inside `check` is
`BlastitWsgiGate` — but the gate's protocol section (§4.2 G.1) and R30 never say it emits a target,
and leaf 9's scope (line 651) only says "`ctl.sock` `PUSH`/`POP`". If the gate pushes the *matched*
route name only (the natural reading of "records raw request metadata"), then every statement from an
unrouted request arrives with an empty `active_route_stack`, is classified by §4.2 F as
migration/fixture SQL ("`None` … `Allow` with **zero** `obs_fd` output"), and is **dropped**. Then:

- the `[[route]]` that `--learn` proposes for a new path gets `queries = []` (an empty grant, which
  the next non-learn run reports as `missing_grant`), and
- R27's headline promise ("writes exact `queries`" for what the suite exercised) is false for the
  one case `--learn` exists for: routes that are not in the manifest yet.

R27 also has **no row in §7.2 at all**. Enumerating the table: row 1 `R1,R2,R6,R9,R10,R18,R23`;
row 2 `R7,R8,R11,R24,R29`; row 3 `R14,R15`; row 4 `R13,R16`; row 5 `R12,R26`; row 6 `R24,R25`;
row 7 `R26,R31`; row 8 `R17,R19-R22,R29`; row 9 `R3-R5,R28,R30,R32-R38`; row 10
`R3-R5,R30,R39-R42`. `R27` is absent, and §8 Leaf 8 lists it only inside a `R27..R38` range.

**Failure mode.** `blastit check --learn` on a manifest with one catch-all route writes proposed
routes with empty query lists, prints `"wrote": true`, and the next `check` fails with
`missing_grant` — with no test able to distinguish that from correct behaviour, because no test states
what the proposal for an unrouted request must contain.

**Suggest.** Add to §4.2 G (and R30): "`BlastitWsgiGate` sends `PUSH <matched route name>` or
`PUSH __unrouted__:<METHOD>:<PATH>` before invoking the app and `POP` after; in `DynamicCheck` a
target beginning `__unrouted__:` is recorded (never treated as setup SQL)". Then define
`plan_learn`'s assembly in one paragraph: group `WireSqlObs` by `target`, map each
`__unrouted__:M:P` through `infer_routes`, assign the union of observed queries to the proposed
route, drop statements already granted, refuse the whole write on any refusal. Finally give R27 its
own row: a golden test where an unrouted `POST /orders/42/` runs two statements and the emitted TOML
contains `[[route]] name = "orders"` with exactly those two `queries`.

---

## MAJOR

### T2-M1 — Every stdout string in §1 is declared normative, and none of them is asserted anywhere

> §1 (line 9): "Every flag, TOML key, exit code, stdout/stderr guarantee … specified in the user
> documentation is normative."

§7.2 maps requirements, not outputs. Unasserted today:

- the 3-line banner and its pluralization (§1.2, lines 34-40: `1 route` vs `<N> routes`), plus the
  `… · uid <euid> (EXPOSED, no auth)` variant that `docs/getting-started.md:44-48` and
  `docs/local.md` show verbatim;
- the human summary (§1.3, lines 47-62): `blastit check: 412 requests, 977 statements, 3/3 routes,
  clean`, the findings block, `1 problem` vs `6 problems`, and the `, <P> proxies` suffix;
- the `--learn` line (§1.3, line 66) and its `(dry-run)`/`proposed` variant, on the correct stream
  (`stdout` in `pretty`, `stderr` under `BLASTIT_LOG_FORMAT=json`).

§7.2 row 8's positive cell mentions `privilege`, `port`, `MockPgServer`, secret stripping, `WARN`
and `dev --learn` writes — never the banner. R22 requires the banner; no test names it. These strings
are the entire human DX surface and the most likely thing to drift during a 9-leaf rewrite.

**Suggest.** One table test per stream: run `blastit dev --port 0` in `pretty`, capture stdout,
assert the three exact lines and `1 route`; run it with two routes and assert `2 routes`; run
`blastit check` in audit mode with a golden `CheckObsV1` and assert the human block byte-for-byte
(`A` for `1 problem`); run `--learn --dry-run` twice (pretty + json) and assert the line and its
stream. Put the expected strings in the design table so a docs edit and a code edit cannot diverge.

### T2-M2 — An `infer_routes` golden row is not derivable from the naming algorithm (§4.2 G.4 vs the rule above it)

> Rule (line 489): "first static segment lowercased with non-`[a-z0-9_-]` runs collapsed to `-` and
> leading non-`[a-z0-9]` stripped".
> Row (line 501): `["/café/1"]` → `("caf", ["/café/[0-9]+/"])`.

`café` → `caf-` (the `é` is a non-`[a-z0-9_-]` run, and only *leading* characters are stripped), so
the rule yields `caf-`, not `caf`. Same class of doubt for `["/_internal/ping"]` → `internal`
(consistent, leading `_` stripped) and for `["/123/456"]` → `route` (consistent). Since §7.2 row 9
makes this table *be* the test, the test cannot be written from the spec: the implementer will
either special-case `café` to match the table (a mutant factory) or write `caf-` and fail the leaf.

**Suggest.** State the full normalizer, e.g. "lowercase, map every run outside `[a-z0-9_-]` to `-`,
then trim leading **and trailing** `-`, then if empty use `route`", and add one row with a non-ASCII
segment that is not the first (`/products/café/`) so trimming is pinned.

### T2-M3 — `fix.to` is written in meta-notation (`[route.<P>].queries`) while the normative user docs show a substituted string; a golden fixture will enshrine whichever one the implementer guesses

> §4.2 G.3 (line 479): `"fix": {"action": "add_query", "to": "[route.<P>].queries"}`.
> `docs/ci.md:70`: `"to": "[route.db].queries"` (proxy name substituted).

`<P>` is not a valid TOML table. Every one of the nine kinds' `to` (and the group variant
`"[group.<G>.<P>].queries"` on line 480) is a string that lands in the JSON contract, and
`docs/ci.md:83-86` tells agents to apply it. A cheap implementer reading the design literally emits
`[route.<P>].queries`; the golden test they write then agrees; the contract in `docs/ci.md` breaks
and an agent's `add_query` fix writes to a nonexistent table.

**Suggest.** Replace each with a concrete example *plus* one sentence: "`<P>` denotes the proxy name
of the finding; for route `catalog` and proxy `db`, `to` is the literal string
`[route.db].queries`". Then quote one full expected JSON line per kind in §4.2 G.3 (the current
table is close; `docs/ci.md:69-76` already has six of them).

### T2-M4 — The critical harness has no isolation contract, and `make check` runs the env-mutating tests twice

> R42 (line 152) and §7.1 (line 563): tests must be unprivileged and never skip.
> §7.2 row 1 (line 575): asserts `BLASTIT_*=""` precedence, `BLASTIT_PRESET`, `BLASTIT_SECCOMP=0`,
> and `--manifest` rejection — all process-global environment.

`Makefile` `check: … rs-test test-critical …` where `rs-test` is `cargo test --workspace
--all-targets` (which already includes every `tests/critical_*.rs`) and `test-critical` runs each of
them again. Neither passes `--test-threads=1`; only `.github/workflows/ci.yml:114` does, for the
`blastit` crate under `sudo`. With `std::env::set_var` (process-global, `unsafe` in edition 2024)
called from `critical_config_and_presets.rs` while sibling tests in the same binary spawn five
processes and read the environment, the design's "fast deterministic" claim is not backed by
anything. Same for port allocation: `--port 0` fixes the *bound* port, but tests that bind a
`MockPgServer` listener on a fixed port reintroduce the race `--port 0` was added to kill.

Also unaddressed: hosts where seccomp cannot be installed. `critical_dev_and_fail_closed.rs`
(§7.2 row 8, line 582) asserts the `egress = false` view gets **`EPERM`** on
`socket(AF_INET)`. On a kernel/arch/container where `prctl(PR_SET_SECCOMP)` fails, the child exits
first and the assertion fails with an unrelated message; with zero-skip that is a red `make check`
on a host that is merely unsupported. `EPERM` vs `ENOSYS` and the seccomp-v1 fallback also differ.

**Suggest.** Add a harness paragraph to §7.1: critical files that touch process env or global state
run under `--test-threads=1` (or hold a documented mutex), each spawns its own runtime dir, and
`MockPgServer` always binds port 0 and reports the port; add a preflight assertion with an
actionable message (`seccomp filter installation failed: kernel …; this host cannot run Blastit's
critical tier`) and accept `EPERM | ENOSYS` where the kernel legitimately differs; stop running the
critical suites twice (drop them from `rs-test` via `--exclude` or make `rs-test` skip
`critical_*`).

### T2-M5 — Leaf acceptance commands cannot pass on a cold checkout (Leaves 7 and 8 need binaries they do not build)

> Leaf 7 (line 639): `cargo test -p blastit --test critical_config_and_presets --locked && cargo test
> -p blastit --test critical_dev_and_fail_closed --locked && UV_OFFLINE=1 make check`.
> Leaf 8 (line 646): `cargo test -p blastit --test critical_check_engine --locked && …`.

`blastit dev` and `blastit check` spawn `blastit-dbproxy` (§4.2 G.1 step 3, ADR-0001 line 20) and
`blastit check` spawns `python -m pytest`; `blastit`'s CLI is `python -m blastit`, which needs the
`_blastit` cdylib. Neither leaf's acceptance command builds either artefact. On a fresh checkout the
order is: `cargo test -p blastit --test critical_dev_and_fail_closed` → compiles the test binary but
not the `blastit-dbproxy` bin target (it is a different package) → the test either fails "dbproxy
not found" or silently asserts less. The `make check` half would build them only if `test-critical`
does (R42 says it does), so the two-command acceptance is not "and", it is "the second command is the
only real one" — which defeats the purpose of per-leaf evidence and will confuse a cheap implementer
into weakening the test to make the first command green.

Also worth pinning: where does `check.rs` look for `blastit-dbproxy`, and how does a test override it?
(`BLASTIT_DBPROXY` / `CARGO_BIN_EXE_` / sibling of `current_exe`) is unstated, so the test cannot be
written portably and a wrong default will make Leaf 8's file depend on `make` having run first.

**Suggest.** Prefix both acceptance lines with the build:
`cargo build --locked -p blastit-dbproxy -p blastit-py && …`; state the `blastit` → `dbproxy` lookup
contract in §4.2 G.1 (env override first, then sibling of the executable, then `PATH`) and assert it
in one test.

### T2-M6 — The test plan needs files that no leaf's `paths` allow it to create

> §7.1 (line 568): "`MockPgServer` (in-process Rust PG v3 TCP server) + deterministic stub `pytest`
> package fixtures".
> §7.2 row 9 (line 583): "golden `CheckObsV1` fixtures covering …".
> Leaf 7 paths (line 636): `… "crates/blastit/tests/critical_config_and_presets.rs",
> "crates/blastit/tests/critical_dev_and_fail_closed.rs"`.
> Leaf 8 paths (line 643): `… "crates/blastit/tests/critical_check_engine.rs",
> "crates/blastit-py/src/**", "crates/blastit-py/tests/critical_check_schema.rs"`.

`MockPgServer` is needed by *two* test files in Leaf 7. The repo's own convention is a shared helper
module (`crates/blastit-http/tests/common/`, `crates/blastit-manifest/tests/common/`,
`crates/blastit-dbproxy/tests/common` are real directories), and `crates/blastit/tests/` currently
holds only `smoke.rs` — so the natural home is a new `crates/blastit/tests/common/`, which is **not**
in Leaf 7's paths. The golden `CheckObsV1` fixtures are files, not code; `crates/blastit/tests/`
fixtures are likewise not in Leaf 8's paths, and `crates/blastit-py/tests/` does not exist at all.
`plan.py` enforces `paths`, so each leaf hits a lane violation and either duplicates the server in
both files (two oracles that can drift) or inlines JSON in Rust (no longer a "fixture").

**Suggest.** Change Leaf 7 and Leaf 8 paths to directory globs where the directory is new
(`"crates/blastit/tests/**"`, `"crates/blastit-py/tests/**"`, `"python/tests/critical/**"`), and name
the files the design already promises in the Scope text so the glob does not become a licence to
touch unrelated tests.

### T2-M7 — The zero-skip and protected-test machinery the plan leans on is unenforced where it matters

> R42 (line 152): "`Makefile` `test-critical` mechanically rejects any `#[ignore]`/`needs_root`/
> `skip`/`importorskip`".
> `Makefile:40` (verified): `critical-guard` → `tools/critical_guard.py --base $(or $(BASE),HEAD)`,
> i.e. `--base HEAD` on every developer run, which sees no diff.

Two gaps. (a) The protected-test guarantee that §7.2 relies on for 10 new critical files is a **no-op
in `make check`**; only an explicit `BASE=<sha> NODE=<id>` run detects a weakened file, and pass 1's
T-m1 is not in §9.2's resolution list. (b) A literal grep for `skip` is both over- and
under-inclusive: it misses `#[cfg_attr(any(), ignore)]` and `cfg!(test)` early returns, and it matches
the word "skip" in a comment or an assertion message (e.g. a test that asserts a `dead_grant` query
is "skipped when table-granted"). The existing tree proves the habit it must police is real:
`crates/blastit-dbproxy/tests/postgres.rs:47` prints "skipping the PostgreSQL integration tests" and
`crates/blastit/tests/smoke.rs:1601,1677` print `skipped: …` — both inside `rs-test`, so `make
check` is already partly green-by-skip today.

**Suggest.** Make the guard a real gate: `make check` computes `BASE` as the merge base with
`origin/develop` (or the last commit that touched `plan/`), and the skip check is a small Rust/Python
AST-or-regex pass allowing an allow-list of files (with a justification comment on the same line).

### T2-M8 — The global body ceiling has a `Content-Length` oracle only; the chunked path is untested

> R12 (line 111): "`Content-Length > BLASTIT_MAX_BODY_BYTES` is rejected upfront at `blastit-http`
> with `413`".
> §4.3 (line 511): "HTTP request body … `blastit-http` (`Content-Length` upfront `413` + **`Dechunker`
> cap**) & `blastit-scheduler` ingress buffer cap".

§7.2 row 4's negative twin is a 10 MiB scripted upload whose `poll_read` panics after
`RespHead(413)` — that is the *early-abort* oracle (R13), and its scripted stream never exercises
`Transfer-Encoding: chunked` accumulation. There is no test for: chunked body that exceeds the cap
without a `Content-Length`; a chunked body exactly at the cap; a chunked body whose declared chunk
sizes sum past the cap while individual chunks are small. The scheduler's "ingress buffer cap" has no
oracle either. This is the one budget in §4.3 that a hostile client controls the framing of, and
`crates/blastit-http/tests/limits.rs` (existing standard test) shows the pattern to copy.

**Suggest.** Add three rows to the §7.2 row-4 negative cell and name them in
`critical_early_abort.rs` (or a `critical_body_cap.rs`): `CL: cap+1` → `413` before any read;
chunked totalling `cap+1` in 64 KiB chunks → `413` with `Connection: close` and no scheduler frame
emitted; chunked totalling `cap` → forwarded with rewritten framing.

### T2-M9 — The CLI flag-rejection matrix (R1-R3) has no table-driven oracle

> R1 (line 94): `--preset` rejected on `--stage=1`/`--role=*`, unknown flags → exit `1`.
> R2 (line 95): `blastit dev` rejects `--manifest`, `--preset`, invalid port/IP → exit `1`.
> R3 (line 96): `blastit check`/`test` rejects `--manifest`, `--preset`, unknown flags → exit `2`;
> `--dry-run` "valid only with `--learn`; error exit `2` otherwise".

§7.2 row 1 asserts only `dev --preset`/`--manifest`; nothing covers `check --manifest`,
`check --preset`, `check --dry-run` without `--learn`, `blastit --stage=1 --preset x`, an unknown
flag on either subcommand, `dev --port 70000`, `dev --host 999.999.999.999`, or `--preset` on
`blastit test`. That is ~10 documented rows with exact exit codes and an exact "stderr only, 0 bytes
on stdout" contract, all cheap to test and all easy to get wrong.

**Suggest.** Add a `#[test] fn cli_matrix()` table in `critical_config_and_presets.rs`: `(argv, env,
expected_exit, stderr_contains, stdout_is_empty)` with one row per documented sentence, and make the
design's §1.1 table its source.

### T2-M10 — R40/R41 are mapped to `python/tests/critical/*` but neither has an assertion there

> §7.2 row 10 (line 584) claims `R39..R42`; the listed assertions are all `check`/`fold` behaviour.
> R41 (line 151): `ruff check`/`ruff format --check` gated by `Makefile` `py-lint`/`py-format`.
> R40 (line 150): "`pyproject.toml` `dependencies = []` remains empty".

R41 is a Makefile property: if `py-lint` is extended (Leaf 6 does that), the requirement is satisfied
by a `make` target, not by a Python critical test — a test cannot assert that a Makefile target is
part of `check`. R40 has no test at all: nothing prevents `uv add` from writing a `dependencies`
entry, and the design's "100% stdlib" claim is load-bearing for the offline critical tier.

**Suggest.** Give R40 a real oracle (a test that loads `pyproject.toml` with `tomllib` and asserts
`project.dependencies == []`, plus a grep that no `python/blastit/*.py` imports a third-party module
outside `pytest_plugin.py`); and state R41's oracle as "`make py-lint` is a dependency of `make
check`" (a Makefile-level assertion, e.g. `make -n check | grep py-lint`), removing the false
`python/tests/critical` attribution.

### T2-M11 — `CheckObsV1` is only tested in the direction the implementer writes it

> §4.2 G.2 (line 450): `HttpObsLine`/`CheckObsV1` typed schema, "also exposed as
> `_blastit.check_audit(...)`".
> §4.2 G.1 step 6 (line 446): "reads `<runtime_dir>/http_obs.jsonl` (`HttpObsLine`)".

The producer is Python (`_check_pytest.py` writes the JSONL; `_blastit.probe_ingress` supplies
`BodySample`), the consumer is Rust (`serde` into `HttpObsLine`). Section 2 bundles both, but no test
asserts that a document produced by the Python writer deserializes into `CheckObsV1` — the goldens
are hand-written JSON in Rust, so renaming `body_len` on one side keeps everything green while
`check` sees an empty `http` vector at runtime. T-M6's `probe_ingress` fix removes the *parser*
differential but not the *schema* differential. Also unspecified: what `check` does if
`http_obs.jsonl` is absent, truncated, or contains an unknown field. (R37 turns 0 observations into
exit `1`, so this is not silent-green — but "the test suite ran, and every request was silently
dropped, so `check` reports `no_observations`" is a misleading message, not a fail-closed one.)

**Suggest.** One conformance test in `python/tests/critical/test_check_contract.py`: call the real
`http_obs` writer for a request, feed the bytes to `_blastit.check_audit` with a fixture manifest, and
assert `requests == 1`; add a Rust test that a malformed line produces a named error (exit `2`,
`blastit check: <runtime>/http_obs.jsonl: <reason>`), not an empty observation set.

---

## MINOR

### T2-m1 — `--learn --json` still has no `schema_version`
§4.2 G.3 / R29 emit `{"proposed": …, "wrote": …}` while the audit document carries
`schema_version: 1`. `docs/ci.md:143-145` matches, so this is not a doc drift, but an agent (and a
test) cannot classify a stdout document without trying both shapes. Add the field, or add one
sentence to §1.3 and `docs/ci.md` saying why it is absent.

### T2-m2 — R35 still asks itself a question
Line 140: "…(suppressed when `requests == 0` and `no_observations` already fires? See §4.2 H
interaction matrix: …)". The following clause does answer it, and §4.2 G.3 answers it again
differently ("whenever `requests > 0`"). Delete the rhetorical question and keep one normative
sentence; a requirement with a `?` in it is not a requirement.

### T2-m3 — §4.2 A's canonicalization matrix is missing four rows real frameworks hit
Not covered: `application/vnd.api+json` and `application/ld+json` (R14 explicitly allows
`application/*+json`, so at least one suffix row is needed); `charset=UTF-8` in upper case (RFC 2045
charsets are case-insensitive — the row currently proves only `utf-8` and `ibm037`, so an
implementer comparing `== "utf-8"` passes every listed test); number lexemes (`1e0`, `1.0`, `-0`,
`9007199254740993`) whose `serde_json` round-trip is not byte-preserving; and `DELETE` with a body
(the matrix only gives `DELETE` with `0` bytes, while `docs/reference/manifest.md` allows `methods`
with bodies).

### T2-m4 — The redacted form of a prefixed literal is unpinned
§7.2 row 6 says all eight forms are "redacted to `…`". For `B'1'`, `X'ff'`, `E'\n'`, `U&'\0041'` the
lexer may emit `…` (prefix swallowed) or `B…` / `X…` / `U&…` (prefix kept). Both pass
`has_quoted_literal` and both are refused, so the test cannot tell them apart — but the string is
user-facing ("safe to paste into a ticket") and lands in `sql.denied`. Add the exact expected output
for `SELECT B'1'`, `SELECT E'\n'`, `SELECT U&'\0041'`, `SELECT $t$x$t$`.

### T2-m5 — The `BLASTIT_HEALTH_PATH` exclusion in R32 is unreachable, so asserting it is fluff
`ARCHITECTURE.md:363` and `docs/reference/settings.md:28`: the scheduler answers the health path
"itself, without a view process", so it never enters `BlastitWsgiGate` and can never appear in
`HttpObsLine`. §4.2 G.2's exclusion is therefore vacuous. Either delete it from R32/§4.2 G.2 or test
it where it can fire (scheduler-level `HttpObsLine` production), otherwise a cheap implementer writes
`assert!(obs.iter().all(|l| l.path != "/_blastit/health"))` over an empty vector.

### T2-m6 — R5 promises empty stdout on exit `3`; `docs/ci.md:99` promises it only on exit `2`
"On exit `2`, stdout is empty and the error is on stderr." The design (line 98) and ADR-0004 §2
extend the promise to exit `3`. The oracle will assert `b""`; the doc must say so. `docs/**` is out of
MA.2's reach, so name the sentence in MA.3's scope rather than leaving the mismatch live.

### T2-m7 — `count >= 6` is the wrong comparator
§7.2 row 2. With `>=`, deleting a fence (or retitling one) shrinks the gate without failing. Assert
the exact six `(file, line)` pairs, or better: a checked-in fixture list the test iterates, so adding
a fence is a deliberate edit rather than silent coverage expansion the test then demands.

### T2-m8 — "offline `make check`" is only as offline as `uv`
R42's acceptance is `UV_OFFLINE=1 make check`, but `check` also runs `cargo test --workspace
--all-targets --locked`, `clippy`, `doc` and `uvx --offline zensical`, none of which inherit
`UV_OFFLINE`. `cargo --locked` still consults the network if a registry entry is missing from the
cache. If offline is a real claim, the recipe needs `CARGO_NET_OFFLINE=1` (and a CI job with no
network namespace, e.g. `unshare -n make check`) — otherwise the acceptance command proves only that
the developer's caches were warm.

---

## What pass 2 gets right (keep it)

- §4.2 A's matrix is now *executable*: input bytes, `Content-Type`, and exact output bytes or exact
  `IngressReject` variant. That is what a critical test needs, and it is the single biggest
  improvement over pass 1.
- §7.2's per-row positive/negative twins with the enforcing crate named — R13/R16 moved to
  `blastit-http`, R31 to `blastit-dbproxy`, R39 to the Python contract file — fixes pass 1's T-B4
  misrouting (only R27 is still missing).
- §4.2 F's "`DynamicCheck` **always** returns `QueryDecision::Allow`" plus "assert HTTP 200, one
  `missing_grant`, `denied == 1`" (§7.2 row 7) kills the contradiction pass 1 found and is
  mutation-resistant.
- R42's offline recipe now names the artefacts (`blastit-py`, `blastit-dbproxy`), the staging step and
  both runners; `UV_OFFLINE=1` acceptance and the 12-row `infer_routes` table are exactly the right
  shape.
- `crates/blastit-dbproxy/tests/critical_learn_and_redaction.rs`'s "verified by raw byte scan across
  entire pipe" for `'s3cr3t'` is a genuinely strong oracle (it cannot pass by assertion omission).

## Suggested entries for §9.2 (replace the current table rows where noted)

| ID | Severity | Resolution |
|---|---|---|
| `T2-B1` | BLOCKER | `accepted -> §2, §7.2, docs repair owner`: `docs/local.md:114` and `docs/reference/manifest.md:48` teach grants that `R24` rejects; fix the two sentences (name the node) and move the fence test to a standard test over a checked-in `(path, line)` fixture list with a failing negative canary. |
| `T2-B2` | BLOCKER | `accepted -> §7.2, MA.2 acceptance`: add `python/tests/test_check.py` as the real-pytest/real-PostgreSQL green-path oracle (`requests > 0`, `findings == []`, exit `0`, plugin actually loaded); say the stub file proves only fd-1 purity and exit mapping. |
| `T2-B3` | BLOCKER | `accepted -> R27, R30, §4.2 G`: `BlastitWsgiGate` pushes `<route>` or `__unrouted__:<M>:<P>`; `DynamicCheck` records `__unrouted__` targets; define `plan_learn`'s assembly; add R27's own §7.2 row. |
| `T2-M1` | MAJOR | `accepted -> §7.2`: assert the §1.2 banner (incl. pluralization and EXPOSED), the §1.3 human summary block and the `learn:` line on both streams. |
| `T2-M2` | MAJOR | `accepted -> §4.2 G.4`: pin the name normalizer (trim leading *and* trailing `-`) and add a non-ASCII middle-segment row. |
| `T2-M3` | MAJOR | `accepted -> §4.2 G.3`: replace `<P>`/`<G>` meta-notation with worked literals (`[route.db].queries`) plus one full expected JSON line per kind. |
| `T2-M4` | MAJOR | `accepted -> §7.1`: critical-harness contract (`--test-threads=1` for env/global tests, port 0 everywhere, seccomp preflight with an actionable message, `EPERM｜ENOSYS`), and stop running the critical suites twice. |
| `T2-M5` | MAJOR | `accepted -> §8 Leaves 7, 8`: prepend `cargo build --locked -p blastit-dbproxy -p blastit-py` and state the dbproxy lookup contract. |
| `T2-M6` | MAJOR | `accepted -> §8 Leaves 7, 8, 9`: widen those paths to the new test/fixture directories the plan already requires. |
| `T2-M7` | MAJOR | `accepted -> R42, Makefile`: make `critical-guard` compute a real base and replace the `skip` grep with an AST-aware allow-listed check. |
| `T2-M8` | MAJOR | `accepted -> R12, §7.2 row 4`: chunked cap (over, at, and under) plus the scheduler buffer cap, asserted in `critical_early_abort.rs`. |
| `T2-M9` | MAJOR | `accepted -> §7.2 row 1`: one `(argv, env, exit, stderr, stdout-empty)` table covering every documented rejection in R1-R3. |
| `T2-M10` | MAJOR | `accepted -> R40, R41`: add a `dependencies == []` oracle and state R41's oracle as a `Makefile`-level assertion. |
| `T2-M11` | MAJOR | `accepted -> §4.2 G.2, §7.2 row 10`: producer-to-consumer conformance test plus a named error for a malformed `http_obs.jsonl`. |
| `T2-m1..m8` | MINOR | `accepted -> §1.3, R29, R32, R35, R42, §4.2 A, §7.2 row 2`: `schema_version` on the learn document, delete R35's question mark, four missing canonicalization rows, pinned redaction output for prefixed literals, drop the unreachable health-path exclusion, document exit-3 empty stdout in MA.3's scope, assert the exact six fences, and add `CARGO_NET_OFFLINE=1`. |
