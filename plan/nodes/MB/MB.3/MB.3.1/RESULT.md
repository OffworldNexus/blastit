# Result MB.3.1

## Summary

Both cited notes exist under `docs/research/` and are indexed:

- `docs/research/php-fastcgi-wire.md` — FastCGI 1.0 header/record layout, name-value
  encoding, request-id semantics (php-fpm never multiplexes), the `PARAMS`
  no-mid-pair-split rule, the 65 535-byte record cap, 8184-byte `STDOUT` chunking, the
  `STDOUT`→`END_REQUEST` ordering, and the two measured php-fpm quirks a client must
  tolerate (`appStatus` always 0, `reserved[3]` uninitialised). It also records the
  canonical non-root run line and the pool file that actually started on PHP 8.4.27, plus
  what breaks in a root-era pool file (`listen.owner`/`listen.group` → fatal `chown`).
- `docs/research/php2shell.md` — 20-row attack table with the exact attacker-controlled
  field per technique (`SCRIPT_FILENAME`, `PATH_INFO`, `PHP_VALUE`, `PHP_ADMIN_VALUE`,
  `auto_prepend_file`, `allow_url_include`, `DOCUMENT_ROOT`, `REDIRECT_STATUS`,
  duplicate/NUL/over-long names), the concrete binary defence, the CVE-2012-1823 /
  CVE-2024-4577 / CVE-2024-8926 / CVE-2016-5385 record, how Symfony/Laravel/Slim consume
  the params, and what stock nginx `fastcgi_params` + `try_files` set (contrast only).

`php-fpm` is **not installed on this host** (`command -v php-fpm` empty, no
`/usr/sbin/php-fpm*`). Every php-fpm measurement is therefore against
`php:8.4-fpm-alpine` (PHP **8.4.27 (fpm-fcgi)**, `php:8.4-cli` for the php-cgi CVE probes)
with a non-root uid-1000 master and a hand-built stdlib-Python FastCGI client. The probe
tree is throwaway under `/tmp/opencode/php-probe/` and is not part of the repo.

Headline measured facts that the notes stand on (all reproduced today):

- `PHP_VALUE=allow_url_include=1` + `auto_prepend_file=php://input` + `CONTENT_LENGTH` +
  body executes the body as PHP (`PWNED-CHAIN`) on 8.4.27, and the ini change **persists in
  the worker** across later requests (`pm.max_requests = 0` default ⇒ until restart).
- `PHP_VALUE=extension=/app/www/evil.so` reaches `dl()` (log: `Unable to load dynamic
  library '/app/www/evil.so'`).
- `security.limit_extensions` empty ⇒ `SCRIPT_FILENAME=/app/www/shell.jpg/x.php` executes
  `shell.jpg`; with `.php` both that and `shell.jpg` are `403`.
- `DOCUMENT_ROOT` pointing at a directory containing `.user.ini` makes that file's
  `auto_prepend_file` run (`PREPEND-RAN`).
- Patched php-cgi 8.4.27 treats a leading-`-` `QUERY_STRING` as literal data (CVE-2012-1823
  neutralised) and the `%AD` form is inert on Linux (CVE-2024-4577 is Windows-only).
- Non-root `php-fpm -F -y` starts; `user`/`group` are ignored with a NOTICE;
  `listen.owner`/`listen.group` abort startup with `failed to chown() the socket`.

## Evidence

### Acceptance: both notes exist

```console
$ test -f docs/research/php-fastcgi-wire.md && test -f docs/research/php2shell.md && echo "both notes exist"
both notes exist
```

### Acceptance: required wire terms

```console
$ for k in BEGIN_REQUEST PARAMS STDIN STDOUT STDERR END_REQUEST requestId paddingLength; do grep -q "$k" docs/research/php-fastcgi-wire.md || echo "missing $k"; done
(no output)
```

### Acceptance: non-root run line

```console
$ grep -q 'php-fpm -F -y' docs/research/php-fastcgi-wire.md && echo "non-root run line recorded"
non-root run line recorded
```

### Acceptance: `SCRIPT_FILENAME` and `PATH_INFO`

```console
$ grep -q 'SCRIPT_FILENAME' docs/research/php2shell.md && grep -q 'PATH_INFO' docs/research/php2shell.md
(exit 0)
```

### Acceptance: every required attack term

```console
$ for k in PHP_VALUE PHP_ADMIN_VALUE REDIRECT_STATUS auto_prepend_file allow_url_include HTTP_PROXY httpoxy CVE-2012-1823 CVE-2024-4577; do grep -q "$k" docs/research/php2shell.md || echo "missing $k"; done
(no output)
```

### Acceptance: citations and table rows

```console
$ test "$(grep -c 'https\?://' docs/research/php2shell.md)" -ge 15 && echo "php2shell note is cited"
php2shell note is cited
$ test "$(grep -c '^|' docs/research/php2shell.md)" -ge 12 && echo "one table row per technique"
one table row per technique
$ grep -c 'https\?://' docs/research/php2shell.md
98
$ grep -c '^|' docs/research/php2shell.md
44
```

### Acceptance: index rows

```console
$ grep -q 'php-fastcgi-wire' docs/research/INDEX.md && grep -q 'php2shell' docs/research/INDEX.md
(exit 0)
```

### Acceptance: `make docs-check`

```console
$ make docs-check
uvx --offline zensical build --strict
Build started
No issues found
Build finished in 0.19s
```

### Acceptance: `make check`

```console
$ make check
...
PASS  docs/frameworks/static-and-spa.md  fence 6  static                0.0s  statically checked every command, flag and snippet (...)
PASS  docs/frameworks/static-and-spa.md  fence 7  static                0.0s  statically checked every command, flag and snippet (...)
PASS  docs/frameworks/static-and-spa.md  fence 8  presets               0.0s  each preset exists and fails closed without capabilities
PASS  docs/reference/cli.md  fence 1  dev-flags             0.1s  each dev flag boots and stops (port 9000 was busy; ran the documented flag on 9001 (declared in inventory.toml))
PASS  docs/reference/cli.md  fence 2  check-flags           4.5s  each check flag runs
PASS  docs/reference/cli.md  fence 3  serve-flags           0.0s  production refuses closed; help lists the commands and presets
PASS  docs/reference/cli.md  fence 4  usage                 0.1s  the documented fold signature exists
PASS  docs/security.md  fence 1  static                0.0s  statically checked every command, flag and snippet (...)
47 fences: 28 executed, 19 accounted, 0 failed · 23.2s
uv run --no-project tools/plan.py validate
plan OK
MAKE_CHECK_EXIT=0
```

Run twice: once before the final note edits and once after (`MAKE_CHECK_EXIT=0` both times).

### Probes (raw command lines, reproducible)

```console
$ docker run --rm php:8.4-fpm-alpine php-fpm -v
PHP 8.4.27 (fpm-fcgi) (built: Nov  5 2026 19:02:14) (NTS)

$ docker run -d --name fpm-probe --user 1000:1000 -v /tmp/opencode/php-probe:/app \
    php:8.4-fpm-alpine php-fpm -F -y /app/pool-nonroot.conf
[NOTICE: [pool www] 'user' directive is ignored when FPM is not running as root
 NOTICE: [pool www] 'group' directive is ignored when FPM is not running as root
 NOTICE: fpm is running, pid 1
 NOTICE: ready to handle connections]

# same pool with listen.owner/listen.group present:
ERROR: [pool www] failed to chown() the socket '/app/run/fpm.sock': Operation not permitted (1)
ERROR: FPM initialization failed

$ uv run --no-project fcgi.py baseline      # annotated BEGIN/PARAMS/STDIN -> STDOUT/END
$ uv run --no-project probe2.py persist     # PHP_VALUE persists in the worker
$ uv run --no-project probe2.py traversal   # limit_extensions + fix_pathinfo truncation
$ uv run --no-project probe3.py dup_script  # duplicates, PROXY, NUL, id 0, keepConn
$ docker run --rm -i ... php:8.4-cli sh -lc '... QUERY_STRING="-d allow_url_include=1 ..." php-cgi'
```

Key observed lines (quoted in the notes):

```text
PWNED-CHAIN                                      # PHP_VALUE chain, PHP 8.4.27
JPG-RAN uid=1000                                 # shell.jpg/x.php, empty limit_extensions
PREPEND-RAN / FAKE-RAN                           # DOCUMENT_ROOT-spoofed .user.ini
SERVER_HTTP_PROXY=NULL / getenv_HTTP_PROXY=false # httpoxy over FastCGI
Script: QUERY_STRING=-d allow_url_include=1 ...  # patched php-cgi keeps it literal
Security Alert! The PHP CGI cannot be accessed directly.   # php-cgi without REDIRECT_STATUS
```

## Review fixes (post-PASS)

Three MINOR accuracy findings from the reviewer, fixed without touching any other claim:

1. §J's annotated request now shows the probe client's extra empty `STDIN` at offset 136, so
   the offsets sum to the stated 144 bytes (16 + 104 + 8 + 8 + 8), with a sentence saying one
   empty `STDIN` is what the spec asks for.
2. The `fix_pathinfo` paragraph in §K (and the parallel bullet in `php2shell.md` §C) now
   states that truncation alone leaves `PATH_INFO` unset (`PHP_SELF=/index.php`, measured)
   and that `PATH_INFO=/extra`/`PHP_SELF=/index.php/extra` requires a `PATH_INFO` param
   (`fpm_main.c`: `path_info = (env_path_info && pilen > slen) ? … : NULL`). The same
   correction was applied to the `PATH_INFO` open question in `php2shell.md`, which had
   inherited the wrong premise.
3. `php2shell.md` DX bullet: the design sentence ("the harness owning the param set rather
   than filtering it") moved to `## Open questions for the design`; the measurement stays.

Re-ran the acceptance greps (all PASS) and `make docs-check` ("No issues found") after the
edits; only `docs/research/**` and this file changed, and `docs/research/` is excluded from
the Zensical build.

## Files

- `docs/research/php-fastcgi-wire.md` (new)
- `docs/research/php2shell.md` (new)
- `docs/research/INDEX.md` (two rows appended)

## Follow-ups

- **No `php-fpm` on this host** — the wire and pool facts are verified on
  `php:8.4-fpm-alpine` (8.4.27), not a distro package. A distro `php8.x-fpm` may ship a
  different `php.ini` (`variables_order`, `allow_url_fopen`) and wrapper NOTICEs.
- **Unverified mechanism:** why duplicate params differ (by-name lookups take the last
  occurrence, a plain `$_SERVER` key keeps the first) — observed twice, not traced through
  `fcgi_hash_apply`/`php_register_variable_ex`. A debug build printing `$_SERVER` in
  insertion order settles it.
- **Unverified:** `doc_root` containment (manual promises "no script outside this
  directory"; `php_fopen_primary_script` ignores `path_translated` when `doc_root` is
  absolute) — one probe with `doc_root=/app/www` + `/app/elsewhere/evil.php` settles it.
- **Unverified:** whether a pool `env[HTTP_PROXY]` plus a request `HTTP_PROXY` param
  re-opens httpoxy (needs a pool edit).
- **Unverified:** `PHP_ADMIN_VALUE` vs an attacker-written `.user.ini` (does the pinned
  value win?), and the exact behaviour at the `pm.max_requests` recycle boundary.
- **Unverified:** PHP 8.5 (`PHP_VALUE` path unchanged as far as 8.4 shows; no 8.5 image
  probed) and Windows php-cgi 4577 (advisory only).
- **Out of scope, found while probing:** `PHP_VALUE extension=` is a `dl()` primitive with
  no change-mode gate — worth a `security-invariants`/`security.md` line when 0.2.1 lands,
  but this node writes research only.
- **Candidate `LESSONS.md` bullet (not added; node paths are `docs/research/**` only):** a
  root-era `php-fpm` pool file aborts as a normal user on `listen.owner`/`listen.group`
  (`failed to chown() the socket … Operation not permitted` → `FPM initialization failed`),
  while `user`/`group` are only NOTICEs. Future non-root php-fpm work will hit it again.
- `make check` rebuilt `dist/blastit-0.2.0-cp314-*.whl` (normal for `docs-test`); it is a
  tracked-ignored build artefact, nothing was staged or committed.
