---
id: MB.6
title: "Static files beside a Python/PHP app + the hashed-asset caching contract"
kind: task
status: ready
owner: "doc-writer"
priority: 1
depends_on: [MB.2]
paths: ["docs/frameworks/**", docs/reference/manifest.md]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-14T00:58"
updated: "2026-11-14T00:58"
---

# Goal

Make the static-asset story land where the owner actually wanted it: **static files served beside a normal Python (Django/Flask) or PHP (Symfony/Laravel `public/`) app**, not only in front of a built SPA. Document the caching contract precisely, including when a year-long `immutable` cache is safe because the filenames are content-hashed, and when it is a footgun.

# Context

0.2.0 shipped the mechanism (`[static]` with `root`/`index`/`immutable`, `openat2` resolution, ETag/304, `immutable` globs → `Cache-Control: public, max-age=31536000, immutable`, everything else `no-cache`, and the rule keyed on the **answering file's path** so an SPA deep URL answered from `index.html` stays `no-cache`). What the page does not do is speak to the common case: a Django app with `collectstatic` output, or a Laravel/Symfony app with `public/` assets, where the dynamic routes still go to the app.

That framing gap is the defect. The proof that it matters: the owner read the shipped page as "static files for a built front end", when their intent was "every backend has static files to serve".

Also to pin down, because it is the usual production bug:
- **Hashed names are the licence for `immutable`.** Vite emits `assets/<name>-<hash>.js`; Django's `ManifestStaticFilesStorage` emits `app.<hash>.css`. Only a name that changes when the bytes change may be cached for a year.
- **Never glob a path that can also answer HTML or an unhashed original.** `*` stops at `/`, and `index` answers are always `no-cache`; still, `immutable = ["/static/*"]` on a Django `STATIC_ROOT` would mark unhashed originals immutable and pin a stale copy in every browser for a year. The page must say this in one unambiguous line, with the correct shape (`/static/<hashed-dir>/*`, or keep hashed output in its own directory).
- **No auto-detection of hashes.** The rule is declared, not guessed: guessing from a filename pattern would silently cache a page for a year when the guess is wrong. Say so, and say what to do instead.

# Acceptance

1. `docs/frameworks/static-and-spa.md` gains a short section **"Static files next to your app"** with three lean, runnable blocks:
   - **Django / Flask:** `collectstatic` output (`STATIC_ROOT`) served by `[static]`, dynamic routes still reaching the WSGI app; show the `blastit.toml` lines and the one command;
   - **PHP (Symfony/Laravel):** `public/` served by `[static]` with the front controller as the app route;
   - **Node/SPA:** the existing case, kept but no longer presented as the only one.
   Each block states where the files come from, the `root`, and the glob for hashed output.
2. A **"Caching"** subsection stating the contract exactly as implemented: `immutable` globs → the year-long header; everything else, `index` included, `no-cache`; ETag/304 on both; the matching is on the answering file's path. Include the hashed-name licence rule and the `STATIC_ROOT`/unhashed-original warning above, with the wrong and right glob side by side.
3. `docs/reference/manifest.md` matches word for word (it currently documents `immutable` correctly but without the hashed-name guidance).
4. Everything stays honest and runnable: fences registered and green (`make docs-test`, `make docs-check`), pages still <= 150 lines each, and the `dx` + `ai-ergonomics` critics pass. A Django or PHP snippet that nobody can run is a defect, not a doc.
5. If the shipped behaviour cannot express the correct Django/PHP shape (e.g. no way to scope `immutable` to a hashed subdirectory because of how `root`/glob interact), say so and open the follow-up implementation node rather than documenting a lie.

# Notes

- This is the "I said static files for PHP/Python" correction: keep it documentation-led, and only open code work if the mechanism genuinely cannot express the common case.
- Do not re-litigate 0.2.0's mechanism; it is shipped, gated and in its release PR. This node adjusts the framing and the caching guidance.
- Keep the kernel/file-descriptor mechanics out of the user page; they belong in `ARCHITECTURE.md` / `docs/security.md` if they are needed at all.
