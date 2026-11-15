# Log MB.3.59

## 2026-11-15T17:56 [implementer]

created (task) The Alpine libphp cannot be versioned by name: the boot refuses 'no shim can be chosen' unless BLASTIT_LIBPHP is versioned

## 2026-11-15T18:05 [lead]

edited fields

## 2026-11-15T18:05 [lead]

Lead: paths widened for what this node actually needs — docs/frameworks/php.md (acceptance 4 edits that page), crates/blastit-php-sapi/src/** (the refusal sentences and the loader's MinorMismatch live there), and crates/blastit/tests/** because the protected suites assert those sentences and will need the node-scoped critical_guard authorization. The Makefile is invoked, not edited. Fix must be STATIC resolution: stage 0 is zero-dlopen by R11, so no stage-0 load may become the source of the minor.

## 2026-11-15T18:06 [lead]

edited fields

## 2026-11-15T18:06 [lead]

Lead: paths re-applied — the planner's body rewrite wrote its own (stale) frontmatter back over my earlier edit, which is the two-writers-on-one-file hazard this repo has a lesson about. Final paths: crates/blastit/src/**, crates/blastit-php-sapi/src/**, crates/blastit/tests/** (protected refusal assertions; needs critical_guard --node MB.3.59), e2e/php-sapi/**, scripts/**, docs/frameworks/php.md (acceptance 4 edits it). The Makefile is invoked, not edited.

## 2026-11-15T18:23 [lead]

Reviewer PASS on attempt 4 with six MINORs, all folded here rather than fixed twice — this node's discovery fix will rewrite `docs/frameworks/php.md`'s passage anyway, so the corrections land WITH the re-measure:
1. `docs/frameworks/php.md:50` "state no version in their names" is false for `libphp83.so` (packed 83 IS 8.3) — stage 0 refuses it as "no PHP major.minor" (`stage0.rs:821`); say "state no dotted major.minor".
2. `:52-54` both `install` commands are Alpine-only — prefix "On Alpine," so a Debian skim cannot run a path Debian does not have.
3. `:56` "and set BLASTIT_LIBPHP" reads required, but `/usr/local/lib` is in `library_dirs()` (`stage0.rs:602-617`), so the copy alone is discovered: "the copy is enough; set BLASTIT_LIBPHP only if it lives elsewhere".
4. `:42-48` "the one privileged step for PHP itself" becomes two on Alpine (package + root copy), and "8.2 is built but not served" is matrix jargon → "8.2 is not supported yet: its package ships no `php_version` (a host-package fact, not a Blastit bug)".
5. `RESULT.md:157-160` — "the gate and the release body cannot drift apart" still implies CHANGELOG prints it, and "only places it is printed" contradicts the H-009 quote in the same sentence → "the gate and `--summary`".
6. `NODE.md:120-129` Notes still say php.md is "not in `paths`" and "only the first three" — strike both; frontmatter now lists php.md and crates/blastit/tests/**.
Also: `make docs-unit`'s vocabulary scan is tree-wide (`test_docs_assets.py:802`, `FALLBACK_TERM:197`) and it flags "fallback" in live node bodies — MB.3.59's body used it, so this command must run before any commit that adds a node body.
