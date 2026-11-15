# Log MB.3.13.4

## 2026-11-14T17:04 [lead]

created (task) MB.3.13 part 4: Protected critical_php_embed suite: boot refusals, exit()/fatal containment, exact cpu_ns, and no libphp in init/http/scheduler/dbproxy

## 2026-11-14T18:20 [unknown]

edited fields

## 2026-11-14T18:20 [lead]

OWNERSHIP: you own the protected suite ONLY. PhpEmbedRuntime and php = [dep:blastit-php-sapi] are MB.3.13.1's (design §5 row 7 amended). Do not build the runtime here.

## 2026-11-14T19:29 [lead]

edited fields

## 2026-11-15T00:48 [lead]

status ready -> active

## 2026-11-15T00:48 [lead]

attempt 1/3 started

## 2026-11-15T00:48 [lead]

base 72e6d1fece8878466d418a5e278c4c5af856d74b

## 2026-11-15T00:48 [lead]

edited fields

## 2026-11-15T00:59 [lead]

status active -> review

## 2026-11-15T01:04 [lead]

REVIEW FAIL (attempt 1): MAJOR the shim-minor leg passes a boot-then-500, so the epic's 'mismatch exits 1 and starts no listener' is unpinned; MAJOR its empty-stdout + assert_never_loaded assertions contradict the ADR-0010 s3 probe child MB.3.36 will add. Both move to MB.3.36 (which owns the tightening). MINORs kept here: non-library leg's canaries are unreachable (zero-dlopen only proved by the ZTS leg), Drop never runs so stub dirs leak, 'same slot' is uid equality (constant in dev), inode swap is a retried stderr race, and the real-pair legs only run where libphp is installed.

## 2026-11-15T01:04 [lead]

status review -> ready

## 2026-11-15T01:17 [lead]

MB.3.36 landed the probe child and tightened the shim-minor leg (Some(1), no ready/request, connect refused, load only below a fork, plus a 250ms timeout leg). Re-dispatching for the five MINORs.

## 2026-11-15T01:17 [lead]

status ready -> active

## 2026-11-15T01:17 [lead]

attempt 2/3 started

## 2026-11-15T01:25 [lead]

REVIEW FAIL (attempt 2): MAJOR --test-threads=1 (which ci.yml:359 uses for every binary, and 1-vCPU hosts default to) fails 3/8 because StubRootGuard deletes the shared root while the OnceLock never rebuilds; must rebuild when the root is absent. MINORs: the maps test fails as uid 0 (add a geteuid gate or a root-phase exclusion), the swap panic should also name 'inode pin removed', and the 250ms leg must prove the probe child is gone.

## 2026-11-15T01:25 [lead]

status active -> ready

## 2026-11-15T01:25 [lead]

status ready -> active

## 2026-11-15T01:25 [lead]

attempt 3/3 started

## 2026-11-15T01:38 [lead]

status active -> done
