# Log MB.3.8

## 2026-11-14T01:09 [planner]

created (task) Release prep 0.2.1: version bump, dated changelog, ARCHITECTURE sync

## 2026-11-14T03:37 [planner]

edited fields

## 2026-11-14T10:51 [planner]

edited fields

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.

## 2026-11-14T16:36 [unknown]

edited fields

## 2026-11-14T18:54 [lead]

INPUT from MB.3.25 review (2026-11-14): ARCHITECTURE.md §7.3 is now stale against seccomp.rs — it says every refusal is EPERM with no clone3->ENOSYS exception, and lists neither clone CLONE_NEW* EPERM, setsid/setpgid EPERM, nor SeccompRole::EmbeddedView (execve/execveat/fork/vfork/non-thread clone). Update §7.3 in this node's ARCHITECTURE sync; design R14/§5.1.3 is the truth.

## 2026-11-14T21:04 [lead]

INPUT from MB.3.10 review (2026-11-14): docs/design/static-and-spa.md:41 still says the PHP worker is 0.2.1; the release folded into 0.2.0. static-and-spa.md is in this node`s paths — fix in the 0.2.0 sync.

## 2026-11-14T21:10 [lead]

RELEASE CHECKLIST ITEM (owner 2026-11-14, verbatim): add to the 0.2.0 certification: 'no / under a static root is servable (404, protected ), proven before the 0.2.0 tag.' Certification fails without it. Implemented by MB.3.33; MB.9.2.2 must preserve it.

## 2026-11-14T21:10 [lead]

RELEASE CHECKLIST ITEM (owner 2026-11-14, verbatim): add to the 0.2.0 certification: "no `.php`/`.phar` under a static root is servable (404, protected `critical_static_entry`), proven before the 0.2.0 tag." Certification fails without it. Implemented by MB.3.33; MB.9.2.2 must preserve it. (Reposted because the shell ate the backticked names in the first line.)
