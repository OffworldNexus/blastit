# DX review — docs/frameworks/php.md

Lens: dx. Re-reviewed at node MB.3.2, revision 3 (resolution pass).

## Findings

### B1. Deployment named a preset but never a pool, and a 0600 socket could not be reached cross-uid [BLOCKER]
Where: `## Deployments` 140–150; pool 46–56.
Resolution: fixed — line 146 now states the production ownership model: "one pool
per route runs as that route's uid, so `0600` stays private". The image/entrypoint
owns the pool; Local (line 38) runs it from the project root, and the preset owns
the Blastit side. The claim that the local block is the deploy block verbatim is
gone; the preset command and the pool sentence are the deploy story.

### B2. `check -- <suite>` and `BLASTIT_URL` contradicted the shipped CLI/CI references [BLOCKER]
Where: `## CI/CD` 110–138; `docs/reference/cli.md`, `docs/ci.md`.
Resolution: fixed on the page — line 3 marks the whole branch `0.2.1`; lines 120–122
state "for a FastCGI manifest that is the suite, not pytest" and `BLASTIT_URL`.
The two shipped pages still describe the WSGI/pytest branch and are outside this
node's paths; `RESULT.md` Follow-up 1 hands their FastCGI paragraph to the
reference/CI owner. The page no longer leaves an agent to guess between the two.

### B3. CI was not reproducible on a clean checkout [MAJOR]
Where: `## CI/CD` 112–113.
Resolution: fixed — "Your CI image has PHP and Composer; start the pool as in
Local" names the two preconditions the old text hid. `run/` and the pool command
are in Local; the audit command follows.

### B4. The named Symfony/Laravel suite is in-process, so `check` would see nothing [MAJOR]
Where: 122–124.
Resolution: fixed — the page now gives the working shape: a test that does
`file_get_contents(getenv('BLASTIT_URL').'/users/42')` speaks HTTP, and an in-process
test sends nothing. `vendor/bin/phpunit` stays as the runner, but the test it must run
is explicit.

### B5. The `--learn` step was not executable and showed nothing it wrote [MAJOR]
Where: 103–110.
Resolution: fixed — the reader stops the first `dev`, removes `[route.static]`, starts
the learning server, requests the concrete `/assets/app.4f1c2a9b7d3e.css` (learn
returns the front controller's `404`), and stops it with Ctrl-C; the page names the
exact line learned (`[route.static] root = "public"`) and leaves `immutable` to the
reader. No fabricated `learn:` count.

### B6. The security/how-it-works links had no FastCGI content [MAJOR]
Where: 100–101.
Resolution: accepted, owned elsewhere — the node's law requires pointing at
`security.md`/`how-it-works.md`; those sections are not in this node's paths. The
promise is proven by `MB.3.7`'s `e2e/php2shell.py`, and `RESULT.md` Follow-up 2
names the mechanism-page owner. The page does not restate kernel mechanics.

### B7. No proxy/env story for a DB-backed Symfony/Laravel app [MAJOR]
Where: block 6–21; pool 46–56; CI 115–117.
Resolution: accepted by scope — this page is the FastCGI HTTP route; `[proxy.db]`,
`[route.db]` and the pool's `env[...]` delivery are the manifest/design contract
(`RESULT.md` Follow-up 3, MB.3.3). The `statements: 0, proxies: {}` example is the
no-DB fixture, not a claim that a DB-backed app needs no grant.

### B8. The `entry` row read as `public/public/index.php`; the `app` sentence was imprecise [MINOR]
Where: 24–29.
Resolution: fixed — `entry` is "relative to `blastit.toml` (under `root` when both
are set)", so `public/index.php` is right in both the complete and the
fastcgi-only state; "no `app`" is stated as the FastCGI executor rule.

### B9. `spa = true` is a silent trap [MINOR]
Where: 12–24.
Resolution: accepted — the example omits `spa`; `static-and-spa.md` owns the
front-controller/`spa` rule, and the page already says a file is answered before
the front controller. A reader adding `spa` departs from the shown block.

### B10. "Fingerprinted ones are cached" was false for `/assets/*` [MINOR]
Where: 81–82.
Resolution: fixed — the page now says "Point `immutable` at fingerprinted output only:
`public/assets/` is the directory your bundler fills with hashed names — never a path
that can also answer HTML or an unhashed original". The shipped `/assets/*` example
stays; the rule is a usage contract, not a change to the responder.

### B11. A 0.2.0 reader gets `unknown key` with no explanation [MINOR]
Where: 3; 148–149.
Resolution: accepted — line 3 states `0.2.1` adds the block; the release boundary
is explicit. The reference entry is MB.3.5's, and the table above is the contract
until it lands.

### B12. The foreground pool implies a second terminal [MINOR]
Where: 58–64.
Resolution: accepted — "`-F` keeps it in the foreground" states why the next
command needs another terminal, and the `--learn` step says Ctrl-C. Adding a
"terminal 1/2" line would exceed the 150-line budget for a MINOR.

## Verdict

Every BLOCKER and MAJOR is resolved on the page: the deploy ownership model is
stated, the CI/`--`/`BLASTIT_URL` branch is pinned to 0.2.1 and made explicit, the
clean-checkout preconditions and the HTTP-test shape are shown, and `--learn`
names the exact write. B6 and B7 are not page defects: one is the node-mandated
pointer to mechanism pages owned elsewhere and proven by the e2e; the other is the
proxy page's contract, outside this page's HTTP scope. The three remaining MINORs
are accepted with reasons. The page is copy-pasteable in Local and honest about
0.2.1 versus 0.2.0.
VERDICT: PASS
