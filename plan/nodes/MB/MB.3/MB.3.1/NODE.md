---
id: MB.3.1
title: "Research FastCGI wire, non-root php-fpm and php2shell techniques with cited sources"
kind: task
status: done
owner: researcher
priority: 1
depends_on: []
paths: ["docs/research/**"]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-14T01:09"
updated: "2026-11-14T01:29"
---

# Goal

Produce the two cited research notes that `docs/frameworks/php.md` and the FastCGI
design stand on: the FastCGI wire protocol plus how a non-root developer runs
`php-fpm`, and the `php2shell` attack class with the **exact attacker-controlled
field** of every technique. Every claim carries a URL and section.

# Context

- Input of record for the whole epic. `MB.3.2` (docs) may not write a line the notes
  do not support; `MB.3.3` (design) copies the attack table and may not add rows
  without a source.
- Two notes, both under `docs/research/`, both listed in `docs/research/INDEX.md`:
  1. `docs/research/php-fastcgi-wire.md`
     - Record header layout (`version`, `type`, `requestId` big-endian u16,
       `contentLength` u16, `paddingLength`, `reserved`) and the types blastit must
       speak: `BEGIN_REQUEST` (role `RESPONDER`, `flags`, `keepConn`), `PARAMS`,
       `STDIN`, `STDOUT`, `STDERR`, `END_REQUEST` (`appStatus`, `protocolStatus`),
       `ABORT_REQUEST`, and `GET_VALUES` only if the design needs it.
     - Name-value encoding: length prefix 1 byte when `< 128`, else 4 bytes with the
       high bit set; names and values independently sized; padding to 8 bytes;
       empty `PARAMS` terminator.
     - `requestId` semantics: one id per connection, strictly increasing, `0` reserved
       for management records; what a real client does when ids are exhausted; whether
       pipelining more than one id on one connection is used in practice by
       `php-fpm` (and what it does when it is not).
     - Frame size limits (`u16` content length), the `STDOUT` + `END_REQUEST` ordering,
       stdout-before-end framing, and how a 200-line role-play of one request looks on
       the wire (annotated hexdump of a `public/index.php` GET is welcome).
  2. `docs/research/php2shell.md`
     - How Symfony / Laravel / Slim / PSR-7-15 front controllers consume
       `SCRIPT_FILENAME`, `SCRIPT_NAME`, `PATH_INFO`, `DOCUMENT_ROOT`, `REQUEST_URI`,
       `QUERY_STRING`, `REDIRECT_STATUS`, `SERVER_PROTOCOL`, `REQUEST_METHOD`: which
       ones PHP itself uses to locate and execute the script, which ones the framework
       uses to route (`PATH_INFO` vs `REQUEST_URI`), and what a real nginx
       `fastcgi_params`/`try_files` block sets for each (`fastcgi_split_path_info`
       included). Cite PHP manual sections and the upstream Symfony/Laravel docs.
     - Running `php-fpm` as a **non-root** developer: `php-fpm -F -y <pool.conf>`;
       `user`/`group` when the master is not root (php-fpm refuses to change uid then);
       `listen = /path/sock` with `listen.owner`/`listen.mode`; `-R` and why it must
       not be needed; `pm = static|dynamic`, `pm.max_children`, `clear_env`, and
       `security.limit_extensions`. State the one canonical non-root command line and
       the pool keys that matter, with sources.
     - `php2shell` class, one table row per technique: `SCRIPT_FILENAME` traversal /
       absolute path, `PATH_INFO`-into-`SCRIPT_FILENAME` trick, `PHP_VALUE` /
       `PHP_ADMIN_VALUE` FastCGI param overwrite, CVE-2012-1823-style query-string `-d`
       argument injection (`?-d allow_url_include=1 -d auto_prepend_file=php://input`),
       CVE-2024-4577 (Windows PHP-CGI best-fit/encoding argument injection — say
       plainly whether it applies to a Linux `php-fpm` listener and why it is still
       tested), `httpoxy` (`HTTP_PROXY` from a `Proxy:` request header), `auto_prepend_file`
       via `PHP_VALUE`, `open_basedir`/`disable_functions` weakening, and
       `REDIRECT_STATUS` / `DOCUMENT_ROOT` spoofing. Columns: technique — attacker
       field (header/param/path byte) — what it reaches — the binary defence (refuse
       the param, pin the value, unset the header).
- Do **not** propose a design, an allowlist, or a manifest key. Findings only; the
  design (`MB.3.3`) turns them into grammar and a refusal matrix.

# Acceptance

```console
$ test -f docs/research/php-fastcgi-wire.md && test -f docs/research/php2shell.md && echo "both notes exist"
$ for k in BEGIN_REQUEST PARAMS STDIN STDOUT STDERR END_REQUEST requestId paddingLength; do grep -q "$k" docs/research/php-fastcgi-wire.md || echo "missing $k"; done
$ grep -q 'php-fpm -F -y' docs/research/php-fastcgi-wire.md && echo "non-root run line recorded"
$ grep -q 'SCRIPT_FILENAME' docs/research/php2shell.md && grep -q 'PATH_INFO' docs/research/php2shell.md
$ for k in PHP_VALUE PHP_ADMIN_VALUE REDIRECT_STATUS auto_prepend_file allow_url_include HTTP_PROXY httpoxy CVE-2012-1823 CVE-2024-4577; do grep -q "$k" docs/research/php2shell.md || echo "missing $k"; done
$ test "$(grep -c 'https\?://' docs/research/php2shell.md)" -ge 15 && echo "php2shell note is cited"
$ test "$(grep -c '^|' docs/research/php2shell.md)" -ge 12 && echo "one table row per technique"
$ grep -q 'php-fastcgi-wire' docs/research/INDEX.md && grep -q 'php2shell' docs/research/INDEX.md
$ make docs-check
$ make check
```

# Notes

- Notes only: no `docs/frameworks/`, no `docs/design/`, no `docs/decisions/`, no code,
  no manifest key. A claim you cannot source is written as "**unverified**" plus what
  would settle it, and recorded under `## Follow-ups` in `RESULT.md`.
- Run `php-fpm` locally to check the pool facts rather than trusting a blog; record the
  PHP and php-fpm version you verified against. If `php-fpm` is not installed, say so
  in `RESULT.md` and cite the manual for the pool keys instead of guessing.
- Confidentiality rule: no client or sibling-project names in the notes.
- If a wire detail needs a decision rather than a fact (e.g. whether to pipeline
  request ids), list it under `## Open questions for the design` — do not decide it.
