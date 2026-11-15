# Log MB.3.58

## 2026-11-15T16:38 [lead]

created (task) Every matrix leg must build where it runs, and no leg may pass two platform tags

## 2026-11-15T16:38 [lead]

status ready -> active

## 2026-11-15T16:38 [lead]

attempt 1/3 started

## 2026-11-15T16:38 [lead]

base 73960fc

## 2026-11-15T17:20 [lead]

status active -> review

## 2026-11-15T17:27 [lead]

REVIEW FAIL (attempt 1). One BLOCKER, two MAJORs.
BLOCKER (`Makefile:640` + `critical_php_embed.rs:2241-2247`): the root+SYS_PTRACE run silently skips the ONLY assertion of `PR_SET_DUMPABLE(0)` — `no_libphp_is_mapped_above_the_view_worker` asserts `denied > 0` only in its `euid != 0` branch, and under root it takes the "content-checked, 0 denied" branch (reviewer's words: "2 content-checked, 0 denied, euid 0"). `ci.yml`'s root run never reaches that branch (no libphp on the runner), so it was the only witness, and a regression dropping `set_dumpable(false)` in `php_view.rs` would now pass everywhere. The comment calling root "the stronger witness `ci.yml` gets" is false. FIX (the reviewer proved it works): keep the root run for the full suite and ADD one `docker run --user "$(id -u):$(id -g)"` filtered to `no_libphp_is_mapped_above_the_view_worker` — `--user 1000:1000 … $bin no_libphp_is_mapped_above_the_view_worker` printed "2 denied (EACCES) ok" — then reword the comment.
MAJOR 1 (`scripts/check_workflows.py:646-661`): rule 14 does not see the defect it was written for. Restoring the pre-node php-sapi step (`cargo test --no-run` + `docker run "$binary"`) in a /tmp copy still reports `358 checks, ok` because the rule needs a literal `target/debug|release` or `/tmp/artifact`. Make it flag ANY job that builds Rust for the host and then `docker run|exec`s anything unless it invokes `check-exec-env.sh`, add that exact shape as a self-test, and state the limitation if any remains in RESULT §4.
MAJOR 2 (`scripts/platform_matrix.py:190-204` PROVEN/summary, `plan/HUMAN.md:70`): "6 built & exercised shims" reads as six WORKING shims. Measured per leg: 8.2 gnu = built+probed+REFUSED (`nm -D /usr/lib/libphp8.2.so`: no `php_version` export); 8.3/8.4 gnu = served; 8.2 musl = built+probed, would refuse (alpine `libphp82` also exports none); 8.3/8.4 musl = built+probed, served unmeasured. SERVED = 2 of 12 today (4 once the aarch64 gnu legs run). Four of twelve shims are unusable for `[route.php]`. FIX: add a per-leg `reaches: built|probed|served|refused` field, PRINT the served count, and put the 8.2 loader fact into the 8.2-musl `suites_because` (today it blames libclang only). The install/support wording must say 8.3/8.4 are supported and 8.2 is built-but-refused, because `loader.rs` requires a `php_version` export.
MINORs: gate the 8.2 suite compile behind a `PHP_SAPI_SUITES` build arg following `suites:` (that image compiles suites that can never pass — 2/5 fail, ~7 min per build, paid by `publish-pypi`'s shims job too); `RESULT.md:129-136` says `python:3.13/3.14-slim` is Debian 12/glibc 2.36 but it is Debian 13/2.41 (conclusion stands, four rows stale); `check-exec-env.sh:101-106`'s floor regex drops `GLIBC_ABI_*` nodes (2.36+), so a DT_RELR-only binary reads as "requires no glibc symbol version" — include them; `docs/knowledge/LESSONS.md:59` and `critical_php_embed.rs:44-47` want follow-ups (the latter claims the runner builds the suites).

## 2026-11-15T17:27 [lead]

status review -> ready

## 2026-11-15T17:27 [lead]

status ready -> active

## 2026-11-15T17:27 [lead]

attempt 2/3 started

## 2026-11-15T17:28 [lead]

edited fields

## 2026-11-15T17:28 [lead]

Lead: item 3's support statement lives in docs/frameworks/php.md (the page a user reads before choosing a minor — it already names libphp-embed/php84-embed at :40). That page and docs/knowledge/LESSONS.md are now in this node's paths; docs/security.md:163-164 may need the same clause if it implies 8.2 works. Put the support line where a reader chooses: PHP 8.3/8.4 supported; 8.2 built but refused because libphp8.2 exports no php_version (a host-package fact, not our bug); musl served measured-or-unmeasured per item 4.

## 2026-11-15T18:01 [lead]

status active -> review

## 2026-11-15T18:05 [lead]

REVIEW FAIL (attempt 3): the support line is user-facing advice and one of its two sentences is wrong for the first library it names; MB.3.59's body also carries a false premise.
MAJOR 1 `docs/frameworks/php.md:48-50` — "copy the library to `libphp8.4.so`" is one target for TWO libraries: copying `libphp83.so` there selects the 84 shim and the loader refuses (`crates/blastit-php-sapi/src/loader.rs:341` `MinorMismatch`: "the shim is built for PHP 8.4 but the host's libphp reports 8.3.x"), so "the same worker serves" is false for 8.3. And it never says the copy must be ROOT-OWNED in a non-writable directory (`/usr/local/lib`, 0644): the `/tmp` copy the leg measured is refused in deployment (`stage0.rs:1053`, `host.rs:1092-1105`). FIX: per-minor targets (`libphp8.3.so` / `libphp8.4.so`) and "copy under /usr/local/lib, root-owned 0644".
MAJOR 2 `MB.3.59`'s NODE.md — Acceptance 4 orders an edit to `docs/frameworks/php.md` which is not in its `paths`, and :75 calls the Makefile target "in this node's paths" (it is not). A planner is fixing that body now; your RESULT should stop relying on the sentence it misquotes.
MAJOR 3 `MB.3.59`'s NODE.md:88-94 — "the pair is `dlopen`ed for the NTS check (`zend_abi`)" is FALSE: `stage0.rs:899-902` and `critical_php_embed.rs:5-6` pin stage 0 to zero `dlopen`, `.dynsym` only, so neither `php_version`'s value nor "what the probe reported" exists at boot. The planner is correcting it to static resolution; your RESULT should not repeat the dlopen premise.
MINORs in your scope: `docs/frameworks/php.md:47` says "`libphp8.2` package" — it is `libphp8.2-embed` (Debian) / `php82-embed` + `libphp82` (Alpine); and `RESULT.md:146-150` claims the release notes "print" the reaches sentence — `scripts/release-notes.sh` emits only `CHANGELOG.md`, which has no such line (only `--check`, `--summary` and `plan/HUMAN.md:99` quote it).
Also for the planner's benefit (state it in RESULT): the 8.4 case is a DIRECTORY gap too — `library_dirs()` (`stage0.rs:602-617`) lacks `/usr/lib/php84`, so an unset `BLASTIT_LIBPHP` cannot reach `/usr/lib/php84/libphp.so` even with a correct name.

## 2026-11-15T18:05 [lead]

status review -> ready

## 2026-11-15T18:05 [lead]

status ready -> active

## 2026-11-15T18:05 [lead]

attempt 3/3 started

## 2026-11-15T18:23 [lead]

status active -> done
