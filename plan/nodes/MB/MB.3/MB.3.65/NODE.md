---
id: MB.3.65
title: "Descope PHP 8.2: supported versions are 8.3 and 8.4"
kind: task
status: ready
owner: implementer
priority: 1
depends_on: [MB.3.64]
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/config.rs
  - crates/blastit/src/cli.rs
  - "crates/blastit-php-sapi/**"
  - "crates/blastit/tests/**"
  - "e2e/php-sapi/**"
  - e2e/php_sapi_probe.py
  - "scripts/**"
  - ".github/**"
  - Makefile
  - ARCHITECTURE.md
  - docs/frameworks/php.md
  - "docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md"
  - docs/knowledge/LESSONS.md
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-15T21:43"
updated: "2026-11-15T21:43"
---

# Goal

Descope PHP **8.2** completely and truthfully. The supported set is **8.3 and 8.4**, and the
product statement is one sentence: *we support supported PHP versions, which are 8.3 and 8.4.*
No fallback, no `get_zend_version()` wiring, no "yet".

# Context

Owner decision (2026-11-15): 8.2 is not a supported PHP version, so we stop shipping, probing,
discovering and explaining it. `MB.3.61` already made discovery version-major; this node removes
8.2 from it entirely.

Today the tree still: declares **12** shims (3 minors × 2 arches × 2 libcs), builds
`libblastit_php82-*`, probes `/usr/lib/php82` and the `libphp8.2.so` / `libphp-8.2.so` /
`libphp82.so` spellings in `stage0.rs`'s `LIBPHP_PROBES`, and tells an 8.2 host a story about a
missing `php_version` symbol and `.dynsym` inspection. A user must never have to learn Blastit's
internals to hear "not supported".

# Acceptance

```console
$ uv run --no-project scripts/platform_matrix.py --summary
$ make check
$ docker run --rm --cap-add SYS_PTRACE <bookworm + libphp8.2-embed only>   # 8.2-only host
$ grep -ri 'php82\|8\.2' docs/ crates/ scripts/ .github/ e2e/ ARCHITECTURE.md
```

1. **Matrix**: the 4 `libblastit_php82-<arch>-<libc>.so` shims are gone from
   `scripts/platform_matrix.py` (`PHP_MINORS`, the `("8.2", …)` rows), from
   `.github/workflows/php-sapi.yml`'s matrix, from every archive listing and from every
   `SHA256SUMS` expectation. Shim set is **2 minors × 2 arches × 2 libcs = 8**, and the counts
   in ADR-0010 / `docs/` come from `--summary` on the run, never hand-edited.
2. **Discovery**: `/usr/lib/php82`, `/usr/local/lib/php82`, `libphp8.2.so`, `libphp-8.2.so`,
   `libphp82.so` (and every `php82` per-minor-directory spelling) are out of `LIBPHP_PROBES` in
   `crates/blastit/src/stage0.rs` and out of ADR-0010's probe-order paragraph.
3. **The refusal a real host sees is plain**: an 8.2-only host reads exactly
   `PHP 8.2 is not supported — Blastit supports PHP 8.3 and 8.4`. Delete the
   `NoPhpVersion`-as-8.2-signature framing and every "not supported yet" hedge wherever they
   reach a user (`stage0.rs` sentences, `cli.rs`, `config.rs`).
4. **ADR-0010**: shim set 2 minors (8 per matrix), no "an `8.2` host is refused for its missing
   `php_version`" sentence, no `8.2` in the probe order, `12` → `8` shims everywhere the ADR
   states a count.
5. **`docs/frameworks/php.md`**: the whole 8.2 paragraph (the `php_version` explanation, the
   "host-package fact, not a Blastit bug" line) is replaced by the single support statement.
   Keep the Alpine unversioned-name guidance (still true for 8.3/8.4); keep nothing explaining 8.2.
6. **Tests**: the protected `critical_php_embed` / `critical_php_backend` 8.2 case is rewritten to
   assert the unsupported-version refusal **and its sentence**, and to go red if that sentence
   regresses (paste a mutation run: change the sentence in a /tmp copy of the source, show the
   failure, restore). No 8.2 fixture remains in `e2e/php-sapi/**`; the two-package image keeps
   proving what it proves (8.4 over 8.3) without 8.2.
7. **Grep clean**: `grep -ri 'php82\|8\.2' docs/ crates/ scripts/ .github/ e2e/ ARCHITECTURE.md`
   returns nothing that claims or explains 8.2 support (release history in `CHANGELOG.md` is fine;
   the `8.2` in unrelated `blastit-http`/`dbproxy` defaults and design/review history is not this
   node's subject — name what you left and why).
8. `make check` exit 0; `uv run --no-project tools/critical_guard.py --base <base> --node MB.3.65`
   authorizes the protected edits; no assertion weakened; no `#[ignore]`.

# Notes

- Sequencing: after `MB.3.64` (both edit `stage0.rs` and the protected suites).
- aarch64/musl legs are **CI-only**: never claim one is proved from here.
- This is a descope, not a workaround: do not add a version-detection fallback of any kind.

# Owner additions (2026-11-15, same node: same files, same writer)

## A. No apology language for a constraint we chose

A doc states the support set and stops. It does not explain, excuse or soften a decision we made
on purpose — that tone is how "8.2 is not supported yet … a host-package fact, not a Blastit bug"
survived three reviews.

- The one real apology is `docs/frameworks/php.md:46-48`: delete the paragraph, and do **not**
  replace it with a shortened excuse. The replacement is the single support sentence.
- **Keep** `docs/frameworks/php.md:44` and `docs/reference/manifest.md:170` ("not a degraded
  mode") and `ARCHITECTURE.md`'s "for now that means the database": those are facts about
  behaviour and scope statements, not apologies. Do not touch them.

## B. A guard, because an unchecked tone rule comes back

Add a narrow **banned-phrase** check over the user-facing docs to `make check`, with negative
self-tests.

- Covered set: `docs/**/*.md`, **excluding** `docs/design/`, `docs/decisions/`,
  `docs/research/`, `docs/knowledge/`, `docs/testing/` and `docs/VISION.md`. `README.md` is
  user-facing too: cover it as well, and say in `RESULT.md` exactly which set the guard walks.
- The list is narrow and literal. The phrases that must be refused are exactly:

  ```text
  not a Blastit bug            we apologize / apologies
  host-package fact            unfortunately / sadly
  not supported yet            self-inflicted
  sorry
  ```

  No bare "limitation", "workaround", "for now" or "degraded" — those are legitimate technical
  words, and a fuzzy rule either fires on real sentences or gets disabled by the first person it
  annoys.
- **Negative self-tests** (pytest under `scripts/`, beside `scripts/test_packaging_gates.py`):
  a fixture file containing each banned phrase proves the check exits non-zero and names the
  phrase and the line; a clean fixture must pass. The guard must not be "fixable" by emptying
  the list, so also assert the list itself is non-empty and that each phrase is still detected.
- Acceptance for this part: `grep -rn` for each banned phrase over the public set returns
  nothing, the guard fails on the dirty fixture and passes on the clean one, `make check` green.
- Paths for this part are already covered (`scripts/**`, `Makefile`, `docs/**` pages above).

Still sequenced last, after `MB.3.64`.
