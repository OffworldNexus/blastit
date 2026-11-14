# FastCGI 1.0 on the wire, and running `php-fpm` as a non-root developer

- Node: MB.3.1   - Author: researcher   - Date: 2026-11-14

## TL;DR (<= 8 lines)

- A FastCGI record is an 8-byte header (`version`, `type`, big-endian `requestId`,
  big-endian `contentLength`, `paddingLength`, `reserved`) plus content plus padding to
  an 8-byte boundary ([spec §3.3](https://fastcgi-archives.github.io/FastCGI_Specification.html)).
- Name-value lengths are 1 byte below 128, else 4 bytes with the high bit set; a stream
  ends with a zero-length record, so `PARAMS` ends with `contentLength == 0`
  ([§3.4](https://fastcgi-archives.github.io/FastCGI_Specification.html)).
- **php-fpm never multiplexes**: it answers `FCGI_MPXS_CONNS = 0` to `GET_VALUES`, serves
  one request id at a time per connection, and ignores `requestId == 0`
  ([php-src `fastcgi.c`](https://github.com/php/php-src/blob/PHP-8.4/main/fastcgi.c)).
- **A name-value pair must not straddle two `PARAMS` records**: php-fpm parses each
  record's content independently and closes the connection on a mid-pair split
  (verified), so one `name+value` is capped at ~65531 bytes.
- `contentLength` is a u16 and php-fpm rejects `contentLength + paddingLength > 65535`;
  STDOUT is chunked at 8184 bytes (`8 KiB` buffer minus header) — measured.
- `END_REQUEST.appStatus` is **always 0** and `reserved[3]` is **uninitialised buffer
  residue** in php-fpm, not zeros (measured: the last three bytes of the script output).
- Non-root `php-fpm -F -y pool.conf` works, but `listen.owner`/`listen.group` make it
  **abort** with `failed to chown() the socket … Operation not permitted`, and
  `user`/`group` are ignored with a NOTICE (measured on PHP 8.4.27).
- A front controller needs only `SCRIPT_FILENAME` (hard) and `REQUEST_METHOD`
  (`SERVER_PROTOCOL` is never parsed by FPM); Symfony/Laravel/Slim route on
  `REQUEST_URI`, not `PATH_INFO`.

## Developer Experience Implications

- **Two params are load-bearing.** php-fpm 404s without `SCRIPT_FILENAME` and silently
  drops the request (empty 200 headers, no script run) without `REQUEST_METHOD`
  (measured; `fpm_main.c::init_request_info`). Everything else a modern front controller
  needs — `REQUEST_URI`, `QUERY_STRING`, `CONTENT_LENGTH`, `CONTENT_TYPE`,
  `SERVER_PROTOCOL` — comes from the HTTP request the developer already has, so a
  manifest that names `root` and the entry script is enough to build the whole `PARAMS`
  set ([Symfony `prepareRequestUri`](https://github.com/symfony/http-foundation/blob/7.3/Request.php),
  [Laravel `Request::capture`](https://github.com/laravel/framework/blob/12.x/src/Illuminate/Http/Request.php)).
- **One connection per request, `requestId = 1`, `keepConn = 0`** matches what nginx does
  (`request_id_lo = 1` hard-coded, `fastcgi_keep_conn off` by default) and what Apache
  does ("we don't support that, and always use a value of '1'"), so it is the behaviour
  php-fpm is exercised against in the wild
  ([nginx module](https://github.com/nginx/nginx/blob/master/src/http/modules/ngx_http_fastcgi_module.c),
  [mod_proxy_fcgi](https://github.com/apache/httpd/blob/trunk/modules/proxy/mod_proxy_fcgi.c)).
- **`php-fpm -F -y` needs no root and no init system**, and `-t` validates the pool file
  without binding the socket (measured). The only non-obvious hazard is a pool file copied
  from a root deployment: `listen.owner`/`listen.group` turn into a fatal `chown` error as
  a normal user (measured). Omit them; the socket inherits the developer's uid and
  `listen.mode`.
- **`clear_env = yes` (the default) is the 12-factor answer**: the worker inherits none of
  the shell's environment, so `HTTP_PROXY`, `DATABASE_URL` and friends cannot leak in by
  accident; only `env[…]` entries and `USER`/`HOME` survive
  ([pool directives](https://www.php.net/manual/en/install.fpm.configuration.php),
  [`fpm_env.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_env.c)).
- **A request that fits in one `PARAMS` record has no framing subtleties.** Keeping the
  whole param set under 65 535 bytes means no splitting, and php-fpm tolerates a missing
  empty-`PARAMS` terminator when a `STDIN` record follows (measured) — but a client should
  still send it, because that is the only thing nginx and the spec ever do.

## Technical Findings

Probe environment: PHP **8.4.27 (fpm-fcgi, NTS)**, `php:8.4-fpm-alpine`, `php-fpm` started
as a **non-root** uid 1000 master with the pool file in §H; the client is a ~200-line
stdlib Python FastCGI implementation (`/tmp/opencode/php-probe/fcgi.py`) that builds records
by hand so every byte is visible. `php-fpm` is **not installed on this host**, so all
php-fpm measurements are from that container; php-src citations are `PHP-8.4`.

### A. Header layout (§3.3, §8)

| Offset | Width | Field | Notes |
|---|---|---|---|
| 0 | 1 | `version` | `1`; php-fpm rejects `hdr.version < FCGI_VERSION_1` (`fastcgi.c`, `fcgi_read_request`) |
| 1 | 1 | `type` | §B |
| 2 | 2 | `requestId` | big-endian u16; `0` = management records (§3.3 "Managing Request IDs") |
| 4 | 2 | `contentLength` | big-endian u16, **max 65535** |
| 6 | 1 | `paddingLength` | u8, 0…255; bytes are ignored |
| 7 | 1 | `reserved` | always 0 |

php-fpm's own writer pads every record so `len + padding` is a multiple of 8
(`fcgi_make_header`), matching the spec's recommendation: "We recommend that records be
placed on boundaries that are multiples of eight bytes"
([§3.3](https://fastcgi-archives.github.io/FastCGI_Specification.html)).

### B. Record types a client must know (§8, Appendix A)

| Type | Value | Direction | php-fpm |
|---|---|---|---|
| `FCGI_BEGIN_REQUEST` | 1 | client → app | required, `contentLength == 8` |
| `FCGI_ABORT_REQUEST` | 2 | client → app | **not implemented** (`fastcgi.h`: `/* [in] (not supported) */`) |
| `FCGI_END_REQUEST` | 3 | app → client | always 8 bytes |
| `FCGI_PARAMS` | 4 | client → app | stream, terminated by a zero-length record |
| `FCGI_STDIN` | 5 | client → app | stream, terminated by a zero-length record |
| `FCGI_STDOUT` | 6 | app → client | stream of ≤8184-byte records |
| `FCGI_STDERR` | 7 | app → client | only with `catch_workers_output = yes` |
| `FCGI_DATA` | 8 | client → app | **not supported** (`fcgi_read_request` never reads it) |
| `FCGI_GET_VALUES` | 9 | client → app | answered, optional to use (§I) |
| `FCGI_GET_VALUES_RESULT` | 10 | app → client | |
| `FCGI_UNKNOWN_TYPE` | 11 | app → client | never sent by php-fpm |

A bad role value makes `fcgi_read_request` `return 0`, which closes the connection
**without** `FCGI_END_REQUEST{UNKNOWN_ROLE}` — a §5.5 deviation worth tolerating.

### C. `BEGIN_REQUEST` (§5.1)

Body is exactly 8 bytes: `role` (u16), `flags` (u8), `reserved[5]`.

- Roles: `FCGI_RESPONDER = 1`, `FCGI_AUTHORIZER = 2`, `FCGI_FILTER = 3`. php-fpm accepts
  all three but only injects `FCGI_ROLE` and then behaves as a responder.
- `flags & FCGI_KEEP_CONN (1)` decides connection lifetime: clear ⇒ php-fpm closes after
  `END_REQUEST` (measured: `recv` returns `b''`); set ⇒ it loops for the next
  `BEGIN_REQUEST` (measured: a second id was served on the same socket).
- Keep-alive also ends when `pm.max_requests` is reached (`fcgi_request_set_keep(request, 0)`).

### D. Name-value encoding, padding, terminator (§3.4)

> "FastCGI transmits a name-value pair as the length of the name, followed by the length of
> the value, followed by the name, followed by the value. Lengths of 127 bytes and less can
> be encoded in one byte, while longer lengths are always encoded in four bytes. The
> high-order bit of the first byte of a length indicates the length's encoding."

So `0x7F` = 127 (one byte); 128 is `80 00 00 80`; the four-byte form carries 31 bits.
Name and value lengths are independent. **There is no per-pair padding** — padding is a
record property, and the padding bytes are counted in `paddingLength`, not in
`contentLength`.

A stream is "a series of zero or more non-empty records … followed by an empty record", so
`PARAMS` must end with a `contentLength == 0` record. php-fpm also accepts a request whose
`PARAMS` is followed directly by `STDIN` (measured), because its read loop exits when the
next header is not `FCGI_PARAMS`; do not rely on that.

### E. `requestId`: one at a time, no multiplexing

- The spec reserves `0` for management records and says application records "have a nonzero
  requestId" (§3.3). php-fpm does not enforce it: `req->id = (hdr.requestIdB1 << 8) +
  hdr.requestIdB0;` with no zero check, and a `BEGIN_REQUEST` with id `0` was served
  normally (measured, `END_REQUEST` came back with id 0).
- **php-fpm does not multiplex.** `fcgi_init` registers exactly one management variable,
  `FCGI_MPXS_CONNS = "0"` (`fastcgi.c`), and `GET_VALUES` answered only
  `FCGI_MPXS_CONNS=0` — it omits `FCGI_MAX_CONNS`/`FCGI_MAX_REQS` (measured). The worker
  loop is `while (fcgi_accept_request(request) >= 0) { … one request … }`, so a second
  `BEGIN_REQUEST` sent before the first `END_REQUEST` is simply not read yet.
- **Pipelining works, concurrency does not.** With `BEGIN_REQUEST`/`PARAMS`/`STDIN` for
  ids 1 and 2 written back-to-back and `KEEP_CONN` set, php-fpm answered id 1 completely
  (`STDOUT 518`, `END_REQUEST`) and only then id 2 completely (`STDOUT 8184`, `STDOUT 8184`,
  `STDOUT 3700`, `END_REQUEST`) — measured, no interleaving and no `CANT_MPX_CONN`.
  Without `KEEP_CONN` the connection closes after id 1 and id 2 is lost.
- **Ids are never exhausted in practice.** nginx hard-codes `request_id_lo = 1` in its
  static `BEGIN_REQUEST`/`PARAMS`/`STDIN` templates and Apache states "Request IDs are
  arbitrary … but we don't support that, and always use a value of '1'". A client that
  reuses a connection should wrap `65535 → 1`, never `0`.

### F. Size limits, and the split rule that bites

- `contentLength` is u16 ⇒ 65 535 max; `paddingLength` u8 ⇒ 255. php-fpm rejects a record
  when `len + padding > FCGI_MAX_LENGTH (0xffff)` (`fastcgi.c`, `fcgi_read_request`), i.e.
  **CL + PL ≤ 65535**, so a maximum-size record must have zero padding.
- **`PARAMS` records are concatenated at pair granularity only.** `fcgi_get_params` walks
  one record's content with `while (p < end)` and returns 0 ("Malformed request") when a
  length pair runs past `end`, which resets the connection. Measured: splitting the params
  body **on a pair boundary** worked; splitting 3 bytes past it produced
  `ECONNRESET`. Consequence: one `name + value` (including its 1–8 length bytes) must fit
  in a single record, so a single param is capped at ~65 531 bytes even though the total
  param set may span many records.
- php-fpm imposes **no** cap on param count or total `PARAMS` bytes; the practical ceilings
  are `memory_limit` and the copy into `$_SERVER`. `post_max_size` is not a FastCGI limit
  (`main/SAPI.c` compares `content_length` to it when building `php://input`).
- **Responses are chunked at 8184 bytes.** `out_buf` is 8 KiB and `fcgi_write` fills it
  header-first; a 20 000-byte response arrived as `STDOUT 8184`, `STDOUT 8184`,
  `STDOUT 3700` (+4 padding), `END_REQUEST 8` (measured). There is **no** zero-length
  `STDOUT` terminator — php-fpm deviates from §5.3 here, and nginx/Apache stop at
  `END_REQUEST` anyway.
- nginx does **not** chunk `PARAMS` into 8 KiB pieces: it encodes all params into one
  buffer and errors with "fastcgi request record is too big" above 65 535. Apache chunks at
  16 KiB per record *between* pairs.

### G. `STDOUT` then `END_REQUEST`; what the 8 bytes really contain

- Ordering (§6.2): all stdout/stderr first, then `END_REQUEST`. php-fpm's `fcgi_flush(req,
  end=1)` closes the current `STDOUT` packet, then appends one `END_REQUEST` with
  `contentLength = 8`.
- Body layout: `appStatus` (u32), `protocolStatus` (u8), `reserved[3]`.
  `protocolStatus` ∈ `REQUEST_COMPLETE 0`, `CANT_MPX_CONN 1`, `OVERLOADED 2`,
  `UNKNOWN_ROLE 3`. php-fpm only ever sends `REQUEST_COMPLETE`.
- **`appStatus` is hard-coded to 0** in `fcgi_flush` (all four bytes). A PHP fatal error is
  an HTTP 500 produced on `STDOUT` (the `Status:` header), not a non-zero `appStatus`; a
  client that treats `appStatus` as an exit code will mis-model every failure.
- **`reserved[3]` is uninitialised.** `fcgi_flush` sets `appStatusB3..B0` and
  `protocolStatus` and never touches `reserved`, so the bytes are whatever the 8 KiB
  `out_buf` held at that offset. Measured: `00 00 00 00 00 41 41 41` ("AAA" from a previous
  output line) and `00 00 00 00 00 54 3d 28` ("T=(") in different runs. The spec says
  reserved bytes are ignored; a client **must** ignore them or it will see garbage.
- `FCGI_ABORT_REQUEST` does nothing (source comment plus a probe: an immediate abort still
  produced a complete response). php-fpm notices a dead client only when a write fails.
- `FCGI_KEEP_CONN` clear ⇒ close after `END_REQUEST` (§C).

### H. Running `php-fpm` as a non-root developer (measured)

The canonical command line, with no `sudo`, no `-R` and no init system:

```console
$ php-fpm -F -y ./etc/php-fpm.conf
```

`-F` keeps it in the foreground ("Force to stay in foreground and ignore daemonize option
from configuration file"), `-y` selects the config file ("Specify alternative path to
FastCGI process manager configuration file"), `-t` tests it
([`php-fpm.8`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/php-fpm.8.in)).
`-R` (`--allow-to-run-as-root`) only relaxes the root-only checks; it is meaningless for a
non-root master and must not be needed.

The exact pool file that worked (`/app/pool-nonroot.conf`, socket and logs under
`/app/run`; `error_log = /dev/stderr` only works with `catch_workers_output = yes`):

```ini
[global]
error_log = /dev/stderr
daemonize = no
pid       = /app/run/fpm.pid

[www]
listen = /app/run/fpm.sock
listen.mode = 0660
pm = dynamic
pm.max_children = 4
pm.start_servers = 1
pm.min_spare_servers = 1
pm.max_spare_servers = 2
clear_env = yes
security.limit_extensions = .php
catch_workers_output = yes
```

started with `php-fpm -F -y /app/pool-nonroot.conf` as uid 1000. Removing
`listen.owner`/`listen.group` was the change that took it from `FPM initialization failed`
to `ready to handle connections`.

Measured facts (PHP 8.4.27, image `php:8.4-fpm-alpine`, master uid 1000):

- `php-fpm -t -y pool.conf` → `NOTICE: configuration file /app/pool.conf test is
  successful`, exit 0, **no root needed**, and it created no socket and no pid file
  (measured).
- With `user = nobody` / `group = nogroup` present, startup logs
  `NOTICE: [pool www] 'user' directive is ignored when FPM is not running as root` and the
  same for `group`, then continues — the manual's "This option is mandatory" only bites a
  **root** master (`fpm_conf_post_process` alerts when root and no user is set).
- With `listen.owner`/`listen.group` also present, startup **fails**:
  `ERROR: [pool www] failed to chown() the socket '/app/run/fpm.sock': Operation not
  permitted (1)` then `ERROR: FPM initialization failed`. Remove them; the socket is
  created with the master's uid and `listen.mode` (0660 here). This is the one trap in a
  pool file copied from a root deployment.
- Workers are still forked; only `setgid`/`setuid`, `chroot` and `process.priority` are
  skipped when not root (`fpm_unix.c`). Binding a port < 1024 fails with `EACCES`; a Unix
  socket in a writable directory does not.
- `clear_env = yes` (default) clears the environment before adding `env[…]`, `USER` and
  `HOME` (`fpm_env.c`), so the worker starts clean.
- `security.limit_extensions` defaults to `.php .phar` and compares the **suffix of the
  resolved script path** (`fpm_php_limit_extensions`), returning `403 Access denied.` —
  never a URI or `PATH_INFO`.
- `pm = dynamic` needs `pm.max_children`; `pm.min_spare_servers`/`pm.max_spare_servers` are
  "also mandatory" for dynamic, `pm.start_servers` defaults to their average, and
  `pm.max_requests` defaults to `0` (never recycle). `pm.max_requests` is also what limits
  how long a poisoned worker lives — see the `PHP_VALUE` persistence probe in
  [php2shell](php2shell.md).
- `php://input` is driven by the `CONTENT_LENGTH` **param**, not by how many bytes arrived
  on `STDIN`: with `CONTENT_LENGTH` absent, a body sent on `STDIN` is invisible to PHP
  (measured while reproducing the prepend chain).

### I. `GET_VALUES` (§4.1) — optional

`{FCGI_GET_VALUES, 0, …}` carries name-value pairs with empty values; the answer is
`{FCGI_GET_VALUES_RESULT, 0, …}` using the same encoding, omitting names it does not know.
php-fpm answers with exactly `FCGI_MPXS_CONNS = "0"` and keeps the connection open. A
client that never multiplexes does not need it; nginx and Apache never send it.

### J. One request, annotated

`BEGIN_REQUEST` id 7 (`keepConn` set), four params, no body. Real bytes from the probe
client (144 bytes, every record padded to a multiple of 8):

```
  off  header bytes                record        id  CL   PL
    0  01 01 00 07 00 08 00 00     BEGIN_REQUEST  7    8    0
       └ body: 00 01 | 01 | 00 00 00 00 00      role=RESPONDER(1), flags=KEEP_CONN(1)
   16  01 04 00 07 00 5d 03 00     PARAMS         7   93    3
       └ body (93 B):
         0f 12 "SCRIPT_FILENAME" "/app/www/index.php"   15 + 18
         0e 03 "REQUEST_METHOD"  "GET"                  14 +  3
         0f 08 "SERVER_PROTOCOL" "HTTP/1.1"             15 +  8
         0c 00 "QUERY_STRING"    ""                     12 +  0
       └ 3 padding bytes
  120  01 04 00 07 00 00 00 00     PARAMS         7    0    0   ← end of params
  128  01 05 00 07 00 00 00 00     STDIN          7    0    0   ← no body
  136  01 05 00 07 00 00 00 00     STDIN          7    0    0   ← the probe client's
                                                                 extra empty STDIN
```

The second empty `STDIN` is the probe client's habit (it writes one after the body and one
after the loop); one empty `STDIN` is what the spec asks for, and php-fpm stops at the
first. The offsets above sum to the stated 144 bytes: 16 + (8+93+3) + 8 + 8 + 8.

The server answers one `STDOUT` record (up to 8184 bytes) and one `END_REQUEST`; a
baseline request with the fuller param set answered:

```
  01 06 00 07 01 c1 07 00   STDOUT id=7 CL=449 PL=7   "X-Powered-By: PHP/8.4.27\r\n…"
  01 03 00 07 00 08 00 00   END_REQUEST id=7 CL=8 PL=0
  00 00 00 00 00 xx xx xx   appStatus=0, protocolStatus=0, reserved[3] = residue (§G)
```

Params php-fpm insists on (`init_request_info`): `SCRIPT_FILENAME` (or `PATH_TRANSLATED`
as a fallback) — absent ⇒ `404 File not found.` on `STDOUT`; `REQUEST_METHOD` — absent ⇒
the SAPI skips script execution and returns only default headers (measured). It never
parses `SERVER_PROTOCOL` (`SG(request_info).proto_num = 1000`, i.e. HTTP/1.0), and it does
not need `REDIRECT_STATUS` (that is a php-cgi `--enable-force-cgi-redirect` thing).

### K. What php-fpm does with the `SCRIPT_FILENAME` it is handed

Worth recording here because the attack note depends on it (all measured):

- Any path segment equal to `..` is refused outright by `is_valid_path()` → `404 File not
  found.` (so `SCRIPT_FILENAME=/app/www/../www/index.php` 404s even though the file
  exists, and `/app/www/../../etc/passwd` never reaches the extension check).
- `cgi.fix_pathinfo = 1` (default) makes php-fpm **stat backwards** from the end of
  `SCRIPT_FILENAME` until it finds a regular file: `SCRIPT_FILENAME=/app/www/index.php/extra`
  executed `index.php` and `$_SERVER['SCRIPT_FILENAME']` came back truncated to
  `/app/www/index.php` (measured). **Truncation alone does not set `PATH_INFO`**: that same
  request, with no `PATH_INFO` param, reported `PATH_INFO` unset and `PHP_SELF=/index.php`
  (measured; `fpm_main.c` computes `path_info = (env_path_info && pilen > slen) ? … : NULL`).
  The documented `PATH_INFO=/extra`, `PHP_SELF=/index.php/extra` result needs the client's
  usual param set (§J) **plus** a `PATH_INFO=/extra` param (measured, with or without the
  trailing `/extra` on `SCRIPT_FILENAME`): `PATH_INFO` is the front-end's value, not
  something `fix_pathinfo` invents. What `fix_pathinfo` does give an attacker is the
  truncated script path, and the extension check runs on that path: with
  `security.limit_extensions` empty, `SCRIPT_FILENAME=/app/www/shell.jpg/x.php` therefore
  executed `shell.jpg` (measured: `JPG-RAN`).
- `security.limit_extensions` is the only thing that stops that: with `.php` set, both
  `shell.jpg` and `shell.jpg/x.php` returned `403 Access denied.`
- A NUL byte truncates the value at the C boundary **before** the extension check:
  `shell.jpg\0.php` was treated as `shell.jpg` (403 under `.php`), `index.php\0.jpg` was
  treated as `index.php` (executed).

### L. Client behaviour comparison

| Client | requestId | Multiplex | `PARAMS` chunking | `keepConn` | `GET_VALUES` |
|---|---|---|---|---|---|
| nginx | always 1 | no | one record, error >65535 | off by default | never |
| Apache `mod_proxy_fcgi` | always 1 | no ("always use a value of '1'") | 16 KiB between pairs | only when reusing | never |
| A correct minimal client | 1 per request, wrap 1…65535 | no | split **between** pairs only | explicit | not needed |

## Comparison Table

| Fact | Spec | php-fpm 8.4.27 (measured) |
|---|---|---|
| Max record content | 65535 (`u16`) | 65535, and `CL+PL ≤ 65535` |
| `requestId` 0 | management only | accepted for application records |
| Multiplexing | optional, advertised by `FCGI_MPXS_CONNS` | `0` — never |
| `PARAMS` split | records concatenate | only at pair boundaries; mid-pair ⇒ reset |
| `STDOUT` terminator | zero-length record | not sent |
| `appStatus` | application exit status | always 0 |
| `reserved[3]` | ignore | uninitialised residue |
| `ABORT_REQUEST` | cancel | ignored |
| `REDIRECT_STATUS` | CGI redirect guard | not used |

## Constraints a client must satisfy (not a design)

1. Send `SCRIPT_FILENAME` and `REQUEST_METHOD`; treat a missing `SCRIPT_FILENAME` as the
   404 it is, not as a transport error.
2. Keep every `name+value` inside one record; never split a pair across records.
3. Keep `contentLength + paddingLength ≤ 65535`; pad records to 8 bytes.
4. Send the empty `PARAMS` and empty `STDIN` terminators.
5. Ignore `END_REQUEST.reserved[3]`; ignore `appStatus` as an exit code; take the HTTP
   status from the `Status:` header on `STDOUT`.
6. Expect one `STDOUT` record per ≤8184 bytes, then `END_REQUEST`; no empty `STDOUT`
   terminator.
7. Use one request id (1) and one connection per in-flight request unless deliberately
   reusing with `keepConn`.
8. Do not depend on `GET_VALUES`, `ABORT_REQUEST` or `FCGI_DATA`.

## Unverified / Open Questions

- **The exact `BEGIN_REQUEST` flag byte order for `keepConn` under load** is not exercised:
  only sequential reuse on one socket was measured, not a client that abandons a request.
  A probe that sends `ABORT_REQUEST` for an id whose script sleeps >1 s would settle
  whether php-fpm ever observes it (it has no read path for abort in `fastcgi.c`).
- **Whether a `PARAMS` pair split across records is tolerated by other SAPIs** (it is
  "malformed" per php-fpm's parser) — only php-fpm was measured.
- **`FCGI_MAX_CONNS`/`FCGI_MAX_REQS` are never answered**; whether any real client uses
  them is unverified (nginx/Apache do not send `GET_VALUES` at all).
- **The 8184-byte STDOUT chunk size is an implementation detail** of `out_buf` (8 KiB) in
  8.4; it was measured for 20 000 bytes but not across PHP versions.
- **`php-fpm` is not installed on this host**; every php-fpm measurement is inside
  `php:8.4-fpm-alpine` (PHP 8.4.27), not a distro package. A distro `php8.3-fpm` may print
  its own wrapper notices and ship a different `php.ini`.
- **`catch_workers_output = yes`** was set for the container (so NOTICEs were visible);
  whether a default pool hides stderr entirely was not measured.

## Open questions for the design

- One connection per request, or a small keep-alive pool with sequential ids? php-fpm
  supports the latter (measured, one request at a time) but a pooled connection holds a
  worker for its lifetime.
- Should the client send `GET_VALUES` at startup to assert `FCGI_MPXS_CONNS = 0` and refuse
  a listener that claims otherwise? (No known php-fpm claims otherwise.)
- When a request's param set exceeds 65 535 bytes, refuse the request, or split between
  pairs? Both are legal; only the split needs a per-pair size check.
- Is `pm.max_requests` part of the pool file Blastit writes, or the developer's choice?
  It bounds how long any per-request ini poisoning survives (see
  [php2shell](php2shell.md)).
- `keepConn = 0` after every request is what nginx defaults to; a reused connection would
  need an idle timeout the FastCGI protocol does not define.

## Sources

- FastCGI specification: <https://fastcgi-archives.github.io/FastCGI_Specification.html> —
  §3.3 Records / Padding / Managing Request IDs, §3.4 Name-Value Pairs, §3.5 Closing
  Connections, §4.1 GET_VALUES, §5.1 BEGIN_REQUEST, §5.3 Byte Streams, §5.5 END_REQUEST,
  §6.2 Responder, §8 Types and Constants, Appendix A/B.
- php-src 8.4: [`main/fastcgi.c`](https://github.com/php/php-src/blob/PHP-8.4/main/fastcgi.c)
  (`fcgi_read_request`, `fcgi_get_params`, `fcgi_hash_set`, `fcgi_flush`, `fcgi_write`),
  [`main/fastcgi.h`](https://github.com/php/php-src/blob/PHP-8.4/main/fastcgi.h),
  [`sapi/fpm/fpm/fpm_main.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_main.c)
  (`init_request_info`, `is_valid_path`, `sapi_cgi_activate`, `fastcgi_ini_parser`),
  [`sapi/fpm/fpm/fpm_php.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_php.c)
  (`fpm_php_limit_extensions`, `fpm_php_zend_ini_alter_master`),
  [`sapi/fpm/fpm/fpm_env.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_env.c),
  [`sapi/fpm/fpm/fpm_unix.c`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/fpm/fpm_unix.c),
  [`sapi/fpm/php-fpm.8.in`](https://github.com/php/php-src/blob/PHP-8.4/sapi/fpm/php-fpm.8.in),
  [`main/SAPI.c`](https://github.com/php/php-src/blob/PHP-8.4/main/SAPI.c).
- PHP manual, FPM: <https://www.php.net/manual/en/install.fpm.configuration.php> (global and
  pool directives: `listen`, `listen.owner`, `user`, `pm`, `pm.max_children`, `pm.max_requests`,
  `clear_env`, `security.limit_extensions`), <https://www.php.net/manual/en/install.fpm.php>.
- nginx:
  [`ngx_http_fastcgi_module.c`](https://github.com/nginx/nginx/blob/master/src/http/modules/ngx_http_fastcgi_module.c),
  [module docs](https://nginx.org/en/docs/http/ngx_http_fastcgi_module.html)
  (`fastcgi_keep_conn`, `fastcgi_param`, `fastcgi_split_path_info`, `fastcgi_request_buffering`),
  [`conf/fastcgi_params`](https://github.com/nginx/nginx/blob/master/conf/fastcgi_params).
- Apache: [`mod_proxy_fcgi.c`](https://github.com/apache/httpd/blob/trunk/modules/proxy/mod_proxy_fcgi.c),
  [`include/util_fcgi.h`](https://github.com/apache/httpd/blob/trunk/include/util_fcgi.h).
- Frameworks (what the params are used for):
  [Symfony `Request.php`](https://github.com/symfony/http-foundation/blob/7.3/Request.php),
  [Laravel `Request.php`](https://github.com/laravel/framework/blob/12.x/src/Illuminate/Http/Request.php),
  [Laminas Diactoros `UriFactory.php`](https://github.com/laminas/laminas-diactoros/blob/3.6.x/src/UriFactory.php).

License note: nothing here is vendored or linked. php-fpm is a runtime the developer
supplies (PHP License 3.01, not Apache-2.0-compatible for *linking* — irrelevant to a
protocol client), and the probes are stdlib Python.
