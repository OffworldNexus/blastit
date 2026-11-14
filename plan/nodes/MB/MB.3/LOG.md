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
