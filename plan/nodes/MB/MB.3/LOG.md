# Log MB.3

## 2026-11-13T10:42 [strategist]

created (epic) PHP-FPM FastCGI Backend & php2shell Defense (0.2.1)

## 2026-11-14T01:54 [planner]

Owner requirement 2026-11-14: every backend (static, WSGI, FastCGI) must be an injected implementation of one language-agnostic Upstream trait. Design extended in MB.3.3 (4th consistency lens, ADR-0008 split), retrofit split into new epic MB.9, MB.3.6 adds the FastCGI conformance row, MB.4 now depends on MB.9.

## 2026-11-14T13:13 [lead]

architect context (2026-11-14): epic MB.12 will land docs/design/backend-runtimes.md + an ADR defining the runtime-provider contract (app-process ownership, secret stripping, per-route uid mapping, Upstream + RuntimeProvider). Standing rules for this epic until then: (1) implement ADR-0008 / php-fastcgi.md 4.1 as accepted; if the manifest surface or the pool-spawn design looks inconsistent with a provider model, record it here and carry on -- do not redesign; (2) when MB.12's design lands, MB.3 design + ADR-0008 get re-read and an explicit amendment is expected; (3) keep pool spawn/ownership structured as 'the thing a provider does' (a self-contained provider seam), never as a one-off arm inside the scheduler, so the amendment is a wiring change rather than a rewrite. Also: no backend-kind match in dispatch; FastCGI stays an impl of the injected Upstream.

## 2026-11-14T13:24 [lead]

INVENTORY for the architect (embed-vs-pool), 2026-11-14. What I believe survives either way, and why:

SURVIVES UNCONDITIONALLY -- crates/blastit-fastcgi (MB.3.4, committed, 44/12/30/1 + 3/331/18 mutants): pure, zero-dep, no I/O, encode/decode + CGI head parse. Needed by (a) an external-fpm fallback and (b) any conformance row that still exercises the wire. It is NOT needed by the primary embed path: with libphp there is no FastCGI hop, so record.rs/param.rs/cgi.rs become fallback-only code. Keep, do not extend.

SURVIVES WITH ONE RENAMED CONCEPT -- the manifest grammar (MB.3.5.1, uncommitted): 'no listen' is right either way (blastit owns the transport in both models, and in embed there is no transport at all). 'entry = public/index.php' is right either way (in embed it is the script passed to the PHP executor). 'workers' KEEPS MEANING in embed (the number of per-route PHP worker PROCESSES, since a non-ZTS libphp cannot share one process across threads) but LOSES its FastCGI framing ('pm.max_children' is FPM vocabulary); the field should stay optional+budget-derived either way. 'root'/'static' composition is unchanged.

DOES NOT SURVIVE as designed -- the param wall (MB.3.4.2's lockdown): PHP_VALUE/PHP_ADMIN_VALUE/REDIRECT_STATUS/HTTP_PROXY and the whole CVE-2024-4577/8926/8927 class are FastCGI/CGI ARGUMENT-INJECTION defects. With embed there is no param stream to smuggle into, so those rows become moot in the primary path (they remain the reason a FastCGI fallback must keep the wall). This is a large security simplification, and it is the strongest argument for embed.

SURVIVES ONLY AS FALLBACK -- pool spawn/supervise (MB.3.6.3), session pump/dispatch (MB.3.6.2), the derived socket + PoolReady gate, BLASTIT_PHP_FPM discovery, and the cgroup/SO_PEERCRED CPU-report channel. All of it is external-process machinery.

NEW RISK THE ARCHITECT MUST WEIGH (from the code/security side, not researched opinion): libphp has no stable C ABI and is built per PHP version; it must ship or be built for the target image (php-dev + matching libphp), which is a heavier distribution story than 'find php-fpm in PATH'. And embedding moves PHP's memory-safety boundary INSIDE a blastit-owned process: today a PHP RCE is a separate route-uid fpm process, whereas in-process it is code execution in the same address space as blastit's worker. That is only acceptable if the PHP host is a dedicated per-route worker process (fork + drop_privileges + seccomp, exactly the CPython 'view' role), never the scheduler, and never sharing an address space with a broker. Both dev and prod should then be the same shape, which is a DX win.

## 2026-11-14T13:39 [lead]

FOLLOW-UP for the embed-vs-pool decision: docs/frameworks/php.md's H1 is 'PHP-FPM, Symfony and Laravel' and the nav label 'PHP-FPM' lives in zensical.toml:37 + docs/frameworks/index.md (both outside MB.3.9.1's paths). Under the libphp direction naming FPM in the page title is wrong. Cheap fix once MB.12 decides: retitle to 'PHP (Symfony, Laravel)' and change the nav label in the same commit; do not do it before the decision, since a FastCGI-fallback page may still want the word.

## 2026-11-14T13:39 [lead]

RENAME RISK from the architect's research (docs/research/php-embed-sapi.md, MB.12): the embed model names the block '[route.php] entry = ...' and the override 'BLASTIT_LIBPHP' (dlopen the HOST's libphp.so, never bundled, PHP-3.01). This affects two things already on disk: (a) MB.3.9.1 shipped a 'BLASTIT_PHP_FPM' row in docs/reference/settings.md and a 'fastcgi' framing that would need renaming/replacing after the decision; (b) MB.3.5.1's '[route.fastcgi]' key survives as the FastCGI-fallback spelling, so its work is not wasted, but expect a small node to add '[route.php]' (or rename) once MB.12 lands -- do NOT pre-emptively rename anything now. Also note the research's licence/ABI facts for whoever implements: dynamic dlopen only, NTS fits the fork-per-route view topology, and stock distro libphp-embed packages work without compiling extensions.

## 2026-11-14T14:22 [lead]

BATCH 3 CLOSED (commits 5c4d8c6, 0bbb877, 0637f89, 4cd727b). MB.3.4 epic gate PASSED: 44/12/30/1, zero deps, pure codec, 3/331/18 mutants, gate audit 0 fluff/0 misplaced/0 oracle-less public items; MB.3.4.6 closed the audit's last two branch-inspection gaps. MB.3.5.1+MB.3.9.1 landed the one-line route.fastcgi + the two-command PHP page as ONE commit (each alone leaves docs_fences/parser red). MB.3.9.2 amended ADR-0008/design 4.1 (listen removed, workers optional+budgeted) after four attempts; its last two fails were plan-level (FastCgiPoolSocketInvalid assigned to a done leaf; MB.3.5.2's Files missing error.rs -- paths fixed). HOLDING, per owner: MB.3.5.2, MB.3.6/.1/.2/.3/.4, MB.3.7, MB.3.9.3/.4/.5 all frozen pending MB.12's embed-vs-pool decision; nothing deleted, nothing failed. KNOWN FLAKE (not mine, not fixed): crates/blastit/src/supervise.rs:1045 a_late_ctl_write_to_a_gone_scheduler_is_best_effort fails intermittently under full-workspace parallelism ('a closed peer cannot take the message'), passes 3/3 alone; it cost one make check run during the gate. First EPIPE on a closed AF_UNIX peer can succeed, so the assertion races. Owner of that file should make it deterministic (second write, or assert on the classified error) instead of retrying the gate.

## 2026-11-14T14:42 [lead]

PLAN FREEZE (2026-11-14, owner): no further ADR-driven renames/supersedes until the ADR reconciliation lands. THERE ARE NOW TWO ADR-0010 FILES: docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md (the one I read and applied in the node-table pass: MB.3.5.4/.10/.11/.12/.13 added, MB.3.6.3.* and MB.3.9.3-.5 superseded, MB.3.5/.5.2/.5.3/.6/.6.1/.6.2/.7/.8/MB.9.3 retargeted) and docs/decisions/ADR-0010-php-embedded-vs-fastcgi.md (written by the architect when its session resumed). The architect is consolidating them into one; ADR-0009-runtime-providers-and-capability-gating.md also appeared from that session. UNTIL THAT LANDS: neither ADR-0010 file is citable as authoritative for a NEW node, and no further plan structural edits happen. What stays true regardless of which draft wins: PHP is embedded via host NTS libphp.so + a per-minor C SAPI shim in the view worker; php-fpm is at most an external fallback; pyo3 must be feature-gated so a Python-free build is possible. That is why MB.3.11 (code) keeps running. I stopped my designer (second writer) before it could finish; docs/design/backend-runtimes.md must be the architect's alone and is NOT mine to reconcile.

## 2026-11-14T14:43 [lead]

CHECKPOINT (owner instruction 2026-11-14): when the architect finishes regenerating docs/design/backend-runtimes.md and consolidating the two ADR-0010 files, the plan gets ONE adjustment pass, not a rewrite: re-read MB.3.5.4, MB.3.6.1/.2, MB.3.6.4, MB.3.7, MB.3.8, MB.9.3 and the MB.3.6.3.*/MB.3.9.3-.5 supersessions against the merged ADR's TRANSPORT answer, and fix only what disagrees. If the answer is 'view.chan, no pool', the plan is already correct and the pass is a NO-OP -- say so rather than editing for the sake of it.

## 2026-11-14T14:54 [lead]

RECONCILIATION LANDED (2026-11-14) -- 3ce74a6's PROVISIONAL marker is CLOSED, CONFIRMED AS A NO-OP. Canonical ADR is docs/decisions/ADR-0010-php-embedded-via-libphp-embed.md (title: 'Embedded PHP Execution via Host NTS libphp.so and Per-Minor SAPI Shims'); the duplicate -vs-fastcgi file was merged and DELETED (verified: only one ADR-0010 on disk). Transport for [route.php] is view.chan (RawFrame socketpair) -- zero pool, zero named socket -- which is exactly what the node-table delta already assumed, so the adjustment pass found NOTHING to change: MB.3.5.4, MB.3.6.1/.2, MB.3.6.4, MB.3.7, MB.3.8, MB.9.3 and the MB.3.6.3*/MB.3.9.3-.5 supersessions stand as committed. Chain: ADR-0008 (Upstream + external fallback codec; pool choreography superseded) -> ADR-0009 (Provider/WorkerRuntime, capability-gated Python, /proc-floored CPU fair queuing, distribution) -> ADR-0010 (embedded PHP over view.chan). NEW SECURITY CONSTRAINT to carry into the nodes: [route.fcgi] is no longer merely 'documented weaker fallback' -- it FAILS CLOSED (exit 1) in Production and under every preset unless BLASTIT_ALLOW_EXTERNAL_FASTCGI=1 is set AND no preset is active. That flag is a security switch, not a convenience, and must be documented as one in docs/reference/settings.md + the php.md fallback note, and named in the boot-refusal message the code emits (MB.3.10 and MB.3.5.4 bodies updated).
