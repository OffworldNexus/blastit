# Log MB.3.47

## 2026-11-15T04:14 [test-curator]

created (task) critical_php_embed's CI-coverage note is false since MB.3.39 (protected file, doc-only)

## 2026-11-15T04:35 [lead]

status ready -> active

## 2026-11-15T04:35 [lead]

attempt 1/3 started

## 2026-11-15T04:35 [lead]

base 36b8aad

## 2026-11-15T04:37 [lead]

status active -> review

## 2026-11-15T04:44 [lead]

status review -> done

## 2026-11-15T04:46 [lead]

post-commit note: 5710182's gate was red on a retry-proof-unrelated flake (critical_php_backend a_blastit_libphp_that_is_not_a_library_refuses_the_boot read its refusal text before the child wrote it); the same tree is green on retry. The lead committed on a swallowed exit code and recorded MB.3.48 to make this deterministic; the .47 diff itself is //!-only and its reviewer ran the gate green.
