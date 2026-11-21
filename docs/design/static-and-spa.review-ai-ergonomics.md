# AI-ergonomics review — docs/frameworks/static-and-spa.md

Lens: ai-ergonomics. Re-reviewed at node MB.2.1 against the current page and inventory.

## Findings

### A1. The canonical manifest omits `app` and never says when that is legal [BLOCKER]
Where: lines 6–19.
What: the block had no `app` while the reference requires it.
Resolution: fixed — line 19 states "An all-static manifest needs no `app`: every route is a `[route.static]`", and the fallback rows name "the manifest's `app`".

### A2. "GET and HEAD only; a body is 413" has no scope and kills the front controller [BLOCKER]
Where: lines 83–86.
What: the rule read as applying to the whole route, so a PHP/Node POST could never reach the app.
Resolution: fixed — the responder answers `GET`/`HEAD` on paths it would answer (other method `405` + `Allow: GET, HEAD`; "a `GET` or `HEAD` with a body is `413`"); a path it does not answer reaches the app with the route's `methods`/`[route.ingress]`.

### A3. `blastit check` cannot pass on the page's own static site [BLOCKER]
Where: lines 95–108; fixture `python/tests/runnable_docs/fixtures/static-site/`.
What: the page ran a pytest-based check on an app-less, suite-less project, so exit was 2/3.
Resolution: fixed — the all-static path runs no pytest, proves every `root`, exits `0`, and a missing root is `2`; fence 5's reason names MB.2.6's path.

### A4. The static finding's `kind` and the no-suite JSON shape are still undefined [MAJOR]
Where: lines 105–108.
What: "a root the suite never requested is a warning" needed an invented `kind` and `fix`.
Resolution: fixed — the sentence and the vocabulary are gone; the page keeps only `unrouted_request` ("a path no route matched is a finding") and defers the enum and exit codes to ci.md, so no key, severity name or `fix` shape is invented.

### A5. `[route.static]` has no key table: `root` and `index` are only prose [MAJOR]
Where: lines 13–26, 66–81.
What: `index` had no default or scope and the table said `index.html`.
Resolution: fixed — `root` (required, relative), `index` (default `index.html`, both directory and fallback), `spa` (`false`) and `immutable` (none) are defined, and rows 72–74 use `index`.

### A6. `immutable` pattern language is unspecified, and `*` is not a regex [MAJOR]
Where: lines 26, 79–81.
What: match target, anchoring, `*` semantics and the default were unstated.
Resolution: fixed — globs on the answering file's path, `*` stops at `/`, omitted means none, non-matches are `no-cache`.

### A7. The precedence table is not total: directories, missing files, per-row status/cache [MAJOR]
Where: lines 70–81.
What: directory slash handling, a directory without `index`, a missing fallback, and per-row status/cache were absent.
Resolution: fixed — five ordered rows with statuses, `/docs` and `/docs/` both `200`, fall-through for a directory without `index`, missing fallback `404`, and the cache rule in lines 79–81.

### A8. The fallback target is two different things, and "the route's app" is not a key [MAJOR]
Where: lines 25, 74–76, 112–116.
What: `spa` and the app competed, and "the route's app" named no key.
Resolution: fixed — `spa` answers "instead of the app", the ordered rows give it precedence, the text says "the manifest's `app`", and line 112 states "A front-controller route omits `spa`".

### A9. "The app's own entry script is not served" has no input that identifies it [MAJOR]
Where: lines 118–119.
What: no key named an entry script, so the 0.2.0 responder could not implement the exclusion.
Resolution: fixed — the sentence moved into 0.2.1 ("the FastCGI worker that runs `public/index.php`, and refuses to serve it as a file, is `0.2.1`"), so 0.2.0 promises nothing it cannot identify.

### A10. Refusal status is a coin flip, the threat list does not exist, and `.well-known` is caught [MAJOR]
Where: lines 88–93.
What: "`400` or `404`" gave two statuses for several classes and the refused path's owner was unstated.
Resolution: fixed — one status per class (`400` for traversal/NUL/control/absolute/backslash, `404` for a symlink escape or a dotfile so neither reveals existence), `/.well-known/` public, and "A refusal is final: it is never the app and never `index.html`". The symlink status now matches the `MB.2.7` e2e matrix (`public/link -> /etc/passwd` is `404`). The outbound links are still empty: A16.

### A11. The front-controller prose asserts 0.2.0 behaviour the page then says is 0.2.1 [MAJOR]
Where: lines 112–119.
What: the `PATH_INFO` sentence was present tense before the 0.2.1 caveat.
Resolution: fixed — "`0.2.0` ships the rewrite and this responder; the FastCGI worker … is `0.2.1`".

### A12. `HEAD`, `405` and `Content-Type` details a driver needs are half-shown [MINOR]
Where: lines 50–64, 83–86.
What: `HEAD`, the `Allow` header and per-extension headers were unspecified.
Resolution: fixed — `405` carries `Allow: GET, HEAD`, "`HEAD` is `GET` without a body", and the two shown responses pin `text/javascript` and `text/html; charset=utf-8`.

### A13. Fence 2's reason claims a check its mode does not perform [MINOR]
Where: inventory fence 2 reason; page lines 6–17.
What: it claimed to check the manifest shape, which no mode does.
Resolution: fixed — the reason now says "checks the documented dev command and its flags, not a boot" and names MB.2.3/MB.2.7 for the shape.

### A14. The new all-static `check` contract contradicts `docs/ci.md` and `docs/reference/cli.md` [MAJOR]
Where: lines 97–99, 106–108 vs `docs/ci.md:9,96,108` and `docs/reference/cli.md:47`.
What: the page's app-less, suite-less exit `0` contradicted ci.md's "runs `pytest -q`", "without `pytest` it exits `2`" and "needs `version`, `app`, `[proxy.db]`".
Resolution: fixed — the page states its own exception ("check proves every `root` exists instead of running pytest, and exits `0`") and confines ci.md to "Every manifest with an `app`"; `RESULT.md` Follow-up 1 hands the ci.md/`reference/cli.md` reconciliation to MB.2.6, so the page itself is unambiguous.

### A15. The year-long cache keyed on the request path cached the SPA fallback HTML [MAJOR]
Where: lines 26, 74, 79–81.
What: line 26 said request path while line 79 said file, so `/assets/typo.js` answered `index.html` with a year-long cache under one reading.
Resolution: fixed — `immutable` is "globs on the path of the file that answers", only "a file from `root` whose path matches" gets the year-long cache, and "every other answer, `index` included, is `no-cache`".

### A16. "Mechanism in How it works, threat list in the security model" points at pages with no static content [MINOR]
Where: lines 92–93.
What: neither `docs/security.md` nor `docs/how-it-works.md` mentions static serving, refusals or dotfiles, and MB.2.8's paths touch only `ARCHITECTURE.md`, so no node fills them; an agent following the pointer finds nothing.
Resolution: open (accepted) — `RESULT.md` Follow-up 3 defers the two sections to MB.2.8, whose paths do not include `docs/security.md` or `docs/how-it-works.md`, so the pointer still needs an owner; until then it promises more than it delivers. The page itself is otherwise complete.

### A17. The front-controller section misstates why `check` is clean [MINOR]
Where: lines 119–120.
What: it said `spa` made the check clean, which implied check drives the responder.
Resolution: fixed — "The block above has no app, so `spa` answers every missing path and `check` proves only its root".

### A18. `immutable`'s match input still lacks its canonical form [MINOR]
Where: lines 26, 79.
What: "the path of the file that answers" does not say whether it is the URL path with a leading `/` (what the pattern `/assets/*` implies) or the path relative to `root` (`assets/app.js`, which that glob cannot match). Choosing the root-relative form fails the page's own asset example and fence 3.
Resolution: fixed — line 26 now reads "globs matched whole against the answering file's path (`/…`)", which pins the leading slash, the full match and `*` stopping at `/`.

### A19. An all-static project that does have a suite is undefined [MINOR]
Where: lines 97–99, 106.
What: the page asserts an all-static manifest "has no app and no suite" and that "every other manifest runs the suite". A static site with its own pytest suite is still all-static (no app), so it is unclear whether `check` runs it (and can exit `3`) or proves roots and exits `0` while skipping it.
Resolution: fixed — line 97 now reads "An all-static manifest has no app: `check` proves every `root` exists instead of running pytest" and line 99 "Every manifest with an `app` runs the suite", so the discriminator is the manifest, not the presence of a suite.

### A20. Fence 6's reason promises an MB.2.7 replay with the 0.2.1 PHP worker [MINOR]
Where: inventory `[[page.fence]] index = 6`; page lines 118–124.
What: MB.2.7 runs inside 0.2.0, before the FastCGI worker exists, yet the reason says MB.2.7 "replays it with the PHP worker once that backend lands" — a proof the named node cannot perform; an agent trusting it plans a PHP e2e in the wrong milestone.
Resolution: fixed — the fence 6 reason now says "MB.2.7 replays the check command on the static fixture, and 0.2.1's e2e adds the PHP worker replay", so the named proof matches the milestone that owns it.

## Verdict
Every BLOCKER and MAJOR is closed; A14's cross-doc follow-up, A18, A19 and A20 are fixed on disk, leaving only A16 (the security/how-it-works pointers, accepted as RESULT.md Follow-up 3) and no contract an implementer must invent.
VERDICT: PASS
