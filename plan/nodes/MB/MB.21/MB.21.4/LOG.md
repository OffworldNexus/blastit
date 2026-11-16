# Log MB.21.4

## 2026-11-16T00:38 [planner]

created (epic) Cut the product over to Blastfile: discovery, migration refusal, runtime parity, check/--learn, Python, Makefile, e2e

## 2026-11-16T02:16 [planner]

edited fields

## 2026-11-16T08:13 [planner]

rescoped children after MB.21.4.1's needs_split (no code landed, 9+6 fixtures made the flip and the translation inseparable): Children rows and the ordering sentence refreshed (4.1 = atomic cutover, 4.2 = parity + 4.3 removals, 4.4 = remaining fixtures + make e2e), acceptance 2 records that make e2e is red from 4.1 until 4.4, and the docs-test note now attributes docs/local.md's harness-executed block to 4.1 while the rest of docs stays MB.21.5.1's. 4.3 and 4.5 unchanged.

## 2026-11-16T10:10 [lead]

MB.21.4.1 closed at a3ac338 after 5 attempts and 5 sessions (this leaf grew into the whole atomic cutover: 74 files). Key outcome: the scheduler now serves static mounts and binds the file-level entrypoint php, the model audits mount roots, and every fixture make check reaches is a Blastfile. Moved to MB.21.4.2: the six reviewer MINORs (escaped path, blastfile::read, a real fmt test, RESULT.md note, doc prose, shared predicates) plus the parity suite. Moved to MB.21.4.3: the --json action vocabulary, fold/pytest plugin and _blastit.audit cutover, the e2e learn case, and the §13.2/§10.2/§11.3 re-spellings. Route::{static_policy,with_static,php,with_php} go with MB.21.4.5.

## 2026-11-16T14:44 [planner]

flake now owned by MB.21.4.8: dev_boots_five_unprivileged_roles_and_strips_secrets read 200 where its own dev log says 413 unmatched, measured 2/4 full-gate runs on this host (plus one reviewer run); the protected 413 assertion stays untouched and the test client's response pairing is bounded after MB.21.4.4.

## 2026-11-16T16:30 [planner]

the residual dev-suite flake is a product race owned by MB.21.4.9: an AllowEmptyOnly request is dispatched from its head (scheduler.rs:1487/:1507) and blastit-view starts the app on REQ_HEAD (lib.rs:474/:501), so a body byte races 413 against the app's 200 (1 ms gap: 5/5 wrong); 4.8's client fix landed and its 413 assertion is untouched.
