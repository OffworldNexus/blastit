# Log MB.3.9

## 2026-11-14T13:17 [lead]

created (epic) blastit dev spawns, configures, supervises and cleans up the php-fpm pool itself

## 2026-11-14T13:20 [lead]

OWNER SPEC (runtime discovery), 2026-11-14 -- folded into MB.3.9.1/.3/.4/.5: the runtime binary is resolved ONCE at boot by a GENERIC, provider-declared resolver. A provider declares (a) its ordered PATH search names and (b) its env override variable; the resolver itself is shared. PHP: search 'php-fpm' then versioned 'php-fpm8.4'/'8.3'/... in PATH, first executable wins; override is BLASTIT_PHP_FPM (env only, NOT a manifest key, same BLASTIT_* family). Validated before any pool spawn (is a file, executable, --version runs); a bad value fails naming variable+value+reason; an absent runtime fails naming the install package for the host distro family AND the override; never a stack trace, never a silent fallback. One resolution for dev/check/prod; resolved absolute path printed in the boot line and --json; two routes resolve one binary. Env must be read in the stage-0 config composition root BEFORE the stage-1 execve swap. ACTION FOR MB.12 (architect): adopt this generic resolver as the RuntimeProvider contract (Node declares its own names + override) instead of inventing a second one -- the design node MB.3.9.3 must say so in one sentence in ADR-0008.

## 2026-11-14T13:22 [lead]

OWNER SPEC (CPU accounting + who limits parallelism), 2026-11-14 -- binding on MB.3.6.3/MB.3.6.4 (logs written there) and owned generically by MB.12's provider contract: (a) the SCHEDULER is the only limiter -- pm.max_children/RLIMIT_NPROC are ceilings >= the route's slot allocation, and an under-provisioned provider is a boot-time refusal naming the mismatch, never a stall on a full pool, never pool-side self-throttling; (b) per-request CPU must travel on the EXISTING completion path (fair.rs Cost + RESP_END_COST clamp; request event cost_ns/cpu_ns) and is the scheduler's alone to convert into credit; (c) php-fpm supplies no per-request CPU and dev/check have no cgroup, so the honest default is observed wall time with cpu_ns:null -- any cpu.stat pro-rata refinement is production-only, must name its admit/complete sample points and its overlap split, and stays inside the wall-time clamp. ACTION FOR MB.12: the RuntimeProvider contract must own the GENERIC resolver AND the generic measurement interface (a runtime blastit does not own yields a per-request cost, defaulting to observed wall time), so FastCGI is the first implementation and not a special case. MB.3.9.3 must say this in one sentence in ADR-0008; MB.3.9.5 must carry the 'crashed flow is still charged wall time and releases exactly once' row in the protected tier.

## 2026-11-14T13:24 [lead]

HELD pending MB.12 embed-vs-pool decision. MB.3.9.1 (page) is running and is treated as a DRAFT: its mechanism-neutral parts (one-line manifest, one command) survive either way; anything naming php-fpm.conf/a pool must be re-read. The CPU-measurement design node (would have been MB.3.9.6) is deliberately NOT created: under embed, per-request CPU is exact in-process (clock_gettime CLOCK_PROCESS_CPUTIME_ID around the request), and the socket/SO_PEERCRED/pgid/forged-record machinery is only needed if an external pool survives as a fallback.

## 2026-11-14T14:06 [lead]

INSTALL-STORY follow-up (reviewer finding on MB.3.9.1, deferred not fixed): docs/frameworks/php.md teaches 'uv tool install blastit' + bare 'blastit dev', while README/getting-started teach 'uv add blastit' + 'uv run blastit'; the page's fence is accounted (static) rather than executed, so nothing proves the tool install. This is a cross-page DX inconsistency, not a php-specific one -- it belongs with MB.12's distribution decision (and the same decision owns BLASTIT_PHP_FPM vs BLASTIT_LIBPHP). When that lands, align all three pages and make the chosen install line an EXECUTED fence, not an accounted one.
