# Log MB.3.5.2

## 2026-11-14T10:51 [planner]

created (task) SocketProbe, boot probes and the manifest reference entry - design MB.3.5 part 2

## 2026-11-14T13:19 [lead]

DO NOT START until MB.3.9.2 and MB.3.9.3 land: this node's brief derives from the old grammar (user-authored listen, DevProbe::check = 'socket must already exist/is S_ISSOCK/connectable'). Under the 2026-11-14 owner decision blastit owns the socket, so check_fastcgi_entries' rule set and DevProbe::check become 'the run dir is blastit's to create; the leaf is absent or stale'. implementer: read MB.3.9.2's amended 4.1 + MB.3.9.3's amended 4.8.1 first; if they are not yet on disk, report blocked rather than implementing the stale rules.

## 2026-11-14T14:12 [unknown]

edited fields

## 2026-11-14T14:12 [lead]

paths extended (2026-11-14) to crates/blastit-manifest/src/error.rs: the review of MB.3.9.2 found the design attributes FastCgiPoolSocketInvalid to MB.3.5.1 (done, never had it -- error.rs carries only FastCgiEntryInvalid/FastCgiListenDerived/FastCgiWorkersInvalid/FastCgiDeadDeclaration) while this leaf's protected suite refuses the three dev legs through it. This leaf ADDS the variant (still gated with the rest of the probe work on MB.12).

## 2026-11-14T14:39 [unknown]

edited fields

## 2026-11-14T14:39 [lead]

ADR-0010 supersedes the pool model for PHP: [route.php] is first-class (embedded NTS libphp.so + per-minor C SAPI shim in the unprivileged view worker); [route.fastcgi] is an external operator-managed fallback (entry + listen, no workers, loud weaker-mode warning); php-fpm pool machinery is dropped, not deferred. Retarget this node per the ADR's Node Changes table; do not implement pool ctl/PoolReady/parentfd.
