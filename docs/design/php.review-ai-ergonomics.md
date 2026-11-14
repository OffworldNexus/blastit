# AI-ergonomics review — docs/frameworks/php.md

Lens: ai-ergonomics. Re-reviewed at node MB.3.2, revision 3 (resolution pass).

## Findings

### A1. The `entry` invariant was absolute, but the `--learn` state has no `root` [BLOCKER]
Where: table 29; `--learn` 103–104; block 12–19.
Resolution: fixed — `entry` is now "relative to `blastit.toml` (under `root` when
both are set)". The complete block and the fastcgi-only state the `--learn` step
prescribes both validate; `SCRIPT_FILENAME` is pinned to the manifest-relative
entry.

### A2. The canonical block contradicted the manifest reference's no-app rules [MAJOR]
Where: 24–25; `docs/reference/manifest.md` 13, 20–22, 135, 142–145.
Resolution: fixed on the page — line 25 states the FastCGI exemption: "A FastCGI
route is its own executor: no `app`, and `[route.ingress]` applies to it." The
reference entry (and the static-fallback/ingress exemption) is MB.3.5's; the page
is the input of record and `RESULT.md` Follow-up 1 names that owner.

### A3. `check`'s PHP branch was unreachable under `ci.md`/`cli.md` [MAJOR]
Where: 3, 120–122; `docs/ci.md` 9–18; `reference/cli.md` 47, 51.
Resolution: fixed on the page — line 3 marks `check -- <suite>` as 0.2.1; lines
120–122 state the FastCGI branch ("that is the suite, not pytest"). `ci.md` and
`reference/cli.md` are outside this node's paths; Follow-up 1 hands them the
third branch. An agent reading this page no longer has to guess.

### A4. `BLASTIT_URL` was defined but its reader was never named [MAJOR]
Where: 120–124.
Resolution: fixed — "The suite reads `BLASTIT_URL`, never the app" plus the
`getenv('BLASTIT_URL')` test call. The suite is the reader; the app cannot see a
`BLASTIT_*` variable, so the page no longer invites an agent to put it in app
config.

### A5. `--learn` was shown learning only a static root, with no stop [MAJOR]
Where: 103–110.
Resolution: fixed — the first `dev` is stopped, the learning server is started
explicitly, the asset request returns the front controller's `404` and records the
path, and Ctrl-C writes `[route.static] root = "public"`; `immutable` is explicitly
the reader's. Route and query learning is `local.md`'s contract (line 99–105), not
this page's; the static write is what the node asks this page to show.

### A6. The ingress rule was POST-only and omitted `raw` [MAJOR]
Where: 33–34.
Resolution: fixed — "A request with a body (POST, PUT, PATCH) needs
`[route.ingress]` (`form` or `json`; `raw` also needs `allow_raw`), or it is a
`413`." This covers a JSON API's PUT and PATCH and the loud opt-out.

### A7. How a PHP route receives proxy outputs was unstated [MAJOR]
Where: pool 46–56; CI 115–117.
Resolution: accepted by scope — this page is the FastCGI HTTP route; `[proxy.db]`
and the pool's `env[...]`/`[route.db]` delivery are the design's (MB.3.3) and the
manifest reference's. `statements: 0, proxies: {}` is the no-DB fixture; the page
does not claim a DB app needs no grant. `RESULT.md` Follow-up 3 names the owner.

### A8. `docs/frameworks/index.md` still said "pytest suite" for every backend [MINOR]
Where: `docs/frameworks/index.md` 63–64.
Resolution: fixed — the sentence now says "it runs your suite", which matches the
PHP page and still describes the WSGI pytest path.

### A9. The `workers` override link points at a reference with no `workers` [MINOR]
Where: 149.
Resolution: accepted — MB.3.5 adds `[route.fastcgi]` to the reference; line 3
marks the block 0.2.1, and the table on this page is the contract until then.

### A10. The security/how-it-works pointers have no FastCGI content [MINOR]
Where: 100–101.
Resolution: accepted, owned elsewhere — the node's law requires the pointer; the
mechanism sections are Follow-up 2, and MB.3.7's `e2e/php2shell.py` proves the
promise now.

### A11. The same 24 requests described a clean and an unexercised run [MINOR]
Where: 117, 134.
Resolution: fixed — the failing `--json` document now says `"requests": 3`, an
asset-only run, so the 24-request clean run and the 3-request failing run cannot
be read as one run.

### A12. Only `[route.fastcgi]` was marked 0.2.1 [MINOR]
Where: 3.
Resolution: fixed — line 3 names `[route.fastcgi]`, the PHP `--learn` static write
and `check -- <suite>` as the 0.2.1 additions.

### A13. `workers` was repeated and the page was at 150 lines [MINOR]
Where: block 19, table 31, deploy 147.
Resolution: fixed — the deploy sentence now says "Match the pool's
`pm.max_children` to `workers`", dropping the repeated default; the block and the
table keep the one useful repetition (example + default).

### A14. The local flow never said to stop the first `dev` [MINOR]
Where: 65–70, 106–108.
Resolution: accepted — "`-F` keeps it in the foreground" and the `--learn` Ctrl-C
make the foreground model clear; a separate terminal is the ordinary reading.
Adding a line would exceed the 150-line budget for a MINOR.

## Verdict

The manifest is bootable as written, `entry` is single-based and survives the
`--learn` state, the no-app FastCGI rule is stated, the `check -- <suite>` branch
and `BLASTIT_URL` reader are explicit, and the JSON example is reachable under the
page's own `exercised` rule. The two cross-doc gaps (the reference's `[route.fastcgi]`
entry and the proxy/pool env contract) are not this page's to write; both are
logged with owners in `RESULT.md`. No BLOCKER or MAJOR remains on the page, and
the MINORs are accepted with reasons. Every console fence is accounted.
VERDICT: PASS
