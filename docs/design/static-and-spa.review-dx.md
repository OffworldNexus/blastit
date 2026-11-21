# DX review — docs/frameworks/static-and-spa.md

Lens: dx. Reviewed at node MB.2.1, attempt 1. Re-judged against the second revision.

## Findings

### B1. `blastit check` cannot succeed for this page's own project [BLOCKER]
Where: `## CI/CD`, lines 97–100.
Resolution: fixed — "An all-static manifest has no app: `check` audits every `root`
(exists, a directory, readable) instead of running pytest, and exits `0`. A root
that is missing or not a directory stops it before anything runs (`2`)". Re-quoted
and re-confirmed against the final revision, where MB.2.6 now owns that path.

### B2. Vite's build output never becomes `public/` [MAJOR]
Where: `## Run it`, line 32.
Resolution: fixed — `npx vite build --outDir public` is now its own `console`
fence, registered `example` with a written reason (nothing runs npm here; MB.2.7
builds the fixture and proves the output lands under `root`), one canonical target.

### B3. The opening flow is out of order and hides an install branch [MAJOR]
Where: `## Run it`, lines 28–43.
Resolution: fixed — build target, then `uv init --app`/`uv add blastit` under "In
a project with no `pyproject.toml` yet", then `uv run blastit dev` and its banner.

### B4. The PHP rewrite is stated in the present tense, the 0.2.1 caveat comes last [MAJOR]
Where: `## Front controller`, lines 110–120.
Resolution: fixed — the rewrite is the manifest's `app`; the PHP entry script and
its worker are named as 0.2.1, "documented and not served" until then.

### B5. Real output is never shown for the second `check` [MINOR]
Where: lines 122–124.
Resolution: fixed — the fence stays because NODE requires it, and the prose now
states exactly what it proves ("`check` proves only its root").

### B6. Exit codes are re-taught from `ci.md` [MINOR]
Where: lines 106–108.
Resolution: fixed — one link, only the two cases this page uses named inline.

### B7. `--json` is shown for nobody on this page [MINOR]
Where: lines 101–107.
Resolution: fixed — one clause says what it is ("the same verdict as one document,
for an agent"), and CI/CD owns the contract.

### B8. `app` is absent, and the page never says that is deliberate [MINOR]
Where: line 19.
Resolution: fixed — "An all-static manifest needs no `app`".

### B9. The deploy fence looks runnable locally [MINOR]
Where: lines 126–136.
Resolution: fixed — "In the container image", then "Locally that exits `1` on
purpose".

### B10. "no regex is involved" contradicts the manifest [MINOR]
Where: line 115.
Resolution: fixed — "the manifest keeps one `.*`".

### B11. A front controller must turn `spa` off, and the page never says so [MAJOR]
Where: lines 112–113, against the opening block (`spa = true`).
Resolution: fixed — "A front-controller route omits `spa`: with `spa = true` the
fallback answers first, so the app is never reached." The trap is now the first
sentence of the section.

### B12. Bare `vite` may not be on the reader's `PATH` [MINOR]
Where: line 30.
Resolution: fixed — `npx vite build --outDir public`.

### B13. The all-static exemption is contradicted by the pages it links [MINOR]
Where: lines 97–99; `docs/ci.md` "Your suite", `reference/cli.md` `check`.
Resolution: fixed here — the page states the exception in its own words and
delegates everything else; the `ci.md`/`cli.md` wording is a tracked follow-up
outside this node's paths (MINOR, does not block).

### B14. The fallback table's last row is unreachable as written [MINOR]
Where: lines 70–76.
Resolution: fixed — "no file, no `spa`, no `app` → `404`".

## Verdict

All ten original findings and the four revision findings are resolved except one
tracked MINOR on pages this node may not touch; the page is ordered, lean (141
lines), copy-pasteable, and honest about 0.2.0 versus 0.2.1.
VERDICT: PASS
