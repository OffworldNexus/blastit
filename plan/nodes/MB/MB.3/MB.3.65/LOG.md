# Log MB.3.65

## 2026-11-15T21:43 [lead]

created (task) Descope PHP 8.2: supported versions are 8.3 and 8.4

## 2026-11-15T21:43 [lead]

edited fields

## 2026-11-15T21:44 [lead]

owner addition: (A) no apology language in user-facing docs — php.md:46-48 deleted, the 'not a degraded mode' lines kept; (B) narrow banned-phrase guard in make check with negative self-tests over docs/**/*.md (minus design/decisions/research/knowledge/testing/VISION) + README.md, no fuzzy words

## 2026-11-15T21:53 [lead]

edited fields

## 2026-11-15T21:53 [lead]

owner addition 2: MB.3.65 becomes a SWAP — 8.2 out, 8.5 in. Matrix stays 12 shims (8.3/8.4/8.5); 8.5 legs are ubuntu:26.04 (libphp8.5-embed/php8.5-dev) and alpine:3.23 (php85-embed/php85-dev); 8.5 ABI offsets measured not assumed; discovery adds 8.5 ahead of 8.4; docs sentence 'We support supported PHP versions: 8.3, 8.4 and 8.5.'

## 2026-11-15T23:18 [lead]

status ready -> active

## 2026-11-15T23:18 [lead]

attempt 1/3 started

## 2026-11-15T23:18 [lead]

attempt 1/3 started; base 2419971

## 2026-11-16T00:11 [lead]

status active -> review

## 2026-11-16T00:19 [lead]

status review -> done

## 2026-11-16T00:19 [lead]

commit fa91776; reviewer MINORs filed as MB.3.68 (installed_unsupported names a stray 8.2 object when a supported lib sits at an unprobed spelling) and MB.3.69 (two-package witness rewritten but unexecuted; RESULT A7 omits LESSONS/php2shell 8.2 hits)
