STATUS: done

# MB.3.30 — Docs + plan guard: the retired PHP transport vocabulary and PHP fallbacks cannot return

## What landed

Two files, both inside `paths`:

1. `python/tests/test_docs_assets.py` (+394 lines, same file `make docs-unit`
   already runs): a stdlib-only guard, `guard_tree(root) -> list[Finding]`, over
   four populations, plus 13 new tests (17 in the file) that prove each
   direction with crafted pages **and** run the guard over the real tree.
2. `docs/VISION.md` (3 lines reworded, nothing else): the two truthful denials
   that still named the retired words, plus one attack bullet whose only
   "attack marker" was the phrase *"header tricks"* (see "VISION line 57" below).

No network, no wheel, no subprocess: the whole file runs in 0.17 s.

## The guard

| Population | Fails on | Allowance |
|---|---|---|
| `docs/frameworks/**`, `docs/deploy/**`, `docs/reference/**`, `README.md` (`*.md`) | `php[-_ ]?fpm` (also `php_fpm`, `php-fpm.conf`), `route.fastcgi` (also `[route.fastcgi]`), bare `fastcgi` | none |
| `docs/VISION.md` | `php[-_ ]?fpm`, `route.fastcgi` always | bare `fastcgi` allowed only on a line carrying a marker from `ATTACK_MARKERS` |
| `plan/nodes/**/NODE.md` | same, plus bare `fastcgi` | markers from `PLAN_ATTACK_MARKERS` (`ATTACK_MARKERS` + `cve`) |
| `crates/**/*.rs` | the literal `docs/design/php-fastcgi.md` | none (a path is not wording) |

- `ATTACK_MARKERS = ("attack", "desync", "parameter trick", "smuggling")` is the
  one named constant, commented as ADR-0010 §1; `PLAN_ATTACK_MARKERS` extends it
  with `cve`. `test_the_vision_allowance_is_exactly_the_agreed_attack_markers`
  pins both, so widening the allowance is a deliberate, visible edit.
- One ordered alternation (`daemon|key|transport`) means the message names the
  spelling a reader would paste: `'route.fastcgi'`, not `'fastcgi'` inside it.
  A line that names the attack may keep bare `fastcgi`; it still fails on
  `php-fpm` and `route.fastcgi` (those groups are never exempted).
- Live briefs = every `plan/nodes/**/NODE.md` whose frontmatter `status` is
  **not** `done`/`superseded`, minus `plan/nodes/MB/MB.3/MB.3.30/NODE.md`
  (this brief defines the list). A brief with no `status` is scanned (fails
  closed). `LOG.md`/`RESULT.md` are never candidates.
- Not scanned: `docs/decisions/**`, `docs/design/**`, `docs/research/**`,
  `docs/testing/**`.
- Fallback offers: `fallback to` / `fall back to` / `as a fallback` whose
  sentence also names `php`/`fastcgi` fails. A sentence with a negation
  (`no`, `not`, `never`, `without`, `zero` before the fallback words) is the
  denial we want and passes — `docs/VISION.md` and `plan/nodes/MB/MB.3/NODE.md`
  ("no external PHP process and no fallback") were both real denials in the
  tree, and both pass.
- Every finding is `"<file>:<line>: '<word>': <detail>"` and the detail ends
  with `PHP_TRUTH = "embedded `libphp`, `[route.php]`; see ADR-0010"`.

## VISION line 57 (extra, minimal, reported on purpose)

The dispatch named lines 33 and 47. Line 57
(`block CGI/FastCGI/proxy header tricks`) is also a real line in the tree and
its only attack-class phrase was *"header tricks"*, which is not one of the four
agreed markers, so the guard as specified fails it. Rather than widen the
allowance (the failure mode this guard exists to stop), the tail of that one
bullet now reads `block CGI/FastCGI/proxy header smuggling.` — same meaning,
same voice, marker present. Nothing else in `docs/VISION.md` changed:

```diff
-- **PHP (embedded `libphp`):** … view workers — no `php-fpm`, no FastCGI surface — with …
+- **PHP (embedded `libphp`):** … view workers — no PHP daemon beside them and no CGI parameter surface — with …
-- **Embedded PHP** (…) zero-footgun defaults, no `php-fpm` daemon and no FastCGI parameter surface).
+- **Embedded PHP** (…) zero-footgun defaults, no daemon beside it and no CGI parameter surface).
-- **Strict HTTP header & protocol mechanics:** … and block CGI/FastCGI/proxy header tricks.
+- **Strict HTTP header & protocol mechanics:** … and block CGI/FastCGI/proxy header smuggling.
```

## Acceptance

### `make docs-unit`

```console
$ make docs-unit
UV_OFFLINE=1 uv run --no-project --with pytest==9.2.0 python -m pytest \
	python/tests/test_runnable_docs.py \
	python/tests/runnable_docs/test_harness.py \
	python/tests/test_docs_assets.py \
	python/tests/test_release_scripts.py -q
............................................................................    [100%]
76 passed, 65 subtests passed in 0.60s
```

The guard's own tests (17 collected):

```console
$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
.................                                                        [100%]
17 passed in 0.17s
```

### `make check`

```console
$ make check > /tmp/opencode/check-final.log 2>&1; echo "check rc=$?"; tail -4 …
check rc=0
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
63 files already formatted
…
default build: 0 pyo3 dependencies (no libpython linked)
uv run --no-project tools/plan.py validate
plan OK
```

`check` runs `guard`, `critical-guard` (7 changed files, no protected test
touched), `rs-fmt`, `rs-lint`, `rs-doc`, `rs-test`, `test-critical`, `py-lint`,
`docs-unit`, `docs-check`, `docs-test`, `rs-no-python`, `plan-validate`.

## Mutation proof

An attacker (or a copied old paragraph) puts the retired word on a scanned page:

```console
$ printf '\nBlastit runs php-fpm behind the ingress.\n' >> docs/frameworks/php.md
$ grep -n "php-fpm" docs/frameworks/php.md
152:Blastit runs php-fpm behind the ingress.
$ make docs-unit
...
E       AssertionError: docs/frameworks/php.md:152: 'php-fpm': not Blastit's PHP transport; use embedded `libphp`, `[route.php]`; see ADR-0010
python/tests/test_docs_assets.py:540: AssertionError
FAILED python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_php_vocabulary
1 failed, 75 passed, 65 subtests passed in 0.57s
make: *** [Makefile:231: docs-unit] Error 1
```

Restored, green again (file byte-identical to HEAD: `git diff --stat docs/frameworks/` empty):

```console
$ head -n -2 docs/frameworks/php.md > /tmp/opencode/php.md.clean && cp /tmp/opencode/php.md.clean docs/frameworks/php.md
$ grep -c "php-fpm" docs/frameworks/php.md
0
$ make docs-unit
76 passed, 65 subtests passed in 0.65s
```

## Both directions (crafted strings, `tmp_path` trees)

| Test | Proves |
|---|---|
| `test_a_product_page_may_not_teach_the_retired_php_transport` | `PHP-FPM`, `[route.FASTCGI]`, `php-fpm.conf`, `FastCGI` all fail with `file:line` + word + truth; the `libphp`/`[route.php]` line passes |
| `test_the_record_keeps_the_superseded_words` | `docs/decisions/`, `docs/design/`, `docs/research/`, `docs/testing/` hits produce **no** findings |
| `test_vision_may_name_the_attack_but_not_our_transport` | `FastCGI/CGI parameter trick` and `HTTP/FastCGI desync` lines pass; `runs php-fpm` and a bare `FastCGI surface` line fail |
| `test_the_vision_allowance_is_exactly_the_agreed_attack_markers` | the allowance cannot be widened silently |
| `test_a_live_brief_may_not_describe_our_php_as_fastcgi` | a `ready` brief fails on `route.fastcgi` and bare `FastCGI`; the `desync attack` and `CVE` lines pass |
| `test_a_finished_brief_is_history_not_a_claim` | `done` and `superseded` briefs carry the words and pass |
| `test_brief_records_are_never_scanned` | `LOG.md` / `RESULT.md` are not candidates |
| `test_a_brief_without_a_status_is_still_a_claim` | a missing status is scanned (fail closed) |
| `test_readme_is_a_product_page` | `README.md` is scanned |
| `test_no_rust_source_cites_the_deleted_design` | `crates/…/lib.rs:1` citing `docs/design/php-fastcgi.md` fails, naming the path |
| `test_a_php_fallback_is_refused_not_offered` | two denials (incl. `no fallback to`) pass; `as a fallback` and `fall back to` offers fail |
| `test_the_real_tree_carries_no_retired_php_vocabulary` | the real tree is clean, with every finding printed on failure |
| `test_the_guard_scans_a_real_tree` | anti-vacuity: product pages, Rust sources, a live/finished brief split, one ADR and one finished brief still carrying the retired words — so the exemptions have subjects |

## Notes / Follow-ups

- **Scope:** only `docs/VISION.md` and `python/tests/test_docs_assets.py` were
  edited. `git status` also lists `plan/JOURNAL.md`,
  `plan/nodes/MB/MB.3/{MB.3.30,MB.3.31,MB.3.32}/{LOG,NODE}.md` — those were
  already modified in this worktree before this attempt (plan tooling / owner
  reiteration); this node touched none of them, and mutated no plan status.
- **Gotcha worth `docs/knowledge/LESSONS.md`** (not in this node's `paths`, so
  recorded here instead): the attack-marker list must cover every attack phrase
  already present in `docs/VISION.md`. *"header tricks"* was not covered, so
  `docs/VISION.md:57` failed the new guard as written; the fix is a marker the
  list already knows (`smuggling`), never a wider allowance.
- `fastcgi` matching is intentionally spelling-based (`route.fastcgi` before
  bare `fastcgi`) so a manifest-key hit reads `'route.fastcgi'` in the gate
  output; a future `[route.php] sub-key` reusing the word would still fail on
  the daemon/key arms.
- Not committed, per dispatch.

---

# Attempt 2 (review fixes, 2026-11-14)

Review failed the fallback scan's core promise. Three fixes, all in
`python/tests/test_docs_assets.py`; VISION:33/:47/:57, the marker constants, the
real-tree assertions and all five scans are unchanged.

## [MAJOR] the fallback scan is paragraph- and sentence-based now

The old `sentence_around` cut a "sentence" at **any** `.` and never crossed a
line, so `Fall back to a daemon, e.g. for PHP routes.` and
`Use the daemon as a fallback (see docs/reference/manifest.md) for PHP.`
produced zero findings. Replaced by:

- `paragraphs(text) -> list[Paragraph]`: blank line, heading, list item, quote,
  table row or fence starts a paragraph; a *wrapped* line joins the paragraph
  above it. Each paragraph is flattened to one string plus `lines[i]` = the
  1-based source line of character `i`, so a sentence found in the flattened
  text still reports the real `file:line`.
- `sentence_bounds(text, start, end)`: splits on `(?<=[.!?])\s+` only, and
  `ends_sentence` refuses a boundary whose period closes an abbreviation
  (`ABBREVIATIONS = e.g, i.e, etc, vs, cf, approx, resp`) or a token carrying a
  slash (a path/URL: `docs/reference/manifest.md`, `manifest.md)`,
  `https://…`).
- `fallback_findings` iterates paragraphs, and the finding's line is the line of
  the **offer phrase** (`paragraph.lines[match.start()]`).

Both reviewer probes now fire, plus the three requested shapes and the denial
twin, each as its own test:

| Test | Shape |
|---|---|
| `test_a_fallback_offer_is_refused_not_offered` | `as a fallback` / `fall back to` one-liners (and the two denials untouched) |
| `test_a_fallback_offer_survives_an_abbreviation` | `Fall back to a daemon, e.g. for PHP routes.` |
| `test_a_fallback_offer_survives_a_path_in_between` | `… (see docs/reference/manifest.md) for PHP.` and a path followed by a wrapped clause |
| `test_a_fallback_offer_survives_a_wrapped_line` | offer on line 1, `PHP` only on line 2 — fails at `php.md:1` |
| `test_a_php_fallback_denial_passes` | `There is no fallback; embedded \`libphp\` is the only way to run PHP.` (+ two more denials) still pass |

## [MINOR] the transport-vs-attack rule is pinned

- `test_vision_may_name_the_attack_but_not_our_transport` now also carries
  `The attack exploited php-fpm's CVE.` → **fails** (`php-fpm`, line 5).
- `test_a_live_brief_may_not_describe_our_php_as_fastcgi` now also carries
  `The CVE list still covers \`[route.fastcgi]\`.` → **fails** (line 10).
- `test_the_vision_allowance_is_exactly_the_agreed_attack_markers` still pins the
  two tuples.

## [MINOR] the dead-citation scan has a crafted negative

`test_a_rust_source_may_cite_a_live_design`: a `.rs` citing
`docs/design/backend-runtimes.md` (live) passes; a `.rs` citing
`docs/design/php-fastcgi.md` fails at `crates/blastit-php/src/lib.rs:1`.

## Guard mutants (each one is killed by the new pins)

Temporarily mutated the guard itself, ran the file, restored with `cmp` proof:

```console
== A: exemption ignores lastgroup   ($F: if exempt and match.lastgroup == "transport": -> if exempt:)
FAILED …::test_vision_may_name_the_attack_but_not_our_transport
FAILED …::test_a_live_brief_may_not_describe_our_php_as_fastcgi
2 failed, 20 passed
== B: every terminator ends a sentence   (ends_sentence -> return True)
FAILED …::test_a_fallback_offer_survives_an_abbreviation
1 failed, 21 passed
== C: every line its own paragraph   (drop the BLOCK_START branch of paragraphs)
FAILED …::test_a_fallback_offer_survives_a_path_in_between
FAILED …::test_a_fallback_offer_survives_a_wrapped_line
2 failed, 20 passed
== D: naive split after any . ! ?   (SENTENCE_BOUNDARY -> (?<=[.!?]))
FAILED …::test_a_fallback_offer_survives_an_abbreviation
1 failed, 21 passed
RESTORED   (cmp -s against the snapshot)
```

## Real-tree mutation proof (an offer, wrapped across two lines)

```console
$ printf '\nFall back to a daemon, e.g. when the runtime is missing,\nfor every PHP route.\n' >> docs/frameworks/php.md
$ make docs-unit
E       AssertionError: docs/frameworks/php.md:152: 'Fall back to': a PHP fallback is refused, not offered; use embedded `libphp`, `[route.php]`; see ADR-0010
FAILED python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_php_vocabulary
1 failed, 80 passed, 65 subtests passed in 0.59s
make: *** [Makefile:231: docs-unit] Error 1
$ head -n -3 docs/frameworks/php.md > /tmp/opencode/php2.clean && cp /tmp/opencode/php2.clean docs/frameworks/php.md
$ git diff --stat docs/frameworks/          # empty
$ make docs-unit
81 passed, 65 subtests passed in 0.57s
```

The offer is on line 152 and the only `PHP` is on line 153: the message names
the line a reader must open, which is the defect the review found.

## Final gates (attempt 2)

```console
$ make docs-unit
............................................................................... [ 97%]
..                                                                       [100%]
81 passed, 65 subtests passed in 0.59s

$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
......................                                                   [100%]
22 passed in 0.19s

$ uvx --offline ruff check python/tests/test_docs_assets.py   -> All checks passed!
$ uvx --offline ruff format --check python/tests/test_docs_assets.py -> 1 file already formatted

$ make check > /tmp/opencode/check-attempt2.log 2>&1; echo rc=$?
rc=0
All checks passed!            (py-lint)
63 files already formatted
81 passed, 65 subtests passed (docs-unit)
plan OK                       (plan-validate)
```

Attempt-1 counts in the sections above (`17 collected`, `76 passed`) are
superseded by `22` and `81`. Nothing else changed: still two files in `paths`,
no commit, no plan-status mutation, and `docs/frameworks/php.md` is byte-clean.

---

# Attempt 3 (denial predicate, 2026-11-14)

One MAJOR left: `FALLBACK_DENIAL` treated any `no|not|never|without|zero`
earlier in the sentence as a denial, so three real offers passed silently.

## The rule, not the examples

```python
FALLBACK_DENIAL = re.compile(
    r"\b(?:no|never|zero)\b(?:\s+\S+){0,3}?\s+"
    r"(?:fallbacks?|alternatives?|degraded modes?|second ways?)\b",
    re.IGNORECASE,
)
FALLBACK_CONDITION = re.compile(
    r"\b(?:if|unless|otherwise)\b|\bwithout\b|\bwhen\b[^.!?]*\bnot\b"
    r"|\bnot\s+available\b|\bdo\s+not\s+stop\s+at\b|\bprefer",
    re.IGNORECASE,
)
```

- **(a) governing negation only**: `no`/`never`/`zero` must reach the fallback
  *noun* (`fallback`, `alternative`, `degraded mode`, `second way`) within 0-3
  words. Bare `not`/`without` are gone from the denial vocabulary — "libphp is
  not available" is a condition, not a promise that no alternative exists.
- **(b) a condition or preference disqualifies the denial**: `if`, `unless`,
  `without`, `when … not`, `not available`, `otherwise`, `do not stop at`,
  `prefer` in the same sentence turn it back into an offer.
  `fallback_findings` now needs `denial and not conditional` to pass a sentence.
- **(c)** the genuine denials still pass: "There is no fallback; embedded
  `libphp` is the only way to run PHP.", "…no fallback and no second way…",
  "Never a fallback: `[route.php]` is the only PHP route."

## Tests (23 in the file; every earlier test and scan untouched)

`test_a_php_fallback_denial_passes` keeps the three denial shapes plus
"Never a fallback, and no fallback to a daemon for PHP." (a denial that also
carries an offer phrase, so the predicate is exercised rather than bypassed).

`test_a_conditional_php_fallback_offer_is_still_an_offer` — the three review
probes, each must FAIL and name its line, plus two shapes that pin rule (a):

| line | text | expected |
|---|---|---|
| 1 | `If \`libphp\` is not available, fall back to a daemon for PHP.` | FAIL `php.md:1` `fall back to` |
| 2 | `Without \`libphp\`, fall back to a daemon for PHP routes.` | FAIL `php.md:2` `fall back to` |
| 3 | `Do not stop at PHP: use a daemon as a fallback.` | FAIL `php.md:3` `as a fallback` |
| 4 | `There is no fallback if \`libphp\` is missing, so fall back to …` | FAIL `php.md:4` `fall back to` (rule (b)) |
| 6 | `Not every host has \`libphp\`, so use a daemon as a fallback for PHP.` | FAIL `php.md:6` (bare `not`, rule (a)) |
| 7 | `There is no \`libphp\`, so fall back to a daemon for PHP.` | FAIL `php.md:7` (`no` does not govern a noun, rule (a)) |

## Guard mutants (each killed, `cmp`-verified restore)

```console
== E: loose denial (bare not/without count again)
FAILED …::test_a_conditional_php_fallback_offer_is_still_an_offer
1 failed, 22 passed
== F: denial ignores the conditional disqualifier  (if denial and not conditional -> if denial)
FAILED …::test_a_conditional_php_fallback_offer_is_still_an_offer
1 failed, 22 passed
== G: any condition disqualifies a denial  (if denial and not conditional -> if not conditional)
FAILED …::test_a_fallback_offer_is_refused_not_offered
FAILED …::test_a_fallback_offer_survives_an_abbreviation
FAILED …::test_a_fallback_offer_survives_a_path_in_between
FAILED …::test_a_conditional_php_fallback_offer_is_still_an_offer
4 failed, 19 passed
RESTORED   (cmp -s against the snapshot; 23 passed)
```

## Real-tree mutation proof (conditional offer, wrapped across two lines)

```console
$ printf '\nIf the runtime is not available, fall back to a daemon\nfor every PHP route.\n' >> docs/frameworks/php.md
$ make docs-unit
E       AssertionError: docs/frameworks/php.md:152: 'fall back to': a PHP fallback is refused, not offered; use embedded `libphp`, `[route.php]`; see ADR-0010
FAILED python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_php_vocabulary
1 failed, 81 passed, 65 subtests passed in 0.60s
make: *** [Makefile:231: docs-unit] Error 1
$ head -n -3 docs/frameworks/php.md > /tmp/opencode/php3.clean && cp /tmp/opencode/php3.clean docs/frameworks/php.md
$ git diff --stat docs/frameworks/          # empty
$ make docs-unit
82 passed, 65 subtests passed in 0.56s
```

## Final gates (attempt 3)

```console
$ make docs-unit
............................................................................... [ 96%]
...                                                                      [100%]
82 passed, 65 subtests passed in 0.57s

$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
.......................                                                  [100%]
23 passed in 0.19s

$ uvx --offline ruff check python/tests/test_docs_assets.py   -> All checks passed!
$ uvx --offline ruff format --check …                         -> 1 file already formatted

$ make check > /tmp/opencode/check-attempt3.log 2>&1; echo rc=$?
rc=0
guard: OK (1 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 7 changed file(s)
All checks passed!            (py-lint)
63 files already formatted
82 passed, 65 subtests passed (docs-unit)
plan OK                       (plan-validate)
```

Still two files in `paths`, nothing committed, no plan status touched, and
`docs/frameworks/php.md` is byte-clean after the mutation proof.

---

# Attempt 4 (clause-scoped offer rule, 2026-11-14)

The fallback-offer logic was replaced by the one specified rule. No exception
list, no conditional marker that "excuses" a clause.

## A-E, as specified

```python
FALLBACK_TERM = re.compile(r"\bfalls?\s+back\b|\bfallback\b", re.IGNORECASE)   # B
PHP_CLAUSE    = re.compile(r"php|fastcgi|daemon|libphp|route", re.IGNORECASE)   # C
DENIAL_NOUN   = re.compile(                                                    # D1
    r"\b(?:no|never|zero)\b(?:\s+\S+){0,3}?\s+"
    r"(?:fallbacks?|alternatives?|degraded modes?|second ways?)\b", re.IGNORECASE)
DENIAL_VERB   = re.compile(                                                    # D2
    r"\b(?:never|do\s+not|don't|must\s+not)\b(?:\s+\S+){0,2}?\s+"
    r"(?:run|use|serve|add|fall\s+back\s+to)\b(?:\s+\S+){0,4}?\s+fallback\b",
    re.IGNORECASE)
CLAUSE_BREAK  = re.compile(                                                    # A
    r";|:|\s*(?:—|--)\s*|,\s*but\b|,\s*though\b|,\s*however\b|,\s*and\s+yet\b",
    re.IGNORECASE)
```

- **A** `paragraphs()` (flattening + `e.g.`/path full-stop handling kept) →
  `sentences()` → `clauses()`; every check runs per clause, and the finding's
  line is `paragraph.lines[clause_left + offer.start()]`, i.e. the real
  `file:line` of the offer.
- **B/C/D/E** `fallback_findings`: one finding per clause that carries an offer
  term, names PHP, and matches neither D1 nor D2. Bare `not`/`without` are not
  denials; nothing is inherited across a clause break.
- `sentence_bounds`/`FALLBACK_OFFER`/`FALLBACK_DENIAL`/`FALLBACK_CONDITION`/
  `PHP_OR_TRANSPORT` are gone.

## One structural choice, reported

A **fenced code block is not prose** (`Paragraph.prose == False`): a code block
has no sentences, and a route *named* `"fallback"` in a `blastit.toml` example
(`docs/frameworks/django.md:70`, `flask.md:61`) would otherwise read as an
offer. The vocabulary scan is unchanged and still covers every line of a fence,
so a `php-fpm`/`route.fastcgi` mention inside one still fails. Mutant L below
shows the flag is load-bearing on the real tree.

## Prose touched (rule kept strict, every page listed)

The rule flagged four live sentences that were not offers. Their prose now uses
an approved shape or a different noun; no regex was widened.

1. `docs/reference/manifest.md:314` — "so a catch-all fallback cannot reach the
   database" → "so a catch-all route cannot reach the database" (not about PHP).
2. `plan/nodes/MB/MB.3/MB.3.20/NODE.md:3` (title) — "php.md fence and fallback
   note" → "php.md fence and its removed setting's note".
3. `plan/nodes/MB/MB.3/MB.3.20/NODE.md:58` — "the retired setting and its
   fallback note are gone" → "…and its note are gone".
4. `plan/nodes/MB/MB.3/MB.3.20/NODE.md:73` — "Do not re-add a fallback" →
   **"Never add a fallback"**, which is D2 exactly (`never` + `add` + `fallback`).

`docs/VISION.md` (lines 33/47/57) is unchanged from attempt 1.

## Tests — every shape from the brief, with its line

`test_every_other_php_fallback_offer_fails` (MUST FAIL):

| line | text | finding |
|---|---|---|
| 1 | `If \`libphp\` is not available, fall back to a daemon for PHP.` | :1 `fall back` |
| 2 | `Without \`libphp\`, fall back to a daemon for PHP routes.` | :2 `fall back` |
| 3 | `Do not stop at PHP: use a daemon as a fallback.` | :3 `fallback` |
| 4 | `There is no fallback — you may fall back to a daemon for PHP.` | :4 `fall back` |
| 5 | `No fallback exists; use a daemon as a fallback for PHP.` | :5 `fallback` |
| 6 | `There is no fallback, but you may use a daemon as a fallback for PHP.` | :6 `fallback` |
| 7 | `There is no fallback: fall back to a daemon for PHP routes.` | :7 `fall back` |
| 8 | `The fallback for PHP routes is a daemon.` | :8 `fallback` |
| 9 | `PHP's fallback is a daemon.` | :9 `fallback` |
| 10 | `PHP does not have a fallback when the daemon is missing.` | :10 `fallback` (bare `not` is not D1) |

plus `test_a_fallback_offer_is_refused_not_offered` (verb + noun),
`test_a_fallback_offer_survives_an_abbreviation` (`e.g.`),
`test_a_fallback_offer_survives_a_path_in_between` (`(see …manifest.md)`),
`test_a_fallback_offer_survives_a_wrapped_line` (offer on the second physical
line → `php.md:2`), and `test_a_dotted_path_or_abbreviation_is_not_a_full_stop`.

`test_a_php_fallback_denial_passes` (MUST PASS): "There is no fallback; embedded
`libphp` is the only way to run PHP." | "A PHP route has no fallback and no
second way." | "Never a fallback: `[route.php]` is the only PHP route." | "Never
run a daemon as a fallback for PHP routes." (D2) | "Do not use a daemon as a
fallback for PHP." (D2) | "This page does not mention PHP."

## Rule mutants (each killed, `cmp`-verified restore)

```console
== H: relevance drops daemon/libphp/route      -> FAILED test_every_other_php_fallback_offer_fails
== I: clauses split only on ';'                -> FAILED that test + the real-tree test
== J: D1 accepts bare not/without              -> FAILED test_every_other_php_fallback_offer_fails
== K: D2 dropped                               -> FAILED test_a_php_fallback_denial_passes
== L: fenced code scanned as prose             -> FAILED test_the_real_tree_carries_no_retired_php_vocabulary
RESTORED (24 passed)
```

## Real-tree mutation proof (denial in one clause, offer in the next)

```console
$ printf '\nThere is no fallback: fall back to a daemon for PHP routes.\n' >> docs/frameworks/php.md
$ make docs-unit
E       AssertionError: docs/frameworks/php.md:152: 'fall back': a PHP fallback is refused, not offered; use embedded `libphp`, `[route.php]`; see ADR-0010
FAILED python/tests/test_docs_assets.py::test_the_real_tree_carries_no_retired_php_vocabulary
1 failed, 82 passed, 65 subtests passed in 0.63s
make: *** [Makefile:231: docs-unit] Error 1
$ head -n -2 docs/frameworks/php.md > /tmp/opencode/php4.clean && cp /tmp/opencode/php4.clean docs/frameworks/php.md
$ git diff --stat docs/frameworks/          # empty
$ make docs-unit
83 passed, 65 subtests passed in 0.61s
```

## Final gates (attempt 4)

```console
$ make docs-unit
............................................................................... [ 95%]
....                                                                     [100%]
83 passed, 65 subtests passed in 0.61s

$ uv run --no-project --with pytest==9.2.0 python -m pytest python/tests/test_docs_assets.py -q
........................                                                 [100%]
24 passed in 0.22s

$ uvx --offline ruff check python/tests/test_docs_assets.py   -> All checks passed!
$ uvx --offline ruff format --check …                         -> 1 file already formatted

$ make check > /tmp/opencode/check-attempt4.log 2>&1; echo rc=$?
rc=0
guard: OK (1 staged/untracked file(s) checked)
critical-guard: HEAD..working tree, 9 changed file(s)
All checks passed!            (py-lint)
63 files already formatted
83 passed, 65 subtests passed (docs-unit)
plan OK                       (plan-validate)
```

Nothing committed; no plan status or managed frontmatter field changed.

---

# Review MINORs (2026-11-14)

All three in `python/tests/test_docs_assets.py`; D1 unchanged, real tree green,
24 tests in the file.

1. **`CLAUSE_BREAK` missed `, and` / `, so`** → added
   `,\s*(?:and|so|then|therefore)\b` (after `,\s*and\s+yet\b`, every existing
   break kept); pinned `There is no fallback, and you may use a daemon as a
   fallback for PHP.` (MUST FAIL, `php.md:11`) and the `, so` twin
   `There is no fallback, so use a daemon as a fallback for PHP routes.`
   (`php.md:12`). Mutant N (break removed) is killed by that test.
2. **`FALLBACK_TERM` had no plural** → `\bfalls?\s+back\b|\bfallbacks?\b`;
   pinned `PHP fallbacks are a daemon.` (MUST FAIL, `php.md:13`, word
   `fallbacks`). Mutant O (singular only) is killed by that test.
3. **`DENIAL_VERB` rejected "never falls back to"** → added
   `\b(?:never|do\s+not|don't|must\s+not)\s+falls?\s+back\s+to\b` to the D2
   alternation; pinned `PHP routes never fall back to a daemon.` (MUST PASS).
   Mutant M (that alternative removed) is killed by the denial test.

Interaction worth knowing: with the plural term live, the `, and` break is what
keeps `plan/nodes/MB/NODE.md:34` green — its clause
`Static asset serving + URL rewriting handle SPA fallbacks, hashed immutable
assets,` carries `fallbacks` but no PHP word, and the following clause starts at
`, and PHP`. Removing the break re-flags that epic brief (mutant N), so the two
MINORs are load-bearing together.

Gates after the fix:

```console
$ make docs-unit
83 passed, 65 subtests passed in 0.64s
$ make check > /tmp/opencode/check-review.log 2>&1; echo rc=$?
rc=0
guard: OK (1 staged/untracked file(s) checked)
All checks passed!            (py-lint, 63 files already formatted)
83 passed, 65 subtests passed (docs-unit)
plan OK                       (plan-validate)
```

No new prose edits (the lead's widened `paths` needed none), nothing committed,
no plan status changed.
