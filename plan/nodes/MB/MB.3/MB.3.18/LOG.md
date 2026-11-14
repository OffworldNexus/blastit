# Log MB.3.18

## 2026-11-14T15:17 [lead]

created (task) Build and publish the Linux packaging matrix: x86_64/aarch64 x glibc/musl, wheels + standalone binaries + PHP shims

## 2026-11-14T15:17 [lead]

QUEUED, not started (owner 2026-11-14): Linux only; aarch64 and Alpine/musl in scope. Runs AFTER MB.3.12 (Cargo.toml/lock + shims) and MB.3.17 (removes a workspace member), and after MB.3.13 (boot selection rule). musl + embedded PHP is the riskiest leg (Alpine packaging, dlopen against musl-linked libphp.so, /proc guarantees) -- if it fails it is DROPPED WITH A WRITTEN REASON, reported up, not silently skipped and never shipped best-effort. 12 shims = 3 minors x 2 archs x 2 libcs, built NATIVELY, named with arch+libc+minor.

## 2026-11-14T15:19 [lead]

ACCEPTANCE TIGHTENED (owner 2026-11-14, 'drop musl if it cannot be made secure'): the musl leg's gate is SECURITY, not 'it runs'. Six items must hold on the Alpine leg -- secrets (stage-0 execve strip effective, worker /proc/<pid>/environ asserted), isolation (per-route uid + SeccompRole::View, filter NOT silently widened for musl: a needed syscall is justified everywhere or musl loses), egress (nothing beyond granted proxy sockets), CPU accounting (CLOCK_PROCESS_CPUTIME_ID + /proc reconciliation; degraded-only is a REPORTED fairness degradation, not a pass), provenance (signed Alpine libphp, dlopen without a compat shim, native musl build), lockdown (ffi.enable=0, phar.require_hash=1, memory_limit, protected extension_dir hold against Alpine INI defaults, asserted via ini_get). Any unfixable failure => DROP musl from matrix/CI/docs/install table with ONE line naming the failing item, and report it to the conductor as an owner-visible decision. A leg never attempted is a FINDING, not a pass. The same six items are recorded per glibc leg so musl is not held to a higher standard. Queue unchanged: MB.3.12 -> MB.3.17 -> MB.3.13 -> MB.3.18.

## 2026-11-14T15:30 [lead]

PROVEN-COUNT RULE (owner 2026-11-14): MB.3.12 built and probed 8.4 only; 8.2/8.3 are source-guarded but UNBUILT. This node's first job is first build + probe of 8.2 and 8.3 (Dockerfile takes PHP_MINOR); a minor that cannot be built AND probed is dropped from the matrix with a reason (the musl rule). Until then NO document may claim '12 shims' -- it is 4 proven (8.4 x 4 legs). ACTION ROUTED UP: the ADR-0010 SS4 distribution matrix row still implies 12; that sentence must become '4 proven (8.4 x 4 legs) until 8.2/8.3 are built and probed'. I did NOT edit docs/decisions/** myself: the architect owns that file and the last two-writer attempt on a design doc cost a 65 KB document. Route it to the architect (one line; no other change).
