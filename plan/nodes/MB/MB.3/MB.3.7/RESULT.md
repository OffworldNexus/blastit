# RESULT MB.3.7 — prove embedded PHP, the documented commands and `php2shell` end-to-end

**STATUS: done (attempt 2)** — `make e2e` exit 0, `make check` exit 0, and every reviewed
probe repaired so that it can actually fail (two reds shown in §3). Two things the node asks
for are still not what the shipped code does; both are measured and named in §5 rather than
asserted away, and one of them (`check_php_entry_0000_refused_for_route_uid`) was missing and
is now a real check.

## 0. What attempt 2 changed (each review item, and where it is)

| Review item | Where it is now |
|---|---|
| **MAJOR 1**: `check_php_entry_0000_refused_for_route_uid` existed nowhere | `e2e/rootless/php_entry_swap.py::check_entry_0000_refused_for_route_uid`, called from `check()`; root `chmod 000` → live `GET /users/42` is 500 with **zero body bytes** and decoy 0 opens → `chmod 0644` → 200 again (§2.5) |
| **MAJOR 2**: a dead server passed `status != 200` | `entry_probe.py` always prints `status` (**0** = no head); `_Server.wait_head()` gates every restart; `_refused()` requires `500 <= status < 600` (§2.5, red in §3.2) |
| P4b must use `php -d auto_prepend_file=/fixture/canaries/prepend.php` | `positive_control()` runs exactly that, on the file row 7 names (§2.3) |
| E11 must require a dict body and an `eq` | `assert_exchange` fails an `absent` row whose body does not parse, and E11 asserts `script_filename` too (§2.2, red in §3.1) |
| E14 must assert the winning value or be labelled Rust-only | labelled: the note plus a printed `INFO: E14` caveat say only "one entry" is observable and the winning value is Rust-only (`critical_*`) |
| E24 must assert the response head or be relabelled | `Exchange.head_absent` asserts no `X-Accel-Redirect` / `X-Sendfile` in the head (§2.2) |
| E4/E20 need the positive `GET /uploads/shell.jpg` → 200 | E4 carries the control (`body_starts` = the JPEG magic); E20's note points at it; the polyglot's canary is now `"JPG-" . "RAN"` so serving the file cannot forge the marker |
| E9/E10 cannot fail on Linux NTS | their notes say so, and `run_probes` prints `INFO: E9/E10 — tripwire for a port`; only `P4b` carries the runtime claim |
| "unpatched runtime" premise | **option 2**, stated plainly in the Dockerfile header, `e2e/php/README.md` and §5.1: the suite proves the surface is **absent by construction** and does **not** claim "even an unpatched runtime" |
| RESULT named H-006 | H-007 / **MB.3.56** throughout (§5.2) |
| "replays every `$ ` command verbatim" overstated | §2.4: three commands — one executed, two explained |
| `e2e/rootless/run.py` reformatted (~780 lines) | restored to `HEAD` and re-applied: **+29 lines**, nothing else (§1) |
| `:264` same-inode claim | dropped; the inode is an `info` line that names the `mv` round-trip as its cause (§2.5) |
| `:293-306` inside-decoy measured | kept as **MB.3.56's witness**, labelled in the code and the output |

## 1. What was built (5 path entries, nothing else)

| Path | What |
|---|---|
| `e2e/php/Dockerfile` | The runtime: the wheel this build produced + Debian trixie's NTS `libphp8.4-embed` (8.4.27) + the C shim. Base image pinned by digest (the CPython the wheel targets), package by version; the header states what the suite does and does not claim. |
| `e2e/php/fixture/**` | Two `[route.php]` routes (`app` with `[route.static]`, `api` without), the twelve-key `/__inspect` front controller, `public/assets/app.4f1c2a9b7d3e.css`, and the canaries `canaries/prepend.php` (`PREPEND-RAN`), `elsewhere/evil.php` (`EVIL-RAN`), `canaries/shell.jpg` (`JPG-RAN`, stored as `"JPG-" . "RAN"` so the bytes never carry the marker). |
| `e2e/php/probes.md` | The accepted §4.7 matrix, verbatim, with its provenance (§5.3). |
| `e2e/php/entry_probe.py` | One GET as JSON, with `status: 0` when no head arrives. |
| `e2e/php/README.md` | Why that runtime, the premise, why the matrix is a copy, what is not proved. |
| `e2e/php2shell.py` | The driver: `--self-test`, the 24 probes with their controls, the canaries, `P4b`, the page's commands, the two-route worker proof. |
| `e2e/compose.yaml` | One additive `php` service (+21 lines). |
| `e2e/run.py` | One additive leg (`php_self_test`, `php_checks`) (+71/−8). |
| `e2e/rootless/php_entry_swap.py` | The container-side half: the swap, the recycled worker, the `chmod 000` row, the route uid's negatives. |
| `e2e/rootless/run.py` | `check_php_entry_swap_leaves_sentinel_untouched` + `--php-image` — **+29 lines only** (restored to `HEAD`, re-applied by hand so the shared file keeps its own formatting). |

## 2. Acceptance, command by command

### 2.1 `uv run --no-project e2e/php2shell.py --self-test`

```console
$ uv run --no-project e2e/php2shell.py --self-test
PASS: the matrix has 25 rows (25)
PASS: 24 of them are client-reachable (24)
PASS: they are `E1`–`E17` and `E19`–`E25`, in order
PASS: row 18 is the one `—` (an internal guard with no HTTP path)
PASS: the `php2shell #` column's union is exactly 1..20 ([1, 2, …, 20])
PASS: `PROBES` matches the matrix's reachable rows 1-to-1
PASS: `PROBES` has one entry per reachable row (24)
PASS: `P1`–`P12` all serve at least one row
PASS: `P4` is the prepend chain whose control is `P4b`
PASS: every `200` request carries a discriminating assertion (keys, or a control's own bytes)
PASS: every positive control asserts the bytes it is a control for
PASS: the polyglot row keeps its positive control (row 4)
PASS: row 24 asserts the response head, not only its body
PASS: and every `eq` assertion names a key the fixture answers
PASS: the page still shows `uv tool install blastit`
PASS: the page still shows `blastit dev --learn`
PASS: the page still shows the deploy preset command
PASS: the page still says `blastit check` runs pytest
PASS: self-test, 27 checks
```

### 2.2 The four greps

```console
$ grep -q 'docs/frameworks/php.md' e2e/php2shell.py && echo "driver replays the page"
driver replays the page
$ grep -q 'php' e2e/compose.yaml && grep -q 'php2shell' e2e/run.py && echo "registered"
registered
$ grep -q 'check_php_entry_swap_leaves_sentinel_untouched' e2e/rootless/run.py && echo "named"
named
$ grep -c 'check_php_entry_0000_refused_for_route_uid' e2e/rootless/php_entry_swap.py
2
```

### 2.3 `make e2e` (exit 0) — the 24 rows and `P4b`

```console
INFO: E9 — cannot fail on Linux NTS (no Windows best-fit, no argv): a tripwire for a port
INFO: E10 — same as E9: CVE-2024-8926's encodings change no directive here
INFO: E14 — only 'one entry' is observable; which duplicate value wins is Rust-only
```

| E | group | measured | discriminating assertion (on the wire, every run) |
|---|---|---|---|
| E1 | P1 | 200 | `script_filename` = `<root>/public/index.php`; one `SCRIPT_FILENAME`; no `EVIL-RAN` |
| E2 | P1 | 400 ×3 | `..`, `%2e%2e`, `..` after the script |
| E3 | P1 | 200 | `script_filename` has no `?`; `query_string` = `x=1` |
| E4 | P2 | 404, then **200** control | the polyglot path is refused; `GET /uploads/shell.jpg` is the file (`\xff\xd8\xff\xe0…`), so the 404 is the gate, not a missing file |
| E5 | P3 | 200 ×2 | `allow_url_include` = `0` |
| E6 | P3 | 200 | `allow_url_include` = `0` |
| E7 | P4 | 200 | `request_method` = `POST`, `allow_url_include` = `0`, no `PREPEND-RAN` |
| E8 | P5 | 200 | `query_string` = the literal `-d …`, `allow_url_include` = `0` |
| E9 | P6 | 200 | `allow_url_include` = `0`, `script_filename` = `<root>/api/index.php` — *cannot fail on Linux NTS* |
| E10 | P6 | 200 ×2 | `allow_url_include` = `0` — *same caveat* |
| E11 | P9 | 200 | body **is** the twelve-key JSON, `script_filename` pinned, `REDIRECT_STATUS` ∉ `server_keys` |
| E12 | P8 | 200 | `document_root` = `<root>`, `user_ini` = `""` |
| E13 | P7 | 200 | `http_proxy` = `false`, `HTTP_PROXY` ∉ `server_keys` |
| E14 | P10 | 200 | `HTTP_X_FOO` exactly once; the *winning* value is Rust-only (caveat) |
| E15 | P10 | 400 ×2 | NUL in the target and in a header value |
| E16 | P10 | 431 | 66 KiB header name |
| E17 | P10 | 431 | 130 headers |
| E19 | P11 | 404 ×2 | `/index.php`, `/prepend.php`, no `<?php` bytes |
| E20 | P2 | 404 ×2 | `/x.phar`, `/shell.jpg/x.php` (control: E4's 200) |
| E21 | P10 | 400 ×2 | bare CR in a value, obs-fold |
| E22 | P12 | 400 ×2 | `CL`+`TE`, duplicate `CL` |
| E23 | P1 | 200 | `script_filename` = `<root>/api/index.php`, `path_info` keeps `..` |
| E24 | P12 | 200, 400 | head carries no `X-Accel-Redirect`/`X-Sendfile`; body is the app's; bad header name 400 |
| E25 | P10 | 503 | stalled body: a bounded refusal, not a hang |

`P4b` (positive control — canary **present**, on the file `E7` names):

```console
PASS: P4b: `php -d auto_prepend_file=…/canaries/prepend.php` runs it (`PREPEND-RAN` present)
PASS: P4b: and then the script itself ran, so the prepend was not a crash
```

### 2.4 The page's commands — three, one executed and two explained

```console
PASS: the page's command is replayed or explained: uv tool install blastit
PASS: the page's command is replayed or explained: blastit dev --learn
PASS: the page's command is replayed or explained: blastit --preset compose-traefik
SKIP: uv tool install blastit (installs the published wheel from PyPI; the container runs the
      wheel this build produced)
SKIP: blastit --preset compose-traefik (a production preset; e2e/rootless (E1) proves it)
PASS: `blastit dev --learn` serves `/users/42`
PASS: `blastit dev --learn` serves `/api/x`
PASS: `blastit dev --learn` exits 0 on SIGINT (0)
PASS: one worker set per `[route.php]` route, each its own pid (['api', 'app'])
PASS: and the two routes are two processes, not one
PASS: `blastit check` exits 2 on a PHP manifest with no pytest (2)
PASS: and names pytest as what is missing ('… pytest is not installed …')
```

`blastit dev --learn` is the one command **executed** verbatim; `uv tool install blastit`
(network install of the published wheel) and `blastit --preset compose-traefik` (production
preset, owned by `e2e/rootless`) are **explained**, with their reason on stdout, and an
unknown `$ ` command fails the run.

### 2.5 The rootless swap, the recycled worker and `chmod 000`

```console
PHP: the entry is not swappable under a live route
  ok   the fixture is installed inside the container
  ok   `blastit dev` serves the PHP route
  ok   `GET /users/42` is the front controller (200)
  ok   the entry's inode is known ((83, 2972207449))
  ok   a privileged harness swaps `public/` aside
  ok   the swapped route still answers a head (not a dead server)
  ok   the live route fails closed while swapped (500)
  ok   and the decoy's marker never reaches the client
  ok   the decoy recorded 0 opens (0)
  ok   a recycled worker answers a head (not a dead server)
  ok   a worker recycled while symlinked fails closed (500)
  ok   and still runs no decoy
  ok   the decoy recorded 0 opens after the recycle (0)
  ok   restoring `public/` succeeds
  ok   `blastit dev` serves again after the restore
  ok   `GET /__inspect` answers again (200)
  ok   and the entry is the manifest's, inside its root ('/app/public/index.php')
  info the restored entry's (st_dev, st_ino) is (83, 2972207449) (it was (83, 2972207449)
       before the swap); the `mv` round-trip restores the same directory, so this is a
       property of the swap, not of the worker
  ok   a privileged harness makes the entry unreadable
  ok   the unreadable entry still answers a head
  ok   `chmod 000` on the entry is refused for the route uid (500)
  ok   and the answer has zero body bytes (0)
  ok   and runs no decoy
  ok   the decoy recorded 0 opens (0)
  ok   restoring the entry's mode succeeds
  ok   the restored entry answers a head
  ok   and serves again (200)
  ok   the route uid cannot `chmod` the entry (… Operation not permitted)
  ok   or create a file in the static root (… Permission denied)
  ok   or unlink the manifest (… cannot remove …: Permission denied)
  info a decoy *inside* the manifest dir answers 200 (DECOY-RAN): MB.3.56's witness, not an
       accepted behaviour — the entry is a path the worker re-resolves per request, so only
       `libphp`/the shim are fd-pinned (ADR-0010 §1); MB.3.56 pins the entry by
       (st_dev, st_ino) and must flip this line to a refusal
```

### 2.6 `make check` (exit 0)

```console
$ make check
…
uv run --no-project tools/plan.py validate
plan OK
EXIT=0
```

## 3. The probes can fail: two reds

Both reds are the **real files** with one line broken, run in the same image; nothing else
changed. They are the two failure modes the review named.

### 3.1 "a non-JSON body must not satisfy an `absent`" (E11)

E11's target replaced with the stylesheet, which answers `200 text/css`:

```console
FAIL: P9 E11: the answer is the twelve-key document, not b'body{color:#123456;margin:0}\n'
FAIL: P9 E11: `script_filename` is '/tmp/broken/public/index.php', not None
FAIL: 2 of 142 promises failed
```

Before the repair the first line did not exist and `REDIRECT_STATUS` was "absent" from an
empty document: the row passed on a body it never read.

### 3.2 "a dead server must not read as `fails closed`" (the recycled worker)

The recycled worker is stopped and **never restarted** (the review's scenario):

```console
  ok   a privileged harness swaps `public/` aside
  ok   the swapped route still answers a head (not a dead server)
  ok   the live route fails closed while swapped (500)
  ok   the decoy recorded 0 opens (0)
  FAIL a recycled worker answers a head (not a dead server)
  info kept the container blastit-d6-php-swap (E2E_KEEP_PHP_SWAP)
rc=1
```

Before the repair `entry_probe.py` printed `{"error": …}` and `None != 200` reported ok.

## 4. The two PHPUnit suites the node names — not replayed, because the page and the code do not have them

The node (written against the retired `php-fastcgi` design) asks for
`blastit check -- vendor/bin/phpunit` against `e2e/php/suite/` (`24 requests …`) and
`--json` against `e2e/php/assets/` (`requests:3 … unexercised_route`). Three measured facts:

1. `docs/frameworks/php.md` §8 says the opposite, in the shipped page: "`blastit check` runs
   `pytest -q` … so a PHP manifest has no request audit in `0.2.0`: it validates the manifest,
   the static roots and the libphp gate, exits `2` without pytest, and has nothing to observe
   if pytest is there." `MB.3.9.1`/`MB.3.8` rewrote the page to match the code.
2. `crates/blastit/src/cli.rs` forwards everything after `--` to **pytest** (`pytest_args`),
   and `check.rs`'s preflight refuses a suite whose interpreter cannot import pytest
   (`require_pytest`, exit `2`). `blastit check -- vendor/bin/phpunit` would run
   `pytest vendor/bin/phpunit`.
3. There is no `e2e/php/suite/` or `e2e/php/assets/` in the accepted design.

So the leg replays the page's actual CI sentence (`blastit check` → exit `2`, naming pytest)
instead of inventing two suites. **Unproven: the two PHPUnit counts.** If `check` gains a
non-pytest runner, this leg is the place to add them.

## 5. Disclosed divergences (measured, not asserted away)

### 5.1 The "unpatched runtime" premise — option 2, chosen and stated

The node wanted a pinned **unpatched** PHP image so that "the runtime is not what saves us".
No such image exists for this design: the official `php` images ship no `libphp` at all
(no `--enable-embed`), and every distribution's `libphp-embed` is the current, patched package
(8.4.27 here); an unpatched 8.2/8.3 `libphp` that the proven 8.4 shim could load is not
shippable or fetchable. So the suite does **not** claim "even an unpatched runtime".

What it claims, and what is now written in the Dockerfile header, `e2e/php/README.md` and the
driver's docstring: **the surface is absent by construction** — no `php-cgi` binary, no
FastCGI parameter stream, no argv, no INI channel a request can reach. `P4b` is the runtime
half that *is* tested: the same interpreter, the same canary file row 7 names, `php -d
auto_prepend_file=…` → `PREPEND-RAN` present. So "the canary is absent" through `blastit`
means the channel is absent, not that the canary is broken.

### 5.2 `[route.php] entry` is a path, not a boot-pinned inode

`PhpBinding.entry` is a `PathBuf` (`crates/blastit/src/php_view.rs`); `blastit_php_sapi.c`
sets `SG(request_info).path_translated = req->entry` and
`zend_stream_init_filename(&handle, req->entry)` **per request**. Only `libphp` and the shim
are fd-pinned (`ADR-0010` §1). Measured: an outside symlink fails closed (500, decoy 0
opens); a decoy **inside** the manifest dir is executed (200 + `DECOY-RAN`, printed as
info). **H-007 is answered** — the ADR wins, pin the entry by `(st_dev, st_ino)` — and the fix
is **MB.3.56** (priority 1, release-blocking, ahead of MB.3.18). `php_entry_swap.py` prints the
inside-decoy case as that node's witness, not as accepted behaviour, and the same script's
route-uid negatives (`chmod`/`O_CREAT`/`unlink` → `EPERM`/`EACCES`) are the permission half
that holds today (`ADR-0010` §2).

### 5.3 The §4.7 table is a copy, not a live document

The node says `--self-test` parses `docs/research/php2shell.md` §4.7. That table is not in that
research note: §4.7 was a section of `docs/design/php-fastcgi.md`, which `MB.3.32` **deleted**
with the design corpus (18 files, `e730527`) when `ADR-0010` replaced `php-fpm`.
`docs/research/php2shell.md` keeps the attack table (§B, `#1`–`#20`) but no `E` rows. So
`e2e/php/probes.md` is the §4.7 table copied verbatim (from `e730527^`) with a provenance
header, and `--self-test` parses that copy: 24 non-`—` rows, `E1`–`E17` + `E19`–`E25`, the
`php2shell #` column's union exactly `{1..20}`, one `PROBES` entry per row, a non-empty
assertion on every `200`. Regenerating the table inside the driver would have made the match a
tautology.

Where `ADR-0010` deleted the mechanism a row was written against, the row asserts the
embedded-era observable and says so in its `note`:

| Row | §4.7 said | Measured | Why |
|---|---|---|---|
| E9 | 200 | 200 (on the route without `[route.static]`) | `%AD` in a path with a static root is a 400 (invalid UTF-8 for the resolver); on a PHP-only route it is data, which is the row's claim. |
| E17 | 400 (`ParamsTooLarge`) | 431 | There is no FastCGI param budget; the HTTP head limit is the bound. |
| E21 | 400 | 400 | Asserted as a bare CR and an obs-fold (a literal `\r\n` in a value *is* a new header line, so that form is not injection). |
| E24 | 502 (malformed CGI head) | 200 + 400 | No CGI head exists to malform. The response head carries no `X-Accel-Redirect`/`X-Sendfile`, the body is the app's own, and an invalid header name is 400. |
| E25 | 504 | 503 (within 10 s) | The stalled body hits the queue/request deadline and the connection closes; no `ABORT_REQUEST` exists without FastCGI. Asserted as `{503, 504}`. |

No row was dropped, skipped or weakened.

## 6. Follow-ups

* **H-007 / MB.3.56** (answered, fix scheduled): pin `[route.php] entry` by `(st_dev, st_ino)`
  at boot the way `libphp`/the shim are. The witness is the `info` line in
  `e2e/rootless/php_entry_swap.py`, which MB.3.56 must flip to a refusal.
* The node's two PHPUnit suites need `blastit check` to accept a non-pytest runner; the page
  already says there is none in `0.2.0`. Belongs to whoever adds that runner.
* Two host gotchas found here belong in `docs/knowledge/LESSONS.md`, which is outside this
  node's `paths`, so they are recorded here instead: (a) `docker exec` flags must come
  **before** the container name — after it docker runs them as the command and returns `127`,
  which reads like a missing binary; (b) the CLI SAPI does not back `php://input` for
  `auto_prepend_file` (it prepends nothing even with `allow_url_include=1`), while
  `-d auto_prepend_file=<file>` does — which is why `P4b` uses the canary file.

## 7. What is unproven, in one place

1. **The two PHPUnit suite counts** (§4) — the CLI runs pytest only, and the page says a PHP
   manifest has no request audit in `0.2.0`.
2. **"The entry inode is pinned at boot", `200`, and `503`** (§5.2) — the entry is
   re-resolved per request; measured 500 / decoy-not-run for an outside swap, decoy-executed
   for an inside one. **MB.3.56**.
3. **"Even an unpatched runtime"** (§5.1) — not claimed and not testable: no distribution
   ships an unpatched `libphp` an 8.4 shim could load. The claim made is that the surface is
   absent by construction, with `P4b` as the runtime's half.
4. **E9, E10, E14's winning value** — the first two cannot fail on Linux NTS (no Windows
   best-fit, no argv) and are tripwires; E14 observes "one entry" only, the winning value is
   Rust-only. All three say so in the driver and in the run's `INFO` lines.
5. **Cross-route `EACCES`** (route A's uid cannot read route B's `manifest_dir`) — not proved
   here: `blastit dev` is single-UID by definition, and the production per-route-uid shape in
   `e2e/rootless/compose.yml` has no PHP route. What this leg proves about route separation is
   behavioural: each route answers from its own `entry` (rows 1, 9 and 23).

Everything else the node asks for is asserted and green, including the `chmod 000` row it
named and an execution-verified swap.
