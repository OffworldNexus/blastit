# Log MA.2.3.2

## 2026-11-12T13:12 [planner]

created (task) Leaf 9: CLI grammar, CWD manifest discovery, presets and seccomp gate

## 2026-11-12T15:09 [lead]

status ready -> active

## 2026-11-12T15:09 [lead]

attempt 1/3 started

## 2026-11-12T15:09 [lead]

base aa401fd704bafcaf119c5196bf9bd447b10873f6

## 2026-11-12T15:17 [implementer]

attempt 1: CLI grammar, CWD manifest discovery, preset precedence, seccomp gate and preload build landed; RESULT.md has acceptance evidence; make check green

## 2026-11-12T15:17 [lead]

status active -> review

## 2026-11-12T15:23 [lead]

REVIEW FAIL (attempt 1): (1) MAJOR: Config::from_env(None) still reads BLASTIT_PRESET for dev/check, contradicting ADR-0005/help text (need PresetPolicy::{Env,IgnoreEnv} + R2 negative twin); (2) MINOR: BLASTIT_SECCOMP=0 preset abort only asserted in-process, need run_cli_env with real exit 1; (3) MINOR: unit-test env() guard must clear BLASTIT_MANIFEST/CWD; (4) MINOR: weak help assertion + missing parse_dev/parse_check grammar twins (--port 0, --port=9000, --host=127.0.0.1, check --learn --dry-run).

## 2026-11-12T15:23 [lead]

status review -> ready

## 2026-11-12T15:23 [lead]

attempt 2/3 started

## 2026-11-12T15:28 [unknown]

attempt 2: PresetPolicy::{Env,IgnoreEnv} threaded through from_env_with_format; Config::for_dev()/for_check(); R2 twin (exported preset -> preset None, Pretty, no seccomp abort); run_cli_env real-process PORT abort + seccomp never-serving; env() guard pins BLASTIT_MANIFEST at an absent fixture; parse_dev/parse_check grammar twins; fmt/clippy/doc/workspace tests + 11 critical green

## 2026-11-12T15:28 [lead]

status ready -> review

## 2026-11-12T15:28 [lead]

attempt 2 done (PresetPolicy IgnoreEnv for dev/check, real-process seccomp exit, env guard, grammar twins); sent to review

## 2026-11-12T15:32 [lead]

REVIEW PASS (attempt 2). Two MINOR findings deferred to the MA.2.3 gate test-curator: (a) critical_config_and_presets.rs:547 unreachable 'stage=config' branch makes the process-level seccomp assertion vacuous on capable hosts; (b) --manifest=x/--preset=x give 'unknown argument' instead of the actionable text and cli.rs:382 doc double negative.

## 2026-11-12T15:52 [lead]

status review -> done
