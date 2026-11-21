# php2shell: FastCGI/CGI parameter injection against PHP, field by field

- Node: MB.3.1   - Author: researcher   - Date: 2026-11-14

## TL;DR (<= 8 lines)

- "`php2shell`" is one class with several delivery channels: a FastCGI client that lets
  request bytes name a **parameter** (`SCRIPT_FILENAME`, `PATH_INFO`, `PHP_VALUE`,
  `PHP_ADMIN_VALUE`, `DOCUMENT_ROOT`) hands the attacker the interpreter's own knobs;
  nginx never lets a client name those (it computes them from `$fastcgi_script_name`), so
  the risk belongs to whoever speaks FastCGI directly.
- php-fpm executes **any** readable `.php` file named by an absolute `SCRIPT_FILENAME`
  (measured: `/app/elsewhere/evil.php` ran from outside `DOCUMENT_ROOT`).
- `cgi.fix_pathinfo=1` truncates `SCRIPT_FILENAME` back to the first existing file, so
  `/uploads/x.jpg/index.php` executes `x.jpg`; `security.limit_extensions` (default
  `.php .phar`) is the only gate, and an empty value disables it (measured).
- **`PHP_VALUE`/`PHP_ADMIN_VALUE` are honoured with no change-mode gate**: `PHP_VALUE`
  set `allow_url_include=1` (an `INI_SYSTEM` directive), loaded an arbitrary `.so` through
  `extension=`, and the values **persist in the worker across later requests** (measured).
- `PHP_VALUE=allow_url_include=1` + `auto_prepend_file=php://input` + a body executed the
  body as PHP (**`PWNED-CHAIN`**) on PHP 8.4.27 — the classic chain works over FastCGI
  params, with no `php-cgi` and no query string.
- CVE-2012-1823 is php-cgi-only and patched php-cgi now treats a leading `-` query string
  literally (measured); **CVE-2024-4577 is Windows php-cgi + Apache, not Linux php-fpm**
  (no code-page best-fit exists there) and is regression-tested only because its first fix
  was bypassed (CVE-2024-8926).
- `DOCUMENT_ROOT` is load-bearing: pointing it at a directory with a `.user.ini` made that
  file's `auto_prepend_file` run (measured).
- The defaults that help are `security.limit_extensions=.php .phar`, `allow_url_include=0`,
  `clear_env=yes`; the defaults that do not are `cgi.fix_pathinfo=1` and a trusted
  `PHP_VALUE`.

## Developer Experience Implications

- **Nothing in this class needs a `blastit.toml` key.** The parameters are the interpreter's
  own CGI contract; a 12-factor developer writes `root`/entry script and expects
  `/checkout/42` to reach `public/index.php`. The framework does not ask for
  `PATH_INFO`, `SCRIPT_NAME` or `REDIRECT_STATUS` (Symfony routes on `REQUEST_URI`,
  Laravel on the Symfony request, Slim's Diactoros on `REQUEST_URI` — see §I), so a
  correct client can build every param from the HTTP request plus the route's `root`, and
  the ones it must never forward are exactly the ones no framework reads.
- **The safe configuration is the default one**: `security.limit_extensions = .php .phar`,
  `allow_url_include = 0`, `cgi.fix_pathinfo = 1` is *not* safe by itself, but the first two
  are, and `clear_env = yes` is already the pool default
  ([FPM configuration](https://www.php.net/manual/en/install.fpm.configuration.php)).
  A developer who never writes a pool file still gets the extension gate.
- **The failure mode a developer would otherwise debug is invisible**: a `PHP_VALUE` in one
  request poisons the worker for every later request (measured, `pm.max_requests = 0`
  means until restart), so "it worked, then everything 500s" has no line in the access log.
- **`try_files $uri =404` is the nginx line developers are told to add**, and it exists only
  because nginx cannot otherwise stop `fix_pathinfo`
  ([nginx `try_files`](https://nginx.org/en/docs/http/ngx_http_core_module.html#try_files));
  a client that computes `SCRIPT_FILENAME` from a validated path has the same guarantee
  without a config line.
- **Symfony's nginx recipe marks the front controller `internal` and 404s every other
  `.php`** ([Symfony nginx](https://symfony.com/doc/current/setup/web_server_configuration.html));
  that is the framework itself saying "only one script is legitimately reachable", which
  is a per-route fact a manifest can state once.

## Technical Findings

Measured on PHP **8.4.27 (fpm-fcgi)**, `php:8.4-fpm-alpine`, non-root master, pool
`security.limit_extensions = .php`, client = hand-built FastCGI (`/tmp/opencode/php-probe`).
Source citations are php-src `PHP-8.4`. Anything not measured is marked.

### A. Three delivery channels, only one of which lets a request name a param

1. **Path bytes → server-computed param.** nginx sets
   `fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name` and
   `fastcgi_param PATH_INFO $fastcgi_path_info`; the client can only shape those two
   variables, never name them
   ([nginx parameters](https://nginx.org/en/docs/http/ngx_http_fastcgi_module.html#parameters),
   [`fastcgi_split_path_info`](https://nginx.org/en/docs/http/ngx_http_fastcgi_module.html#fastcgi_split_path_info)).
2. **Header → `HTTP_*` param.** "HTTP request header fields are passed to a FastCGI server
   as parameters … the `User-Agent` header field is passed as the `HTTP_USER_AGENT`
   parameter" (same page). This channel is `httpoxy` and every `HTTP_*` `$_SERVER` key.
3. **Direct PARAMS.** A FastCGI client that forwards arbitrary param names can send
   `SCRIPT_FILENAME`, `PATH_INFO`, `PHP_VALUE`, `PHP_ADMIN_VALUE`, `DOCUMENT_ROOT`
   verbatim. php-fpm reads them by exact name
   ([`fpm_main.c::init_request_info`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_main.c)).

Everything in §B except httpoxy and the two CVEs is channel 3.

### B. The attack table

| # | Technique | Attacker field (exact) | What it reaches | Binary defence |
|---|---|---|---|---|
| 1 | `SCRIPT_FILENAME` override | FastCGI param `SCRIPT_FILENAME` | any readable `.php` on disk: `/app/elsewhere/evil.php` ran with `DOCUMENT_ROOT=/app/www` (measured) | refuse the param; compute it from the validated route path |
| 2 | `SCRIPT_FILENAME` traversal | `SCRIPT_FILENAME` containing a `..` path segment | php-fpm refuses it (`is_valid_path`) → `404 File not found.` (measured); not a place to rely on | pin the value; never concatenate caller bytes |
| 3 | `SCRIPT_FILENAME` + `?query` suffix | `SCRIPT_FILENAME=/app/www/index.php?x=1` | not stripped on 8.4: the `?` is part of the filename → `404` (measured) | pin; refuse `?` in a param value |
| 4 | `PATH_INFO` folded in (`fix_pathinfo`) | `SCRIPT_FILENAME=/app/www/shell.jpg/x.php` (+ optional `PATH_INFO`) | executes `shell.jpg` when `security.limit_extensions` is empty (measured `JPG-RAN`); `403` with `.php` | refuse `PATH_INFO`; pin `SCRIPT_FILENAME`; keep `limit_extensions` |
| 5 | `..` after a real script | `SCRIPT_FILENAME=/app/www/index.php/trailing/../junk` | truncation removes the `..` and `index.php` executes (measured) | pin the value; do not validate by substring |
| 6 | `PHP_VALUE` | param `PHP_VALUE` (INI syntax, newline separated) | *any* directive, including `INI_SYSTEM`; survives into later requests (measured) | refuse the param unconditionally |
| 7 | `PHP_VALUE extension=` | `PHP_VALUE=extension=/app/www/evil.so` | `dl()` of an arbitrary shared object: log line `Unable to load dynamic library '/app/www/evil.so'` (measured) | refuse the param unconditionally |
| 8 | `PHP_ADMIN_VALUE` | param `PHP_ADMIN_VALUE` | same as 6 and also pins `modifiable`; `open_basedir=/etc` applied and persisted (measured) | refuse the param unconditionally |
| 9 | `auto_prepend_file` via `PHP_VALUE` | `PHP_VALUE=auto_prepend_file=/app/www/prepend.php` | attacker file parsed before every script (`PREPEND-RAN`, measured) | refuse `PHP_*_VALUE`; pin `auto_prepend_file` empty |
| 10 | `auto_prepend_file=php://input` | `PHP_VALUE=allow_url_include=1\nauto_prepend_file=php://input` + body + matching `CONTENT_LENGTH` | body executed as PHP (`PWNED-CHAIN`, measured) | refuse `PHP_*_VALUE`; keep `allow_url_include=0` |
| 11 | `allow_url_include` via `PHP_VALUE` | same param | `php://input`, `data://` usable by `include`; `ini_get` returned `1` (measured) | same as 10 |
| 12 | CVE-2012-1823 argument injection | query string starting with `-` and containing no `=` (`?-d allow_url_include=1 -d auto_prepend_file=php://input`) | **php-cgi only**: argv; patched 8.4.27 treats it as a literal `QUERY_STRING` (measured) | do not spawn php-cgi; never map query bytes to argv |
| 13 | CVE-2024-4577 | query byte `%AD` (soft hyphen) best-fit to `-` under a Windows code page | **Windows php-cgi + Apache only**; no Linux FPM path exists (no ANSI→UCS-2 best fit; `QUERY_STRING` is never argv) | none needed for FPM; keep it in the regression suite (first fix bypassed as CVE-2024-8926) |
| 14 | httpoxy | header `Proxy:` → param `HTTP_PROXY` | `getenv('HTTP_PROXY')` and `$_SERVER['HTTP_PROXY']`; in FPM the param is **not** exported and PHP 8.4 drops it from `$_SERVER` (measured: `NULL`), while a bare `Proxy` param stays as `$_SERVER['Proxy']` (measured) | never emit `HTTP_PROXY`; if the app needs one, set it as a real env var |
| 15 | `REDIRECT_STATUS` | param `REDIRECT_STATUS` | php-cgi's `cgi.force_redirect` gate only: php-cgi without it prints "Security Alert! The PHP CGI cannot be accessed directly." (measured); php-fpm ignores it | do not treat it as a guard; do not forward it |
| 16 | `DOCUMENT_ROOT` spoof | param `DOCUMENT_ROOT` | `PATH_TRANSLATED` arithmetic and **which `.user.ini` is loaded**: `DOCUMENT_ROOT=/app/fake` with a `.user.ini` there ran `PREPEND-RAN` (measured) | pin `DOCUMENT_ROOT` to the route's `root` |
| 17 | Duplicate param | same name twice | differential: `SCRIPT_FILENAME`/`QUERY_STRING`/`REQUEST_METHOD` take the **last** value (measured), a plain `$_SERVER` entry keeps the **first** (`FOO=first`, measured) | reject duplicate names (mechanism in §H, unverified) |
| 18 | NUL in a value | `SCRIPT_FILENAME=/app/www/shell.jpg\0.php` | C-string truncation before the extension check: treated as `shell.jpg` (403 under `.php`) and `index.php\0.jpg` ran `index.php` (measured) | reject any param value containing `\0` |
| 19 | Unknown / bare params | any name, e.g. `X_TEST`, `FOO` | every param becomes a `$_SERVER` key unfiltered (measured); `HTTP_` prefixing is the front-end's job | allowlist the exact param names |
| 20 | Over-long name/value | a 60 000-byte param name | accepted; bounded only by the 65 535-byte record and `memory_limit` (measured: the request still served `index.php`) | cap param count and total size |

Sources for the mechanics: [`fpm_main.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_main.c)
(`init_request_info`, `is_valid_path`, `sapi_cgi_activate`, `fastcgi_ini_parser`),
[`fpm_php.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_php.c)
(`fpm_php_limit_extensions`, `fpm_php_zend_ini_alter_master`),
[`main/fastcgi.c`](https://github.com/php/php-src/blob/PHP-8.4/main/fastcgi.c)
(`fcgi_get_params`, `fcgi_hash_set`, `fcgi_hash_get`),
[`main/fopen_wrappers.c`](https://github.com/php/php-src/blob/PHP-8.4/main/fopen_wrappers.c)
(`php_fopen_primary_script`, `doc_root`/`user_dir`),
[`main/php_variables.c`](https://github.com/php/php-src/blob/PHP-8.4/main/php_variables.c)
(`check_http_proxy`, name mangling), [`sapi/cgi/cgi_main.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/cgi/cgi_main.c)
(`skip_getopt`, `force_redirect`).

### C. What php-fpm does with the `SCRIPT_FILENAME` it is handed

- It is taken verbatim: `env_script_filename = FCGI_GETENV(request, "SCRIPT_FILENAME")`,
  with `PATH_TRANSLATED` as the fallback, then `SG(request_info).path_translated`
  (`init_request_info`). **No containment against `DOCUMENT_ROOT`** — measured by running
  `/app/elsewhere/evil.php` while `DOCUMENT_ROOT=/app/www`.
- `is_valid_path()` refuses any `..` *segment* in the final translated path and the answer is
  `404 File not found.`, not `403`. Because `fix_pathinfo` rewrites the path first, a `..`
  after an existing script disappears (§B#5).
- `cgi.fix_pathinfo` is `1` and `INI_SYSTEM`
  ([manual](https://www.php.net/manual/en/ini.core.php#ini.cgi.fix-pathinfo)): if the path
  does not stat, php-fpm walks back through `/` to the first existing regular file and
  **truncates `SCRIPT_FILENAME` to it**. Measured: `SCRIPT_FILENAME=/app/www/index.php/extra`
  executed `index.php` with `$_SERVER['SCRIPT_FILENAME']` truncated to `/app/www/index.php`.
  **Truncation alone leaves `PATH_INFO` unset** (`PATH_INFO` unset, `PHP_SELF=/index.php`,
  measured; `fpm_main.c`: `path_info = (env_path_info && pilen > slen) ? … : NULL`); the
  `PATH_INFO=/extra`, `PHP_SELF=/index.php/extra` result needs a `PATH_INFO` param as well
  (measured).
- `security.limit_extensions` (default `.php .phar`) compares the **suffix of the final
  path** and answers `403 Access denied.` ([manual](https://www.php.net/manual/en/install.fpm.configuration.php#security.limit-extensions),
  `fpm_php_limit_extensions`). An empty value builds no list at all, i.e. everything is
  allowed (`fpm_conf.c`, `fpm_php.c`), which is how `shell.jpg` became executable.
- `doc_root` is an alternative containment: when it is set, `php_fopen_primary_script`
  builds `doc_root + request_uri` and **ignores** `path_translated`; the manual promises
  "you can be sure no script is executed outside this directory"
  ([CGI security, case 3](https://www.php.net/manual/en/security.cgi-bin.doc-root.php),
  [`doc_root`](https://www.php.net/manual/en/ini.core.php#ini.doc-root)). It is empty by
  default, and it only helps if the request URI is pinned too.
- Relative `SCRIPT_FILENAME` resolves against the worker's CWD: `www/index.php` 404'd in a
  container whose CWD was `/` (measured). Pool `chdir` changes that CWD
  ([`chdir`](https://www.php.net/manual/en/install.fpm.configuration.php)).

### D. `PHP_VALUE` / `PHP_ADMIN_VALUE`: the change-mode table is not a gate

`fastcgi_ini_parser` parses the param as INI text and applies each entry with
`fpm_php_apply_defines_ex(&kv, mode)`, where `PHP_VALUE` uses `ZEND_INI_USER` and
`PHP_ADMIN_VALUE` uses `ZEND_INI_SYSTEM`. That function:

- special-cases `extension` → `php_dl(value, MODULE_PERSISTENT, …)`;
- special-cases `disable_functions` / `disable_classes` → `zend_disable_functions` /
  `zend_disable_classes`;
- otherwise calls the directive's `on_modify` **directly** through
  `fpm_php_zend_ini_alter_master()` — which never compares `ini_entry->modifiable` and
  assigns `ini_entry->value` unconditionally, raising `modifiable` to `SYSTEM` only when the
  mode is `SYSTEM`.

Measured consequences on 8.4.27:

- `PHP_VALUE=allow_url_include=1` → `ini_get('allow_url_include') === '1'` even though the
  manual lists it `INI_SYSTEM`
  ([allow_url_include](https://www.php.net/manual/en/filesystem.configuration.php#ini.allow-url-include),
  [change modes](https://www.php.net/manual/en/configuration.changes.modes.php)).
- `PHP_VALUE=auto_prepend_file=…` → the file runs before every script.
- `PHP_VALUE=allow_url_include=1\nauto_prepend_file=php://input` with `REQUEST_METHOD=POST`,
  a matching `CONTENT_LENGTH` and a PHP body → the body executed (`PWNED-CHAIN`). Note the
  body is only visible to `php://input` when `CONTENT_LENGTH` says so; with the param absent
  the chain silently does nothing (measured).
- `PHP_ADMIN_VALUE=open_basedir=/etc` → `ini_get('open_basedir') === '/etc'`.
- `PHP_VALUE=disable_functions=exec` changed nothing (`disable_functions` is read once at
  module init), and an **unknown** directive is logged at `ZLOG_ERROR`
  (`Passing INI directive through FastCGI: unable to set '<key>'`) while the request
  continues — there is no rejection path in `fastcgi_ini_parser` (source; the log line was
  not provoked in a probe).
- **Persistence**: after one request carrying `PHP_VALUE=auto_prepend_file=…`, the *next*
  request on a fresh connection with no `PHP_VALUE` still ran the prepend, on the same
  worker pid, and `ini_get` still reported it. `pm.max_requests = 0` (the default) means the
  worker is never recycled, so the poisoning lasts until FPM restarts. Measured across five
  requests; a restart cleared it.
- `allow_url_include` is **deprecated but not removed** (`E_DEPRECATED:
  "Directive 'allow_url_include' is deprecated"` on 8.4) — still settable, still enforced.

This is the single largest delta between "the manual's change-mode table" and what a
FastCGI client can do: the table describes `php.ini`/`ini_set` paths, not the FPM param path.

### E. CVE-2012-1823 and CVE-2024-4577

**CVE-2012-1823** — `php-cgi` before 5.3.12/5.4.2 passed a query string without `=` to the
CGI binary's argv, so `?-d allow_url_include=1 -d auto_prepend_file=php://input` became
command-line options
([NVD](https://nvd.nist.gov/vuln/detail/CVE-2012-1823),
[bug #61910](https://bugs.php.net/bug.php?id=61910),
[5.x changelog, 2012-05-03](https://www.php.net/ChangeLog-5.php)). PHP's own note says
"people using mod_php or php-fpm are not affected"
([archive 2012-05-06](https://www.php.net/archive/2012.php#id2012-05-06-1)). The fix is
`skip_getopt` in `sapi/cgi/cgi_main.c`: if `QUERY_STRING` has no `=`, is not empty, and
begins with `-` after URL-decoding, PHP does not parse argv at all. Measured on patched
8.4.27: `QUERY_STRING=-d allow_url_include=1 -d auto_prepend_file=php://input` arrived at
the script as a literal query string and `index.php` ran normally; the same options passed
as real `-d` arguments did prepend (`php://input`) — i.e. the argv path itself is intact,
only the query-string route is closed.

**CVE-2024-4577** — Windows `php-cgi` under Apache: Windows "Best-Fit" mapping replaces
`0xAD` (soft hyphen) with `-` in the command line, bypassing the leading-hyphen check, for
code pages 932/936/950 (and 65001 in the follow-up). Fixed in 8.1.29 / 8.2.20 / 8.3.8 on
2024-06-06 ([NVD](https://nvd.nist.gov/vuln/detail/CVE-2024-4577),
[GHSA-3qgc-jrrr-25jv](https://github.com/php/php-src/security/advisories/GHSA-3qgc-jrrr-25jv),
[8.3.8 changelog](https://www.php.net/ChangeLog-8.php#8.3.8)).

**A Linux php-fpm listener is not affected.** The vulnerable path needs (a) the Windows
ANSI→UCS-2 conversion that performs best-fit substitution and (b) `php-cgi`'s argv parsing.
Linux has no such conversion, and php-fpm never turns `QUERY_STRING` into argv — it only
assigns it to `SG(request_info).query_string`. Measured: the `%AD`-prefixed query string on
Linux `php-cgi` 8.4.27 reached the script unchanged, no ini change. It is still worth a
regression test because the 2024 fix was bypassed within three months
(**CVE-2024-8926**, and the related **CVE-2024-8927** `cgi.force_redirect` env collision,
both fixed 2024-09-26 in 8.3.12,
[GHSA-p99j-rfp4-xqvq](https://github.com/php/php-src/security/advisories/GHSA-p99j-rfp4-xqvq),
[GHSA-94p6-54jq-9mwp](https://github.com/php/php-src/security/advisories/GHSA-94p6-54jq-9mwp),
[8.3.12 changelog](https://www.php.net/ChangeLog-8.php#8.3.12)): a harness that pins the
"query string is data, never argv" invariant catches a regression wherever it reappears.

### F. httpoxy (`Proxy:` → `HTTP_PROXY`) under FPM

RFC 3875 maps the request `Proxy:` header to the `HTTP_PROXY` environment variable, which
many HTTP clients honour as an egress proxy ([httpoxy.org](https://httpoxy.org/)). PHP's fix
(CVE-2016-5385, [bug #72573](https://bugs.php.net/bug.php?id=72573)) makes `sapi_getenv()`
return NULL for `HTTP_PROXY` and deletes it from `$_SERVER`/`$_ENV` unless the *process*
environment really has one (`check_http_proxy` in `main/php_variables.c`).

Measured on php-fpm 8.4.27: sending `HTTP_PROXY=http://attacker:8080` and
`Proxy=http://attacker:8080` as params produced

```
SERVER_HTTP_PROXY=NULL            # deleted by check_http_proxy
SERVER_Proxy='http://attacker:8080'   # a bare name is just another $_SERVER key
getenv_HTTP_PROXY=false           # FastCGI params are never exported to the process env
```

So over FastCGI the classic vector is already broken on two counts: params do not become
environment variables, and `HTTP_PROXY` is filtered from `$_SERVER`. It comes back if the
pool itself sets `env[HTTP_PROXY]` (a real egress proxy), because then
`getenv("HTTP_PROXY")` is set and the filter leaves the request-supplied value alone —
**unverified** (needs a pool with `env[HTTP_PROXY]` plus one request).

### G. `REDIRECT_STATUS` and `DOCUMENT_ROOT`

- `REDIRECT_STATUS` exists for `php-cgi` compiled with `--enable-force-cgi-redirect`:
  without it the binary prints "Security Alert! The PHP CGI cannot be accessed directly."
  (measured on `php:8.4-cli`) ([cgi.force_redirect](https://www.php.net/manual/en/ini.core.php#ini.cgi.force-redirect),
  [case 2](https://www.php.net/manual/en/security.cgi-bin.force-redirect.php)). php-fpm
  contains no `force_redirect` reference; the baseline request without `REDIRECT_STATUS`
  served normally (measured). It is not a security control.
- `DOCUMENT_ROOT` is read for `PATH_TRANSLATED` arithmetic and for `.user.ini` discovery:
  `php_cgi_ini_activate_user_config` walks from the script's directory up to
  `DOCUMENT_ROOT` loading `.user.ini`
  ([`.user.ini files`](https://www.php.net/manual/en/configuration.file.per-user.php)).
  Measured: `DOCUMENT_ROOT=/app/fake` with `/app/fake/.user.ini` containing
  `auto_prepend_file=/app/www/prepend.php` ran the prepend and then `x.php`
  (`PREPEND-RAN FAKE-RAN`). `.user.ini` only honours `INI_ALL`/`INI_PERDIR`/`INI_USER`
  entries, and `auto_prepend_file` is `INI_PERDIR`.

### H. Duplicates, unknown names, NUL, size

- `fcgi_hash_set` overwrites an existing key's value, and `fcgi_hash_get` returns that
  bucket — so php-fpm's **own** named lookups see the last occurrence. Measured:
  `SCRIPT_FILENAME` bad-then-good executed the good one; good-then-bad 404'd;
  `QUERY_STRING=a=1` then `b=2` gave `$_GET` key `b`; `REQUEST_METHOD=POST` then `GET`
  served a GET.
- A param that only lands in `$_SERVER` keeps the **first** occurrence: sending
  `FOO=first`, `FOO=second` printed `FOO='first'`, and `foo` (lower case) was a separate
  entry (`foo='lower'`) — names are case-sensitive and duplicates are a real
  parser-differential surface. Why the two paths disagree was not traced through
  `fcgi_hash_apply`/`php_register_variable_ex` — **unverified**; the observation is from
  two requests.
- Every param becomes a `$_SERVER` key with no `HTTP_` prefixing and no filtering: a bare
  `X_TEST` appears as `X_TEST`, an `HTTP_X_TEST` as `HTTP_X_TEST` (measured). Name mangling
  then applies PHP's usual rules (leading spaces stripped, `.`/space → `_`, `[` →
  array syntax, `GLOBALS` rejected) in `php_register_variable_ex`.
- NUL: php-fpm keeps the binary name in its hash but `$_SERVER` registration truncates at
  the NUL, and a NUL in `SCRIPT_FILENAME` truncates the C string used for path handling.
  Measured: `shell.jpg\0.php` was gated as `shell.jpg` (403 under `.php`) and
  `index.php\0.jpg` executed `index.php`. **Reject `\0` in any param.**
- Size: names and values are 4-byte-length encodable; the only bound is the 65 535-byte
  record (and `memory_limit`). A 60 000-byte param name was accepted and the request still
  served (measured). No param-count limit exists in php-fpm.

### I. How the front controllers consume the params

| Param | PHP core | Symfony | Laravel | Slim / PSR-15 (Diactoros) |
|---|---|---|---|---|
| `SCRIPT_FILENAME` | **yes**: script path + `limit_extensions` | `prepareBaseUrl` (basename compare with `SCRIPT_NAME`) | inherits Symfony | no |
| `SCRIPT_NAME` | `PHP_SELF` under `fix_pathinfo` | `prepareBaseUrl` | inherits | no |
| `PATH_INFO` | used when `fix_pathinfo=1` | no | no | no |
| `REQUEST_URI` | no (pass-through) | **yes**: `prepareRequestUri` | inherits | **yes**: `marshalRequestPath` |
| `QUERY_STRING` | **yes** → `$_GET` | yes | yes | yes |
| `REQUEST_METHOD` | **yes** (POST gate) | `getMethod`, default `GET` | inherits | default `GET` |
| `CONTENT_TYPE`/`LENGTH` | **yes** (POST body) | yes | yes | yes |
| `SERVER_PROTOCOL` | no (`proto_num = 1000`) | protocol string | inherits | default `1.1` |
| `DOCUMENT_ROOT` | `PATH_INFO` derivation, `.user.ini` | no | no | no |
| `REDIRECT_STATUS` | php-cgi only | no | no | no |

- Symfony: `public/index.php` returns a closure; `symfony/runtime` calls
  `Request::createFromGlobals()`, which passes `$_SERVER` into
  [`Request::prepareRequestUri`](https://github.com/symfony/http-foundation/blob/7.3/Request.php)
  (`REQUEST_URI`, else `ORIG_PATH_INFO`+`QUERY_STRING`, else `''`).
- Laravel: `public/index.php` ends with `$app->handleRequest(Request::capture())`;
  `Request::capture()` is `createFromBase(SymfonyRequest::createFromGlobals())`
  ([`Request.php`](https://github.com/laravel/framework/blob/12.x/src/Illuminate/Http/Request.php)).
- Slim 4: `AppFactory::create()` → `ServerRequestCreatorFactory` → the first installed
  provider, Laminas Diactoros by default, whose `fromGlobals()` builds the URI from
  `REQUEST_URI` ([`UriFactory`](https://github.com/laminas/laminas-diactoros/blob/3.6.x/src/UriFactory.php),
  [`ServerRequestFactory`](https://github.com/laminas/laminas-diactoros/blob/3.6.x/src/ServerRequestFactory.php)).
- Net: **no mainstream front controller reads `PATH_INFO` or `SCRIPT_NAME` to route**;
  routing is `REQUEST_URI`. `SCRIPT_FILENAME` matters only because PHP executes it.

### J. What stock nginx sets (contrast only)

`conf/fastcgi_params` itself is 19 `fastcgi_param` lines —
`QUERY_STRING`, `REQUEST_METHOD`, `CONTENT_TYPE`, `CONTENT_LENGTH`, `SCRIPT_NAME`,
`REQUEST_URI`, `DOCUMENT_URI`, `DOCUMENT_ROOT`, `SERVER_PROTOCOL`, `REQUEST_SCHEME`,
`HTTPS`, `GATEWAY_INTERFACE`, `SERVER_SOFTWARE`, `REMOTE_ADDR`, `REMOTE_PORT`,
`SERVER_ADDR`, `SERVER_PORT`, `SERVER_NAME`, `REDIRECT_STATUS 200`
([`conf/fastcgi_params`](https://github.com/nginx/nginx/blob/master/conf/fastcgi_params)) —
and **does not contain `SCRIPT_FILENAME`**; that is the first line of `conf/fastcgi.conf`
([`conf/fastcgi.conf`](https://github.com/nginx/nginx/blob/master/conf/fastcgi.conf)).

The canonical PHP location block adds it back from a server-side variable, and that is the
whole mechanism that keeps a client from naming it:

```nginx
location ~ \.php$ {
    try_files $uri =404;
    fastcgi_split_path_info ^(.+\.php)(/.*)$;
    include fastcgi_params;
    fastcgi_param SCRIPT_FILENAME $realpath_root$fastcgi_script_name;
    fastcgi_param PATH_INFO $fastcgi_path_info;   # Symfony/Slim only
    fastcgi_pass unix:/run/php/php8.3-fpm.sock;
}
```

`fastcgi_split_path_info` "should have two captures: the first becomes `$fastcgi_script_name`,
the second becomes `$fastcgi_path_info`"
([nginx](https://nginx.org/en/docs/http/ngx_http_fastcgi_module.html#fastcgi_split_path_info)),
`try_files` with `=404` is what stops `fix_pathinfo`
([nginx](https://nginx.org/en/docs/http/ngx_http_core_module.html#try_files),
[ticket #1810](https://trac.nginx.org/nginx/ticket/1810)). Symfony's recipe additionally
marks the front controller `internal` and 404s every other `.php`
([Symfony](https://symfony.com/doc/current/setup/web_server_configuration.html)); Laravel's
uses `try_files $uri $uri/ /index.php?$query_string` and no `PATH_INFO`
([Laravel deployment](https://github.com/laravel/docs/blob/12.x/deployment.md)).

## Comparison Table: what the defaults already stop

| Default | Value | Stops | Does not stop |
|---|---|---|---|
| `security.limit_extensions` | `.php .phar` | `x.jpg`, `x.php.jpg`, `x.jpg/x.php` | any uploaded/planted `.php`; empty value disables it |
| `allow_url_include` | `0` (deprecated, `INI_SYSTEM`) | `php://input`, `data://` via `include` | a local-path `auto_prepend_file` |
| `allow_url_fopen` | `1` | nothing related | — |
| `cgi.fix_pathinfo` | `1` | nothing; it *creates* the truncation hole | `x.jpg/x.php` when extensions are empty |
| `doc_root` | empty | nothing | — (set it to gain containment) |
| `clear_env` | `yes` | leaking the shell environment into workers | FastCGI params, which are not env vars |
| `PHP_VALUE` acceptance | unconditional | nothing | — |
| `.user.ini` | honoured up to `DOCUMENT_ROOT` | — | a spoofed `DOCUMENT_ROOT` selecting another tree |

## Unverified / Open Questions

- **Why duplicates differ** (`SCRIPT_FILENAME` last-wins, `FOO` first-wins): observed twice,
  not traced through `fcgi_hash_apply` → `php_register_variable_ex`. A patch-level reading
  of `ZEND_INI_STAGE`/`zend_hash_update_ind` and a debug build printing `$_SERVER` in
  insertion order would settle it.
- **`doc_root` containment was not measured**: the manual promises no execution outside it.
  A probe with `doc_root=/app/www` and `SCRIPT_FILENAME=/app/elsewhere/evil.php` would show
  whether it 404s or ignores `path_translated` (the source says the latter when `doc_root`
  is absolute).
- **`PHP_ADMIN_VALUE` and `.user.ini` interaction** (does an attacker `.user.ini` override a
  pinned admin value?) was not tested; `modifiable = SYSTEM` suggests no.
- **`env[HTTP_PROXY]` plus a request `HTTP_PROXY` param** — the one configuration where
  httpoxy could return; not built.
- **`pm.max_requests` bounds the `PHP_VALUE` persistence window**; the exact behaviour at
  the recycle boundary (does the request that hits the limit still see the poisoned ini?)
  was not measured.
- **Exact fixed versions for CVE-2024-8926** beyond 8.3.12 were not enumerated (only the
  8.3.12 changelog was read).
- **Windows php-cgi behaviour** (the 4577 trigger) is from the advisory only; no Windows
  probe was run, and none is planned.
- **PHP 8.5** was not available in the image; the `PHP_VALUE` code path is unchanged since
  8.3 as far as the 8.4 tree shows, but a 8.5 run would confirm.
- **No `php-fpm` on this host**: all php-fpm measurements are `php:8.4-fpm-alpine`
  (8.4.27), not a distro package; a distro may ship a different `php.ini`
  (`variables_order`, `allow_url_fopen`) that changes `$_ENV`/`$_SERVER` contents.

## Open questions for the design

- Refuse `PHP_VALUE`/`PHP_ADMIN_VALUE` outright, or forward a fixed allowlist of directives?
  (The measured persistence means "filter once" is not a per-request mitigation.)
- A single request's `PHP_VALUE` poisons the worker until restart (measured): should the
  param set be owned by the harness rather than filtered per request?
- Is `PATH_INFO` ever needed? No surveyed framework routes on it, and a client that omits it
  still gets `fix_pathinfo` truncation (`PATH_INFO` unset, `SCRIPT_FILENAME` truncated) — but
  a front controller that reads `$_SERVER['PATH_INFO']` itself would silently change
  behaviour if it were dropped.
- Pin `doc_root` in the pool file (server-side containment) or compute `SCRIPT_FILENAME`
  from a validated route path (client-side)? The manual promises more for `doc_root`; the
  client-side answer needs no pool edit.
- Does a route ever legitimately need a `.phar` entry script? The default allows it.
- How should duplicate params be handled — reject the request, or keep the first (what
  `$_SERVER` does) and assert the by-name lookups agree?
- `pm.max_requests` is the only thing that bounds a poisoned worker today; should the pool
  file Blastit writes set it, or is that the developer's operational choice?
- CVE-2024-4577 has no Linux FPM path: is it a regression test at the *param* layer (assert
  no query byte ever reaches argv) or only a scanner rule?

## Sources

- php-fpm / php-src (8.4):
  [`sapi/fpm/fpm/fpm_main.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_main.c)
  (`init_request_info`, `is_valid_path`, `sapi_cgi_activate`, `fastcgi_ini_parser`,
  `sapi_cgi_register_variables`),
  [`sapi/fpm/fpm/fpm_php.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_php.c)
  (`fpm_php_limit_extensions`, `fpm_php_apply_defines_ex`,
  `fpm_php_zend_ini_alter_master`),
  [`sapi/fpm/fpm/fpm_conf.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_conf.c),
  [`sapi/fpm/fpm/fpm_env.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_env.c),
  [`main/fastcgi.c`](https://github.com/php/php-src/blob/PHP-8.4/main/fastcgi.c),
  [`main/php_variables.c`](https://github.com/php/php-src/blob/PHP-8.4/main/php_variables.c),
  [`main/fopen_wrappers.c`](https://github.com/php/php-src/blob/PHP-8.4/main/fopen_wrappers.c),
  [`main/streams/streams.c`](https://github.com/php/php-src/blob/PHP-8.4/main/streams/streams.c),
  [`sapi/cgi/cgi_main.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/cgi/cgi_main.c),
  [`main/main.c`](https://github.com/php/php-src/blob/PHP-8.4/main/main.c).
- PHP manual: [CGI security](https://www.php.net/manual/en/security.cgi-bin.php),
  [case 2 force-redirect](https://www.php.net/manual/en/security.cgi-bin.force-redirect.php),
  [case 3 doc_root](https://www.php.net/manual/en/security.cgi-bin.doc-root.php),
  [change modes](https://www.php.net/manual/en/configuration.changes.modes.php),
  [`.user.ini`](https://www.php.net/manual/en/configuration.file.per-user.php),
  [`cgi.fix_pathinfo`](https://www.php.net/manual/en/ini.core.php#ini.cgi.fix-pathinfo),
  [`cgi.force_redirect`](https://www.php.net/manual/en/ini.core.php#ini.cgi.force-redirect),
  [`doc_root`](https://www.php.net/manual/en/ini.core.php#ini.doc-root),
  [`user_dir`](https://www.php.net/manual/en/ini.core.php#ini.user-dir),
  [`auto_prepend_file`](https://www.php.net/manual/en/ini.core.php#ini.auto-prepend-file),
  [`allow_url_include`](https://www.php.net/manual/en/filesystem.configuration.php#ini.allow-url-include),
  [`open_basedir`](https://www.php.net/manual/en/ini.core.php#ini.open-basedir),
  [`disable_functions`](https://www.php.net/manual/en/ini.core.php#ini.disable-functions),
  [FPM configuration](https://www.php.net/manual/en/install.fpm.configuration.php).
- CVEs / advisories: [CVE-2012-1823](https://nvd.nist.gov/vuln/detail/CVE-2012-1823),
  [bug #61910](https://bugs.php.net/bug.php?id=61910),
  [PHP archive 2012-05-06](https://www.php.net/archive/2012.php#id2012-05-06-1),
  [CVE-2024-4577](https://nvd.nist.gov/vuln/detail/CVE-2024-4577),
  [GHSA-3qgc-jrrr-25jv](https://github.com/php/php-src/security/advisories/GHSA-3qgc-jrrr-25jv),
  [CVE-2024-8926](https://nvd.nist.gov/vuln/detail/CVE-2024-8926),
  [GHSA-p99j-rfp4-xqvq](https://github.com/php/php-src/security/advisories/GHSA-p99j-rfp4-xqvq),
  [GHSA-94p6-54jq-9mwp](https://github.com/php/php-src/security/advisories/GHSA-94p6-54jq-9mwp),
  [CVE-2016-5385](https://nvd.nist.gov/vuln/detail/CVE-2016-5385),
  [bug #72573](https://bugs.php.net/bug.php?id=72573),
  [httpoxy.org](https://httpoxy.org/),
  [8.3.8 changelog](https://www.php.net/ChangeLog-8.php#8.3.8),
  [8.3.12 changelog](https://www.php.net/ChangeLog-8.php#8.3.12),
  [5.x changelog](https://www.php.net/ChangeLog-5.php).
- Frameworks:
  [Symfony `Request.php`](https://github.com/symfony/http-foundation/blob/7.3/Request.php),
  [Symfony nginx recipe](https://symfony.com/doc/current/setup/web_server_configuration.html),
  [Laravel `Request.php`](https://github.com/laravel/framework/blob/12.x/src/Illuminate/Http/Request.php),
  [Laravel deployment](https://github.com/laravel/docs/blob/12.x/deployment.md),
  [Slim web servers](https://www.slimframework.com/docs/v4/start/web-servers.html),
  [Diactoros `ServerRequestFactory`](https://github.com/laminas/laminas-diactoros/blob/3.6.x/src/ServerRequestFactory.php),
  [Diactoros `UriFactory`](https://github.com/laminas/laminas-diactoros/blob/3.6.x/src/UriFactory.php).
- nginx: [`conf/fastcgi_params`](https://github.com/nginx/nginx/blob/master/conf/fastcgi_params),
  [`conf/fastcgi.conf`](https://github.com/nginx/nginx/blob/master/conf/fastcgi.conf),
  [`ngx_http_fastcgi_module`](https://nginx.org/en/docs/http/ngx_http_fastcgi_module.html),
  [`try_files`](https://nginx.org/en/docs/http/ngx_http_core_module.html#try_files),
  [ticket #1810](https://trac.nginx.org/nginx/ticket/1810).

License note: no third-party code is vendored or linked by this note. PHP (PHP License
3.01), nginx (BSD-2-Clause) and the frameworks (MIT) are referenced, not shipped; the probe
client is stdlib Python.
