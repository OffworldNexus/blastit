# Log MB.42.2.2.2

## 2026-11-19T20:07 [unknown]

created (task) blastit --internal-json (12-row contract) + pure-stdlib _blastit.py bridge; retire crates/blastit-py

## 2026-11-19T20:30 [lead]

status ready -> active

## 2026-11-19T20:30 [lead]

attempt 1/3 started

## 2026-11-19T20:30 [lead]

base 1da3e3ddfa6782d08474220be2c6f0a6e4e49d08

## 2026-11-19T20:42 [lead]

lead decisions: (1) item 7 DECLINED - keep the audit_v1/infer_routes re-exports (one-line aliases, single implementation; retiring would edit 2 protected tests for zero gain); (2) make test-critical was red on the literal '#[ignore]' token in critical_python_shim.rs prose - a defect I introduced in .1.4, now a fix task on this node; (3) wheel pipeline still naming the deleted crates/blastit-py is now explicit MB.42.3 scope (pyproject maturin manifest-path, blastit_build.py, build-wheel.sh, wheel-smoke.sh).

## 2026-11-19T20:44 [lead]

status active -> review

## 2026-11-19T20:47 [unknown]

edited fields

## 2026-11-19T20:47 [lead]

REVIEW FIX (filtered): MAJOR pyproject.toml:83 maturin manifest-path still names the deleted crates/blastit-py -> uv build/maturin fail at config load = no wheel can build (gate blocker, paths extended); MAJOR LiveInterpreter single-impl trait + unreachable Hosted/shim_only/runtime_hooks/hosted_hooks branch survives the deletion (charter forbids single-impl ceremony); MAJOR/BLOCKER BrokenPipeError escapes as OSError where fold/pytest catch ValueError; BLOCKER bin_path failure is a raw traceback with no reinstall step and a false 'looked in BLASTIT_BIN'; BLOCKER dead child reported as a size-limit problem with stderr discarded; MAJOR --internal-json hangs on a TTY; MAJOR wrong-binary-on-PATH names nothing; minors: ValueError lacks the op, frame cap overshoot by one chunk, internal_json 199 LOC (move decoders to args.rs), stale pyo3-extension prose, CRITICAL_SKIP_PATTERN matches #[ignore] in comments so the guard (not the protected prose) must be fixed, check.rs literal false flag.

## 2026-11-19T21:10 [lead]

protected-test deletion verified (crates/blastit-py/tests/critical_check_schema.rs): its subjects are the retired pyo3 bindings; coverage migrated - schema_version rejection pinned by ObsError::SchemaVersion (blastit-audit/src/check_tests.rs:302), statements is the sum by construction (evaluate.rs:252), plan_learn/probe_ingress/corrupt-line-naming/no-secret in critical_internal_json.rs. Deletion authorized by crates/** and mandated by ADR-0017 §4.

## 2026-11-19T21:10 [lead]

status review -> done
