# Log MB.42.1

## 2026-11-19T14:30 [conductor]

created (task) Write ADR-0017 & technical design for universal libpython-free binary, local CLI handoff, .deb/.rpm + install.sh, py3-none wheels & Composer wrapper

## 2026-11-19T14:31 [lead]

status ready -> active

## 2026-11-19T14:31 [lead]

attempt 1/3 started

## 2026-11-19T14:31 [lead]

base 8be9d3ead248b3b943856da533562814cfdc2ca6

## 2026-11-19T14:37 [lead]

OWNER CLARIFICATION 2026-11-19: distribution goes through signed APT/RPM repositories (Cloudsmith offworld-nexus/{apt,rpm}, components main|nightly, Forgejo mirror forge.offworld.nexus), NOT bare .deb/.rpm download links; install.sh configures the repo + apt/dnf install; direct .tar.gz only for --develop/--user/Alpine; BLASTIT_DEB/BLASTIT_RPM local container verification. ADR-0017 + design must specify this.

## 2026-11-19T15:48 [lead]

OWNER REQUIREMENT 2026-11-19: PHP 8.6 end-to-end (8.3-8.6). New task MB.42.5 created; MB.42.3 now depends on MB.42.2+MB.42.5; architect notified to fold Minor::Php86, 13/17 candidates/probes, 16 shim legs, sury bookworm + alpine:edge testing legs into ADR-0017/design.

## 2026-11-19T16:42 [lead]

status active -> review

## 2026-11-19T16:46 [lead]

REVIEW FIX (filtered): B1 glibc floor false (shims need 2.36-2.41, .deb says 2.28, gate unenforceable); B2 py3-none wheel has no builder/consumer contract (maturin --bindings bin, wheel_legs keys, runtime_image replacement); UX-B1 install.sh stdin never a TTY under 'curl|sh' so repo path never runs (use /dev/tty); M1 protected test_lsp_stdio breaks (_blastit.main dropped, _bin/blastit unstaged); M2 12-op bridge lacks python signatures/return types (audit returns JSON text); M3 blastit-audit vs audit_v1 double grant-diff owner + 2156-LOC god file; M4 SHA-256 defeated when BLASTIT_RELEASE_BASE_URL set; UX-M: install-vs-lockfile precedence, ban 'uv run blastit' in user pages, BLASTIT_PYTHON/free-threaded refusal text, PHP_PACKAGES per-minor 8.6 name, ARCHITECTURE.md still contradicts ADR-0017; m: handoff should skip unsafe candidate not exit 1, pin record rationale, blastit-dbproxy fate, sury key fingerprint; plus ADR shim name typo libblastit_php8{3,4,5,6}

## 2026-11-19T17:17 [lead]

status review -> done

## 2026-11-19T17:17 [lead]

commit 5b3ce8df52aed5b4954eefd7b5543e16e4bc6ec6; fast-check green, docs-check green, critical_guard clean, guard OK
