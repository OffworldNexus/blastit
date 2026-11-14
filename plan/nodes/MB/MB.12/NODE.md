---
id: MB.12
title: "Architect how blastit serves each backend runtime (WSGI, PHP-FPM, Node, static, JVM) and what distribution follows from it"
kind: epic
status: ready
owner: architect
priority: 1
depends_on: []
paths: ["docs/design/backend-runtimes.md", "docs/decisions/**", "docs/frameworks/**"]
attempts: 0
max_attempts: 3
escalation: architect
created: "2026-11-14T13:05"
updated: "2026-11-14T13:05"
---

# Goal

Architect **how blastit serves each kind of backend**, end to end, and let distribution fall out of that — instead of deciding packaging first and hoping the architecture fits.

The question this epic answers, before any packaging or feature-flag work:

> For each language/runtime we serve — Python WSGI, PHP-FPM, Node/Vite SSR, static files, later JVM — **who holds the app's code, who owns its process lifecycle, where do secrets live, what does the sandbox look like, how does a request reach app code, and what must exist on the host?**

# Context

Blastit's current design has one shape: the supervisor spawns per-route workers from *its own binary*, and a worker embeds CPython to run a WSGI callable. That works because blastit *is* the Python app server. Every other backend breaks that assumption in a different way, and pretending otherwise is how "just put it all in one binary" becomes an unmaintainable lie:

| Backend | Who runs app code | Runtime on host | Who can own the lifecycle |
|---|---|---|---|
| WSGI (Python) | blastit's own worker, CPython embedded via PyO3 | libpython of a matching minor version | blastit (it is the server) |
| PHP-FPM | `php-fpm` master, one pool per route | `php-fpm` + the app's `public/index.php` | blastit supervises the master; php-fpm owns workers |
| Node/Vite SSR | the app's `node server.js` (adapter handler) | Node major | blastit spawns it, or connects to one the user runs |
| Static | nobody — blastit answers | nothing | blastit |
| JVM/Tomcat | the app's JVM / Tomcat | JVM | blastit connects; Tomcat owns its threads |

The consequences differ per row and must be written down rather than discovered:

1. **Binary linkage.** Embedding CPython (WSGI) drags `libpython`, `PYO3_PYTHON`, `PYTHONHOME`/`LD_LIBRARY_PATH` into the binary and the host. A static-or-PHP deployment must not carry any of that. So the *artifact graph* has two shapes (Python-linked, not), while the *behaviour* stays one program. Feature-gating the Python backend is the mechanism; the architecture must state it as "capability absent, refuses cleanly", never "different program".
2. **Process ownership.** blastit can only strip secrets at `execve` for processes **it** spawns. For php-fpm that means blastit spawns the master with a stripped environment and per-pool uid; for Node it means either blastit spawns the server (and can then strip) or the user does (and blastit cannot — a materially weaker mode that must be documented or refused). Embedded WSGI stays the case blastit controls completely.
3. **Sandbox identity.** Per-route uid isolation assumes the worker runs as that uid. php-fpm gives one pool = one uid, which fits. Node gives one process = one uid, so per-route isolation means one process per route (memory cost must be stated). Embedded Python fits because blastit forks its own binary. This should be a property of the runtime-provider contract, not a per-backend special case.
4. **Ingress and egress are unchanged.** The parser, header/body limits, scheduler, per-route sockets, `--learn`/`check` observation and the proxies sit *before* app code and must not vary per language. The `Upstream` trait (ADR-0008) is the seam: a backend is an implementation plus a runtime provider, not a second pipeline.
5. **`--learn`/`check` attribution.** Wire observation is language-agnostic only if it happens at the boundary blastit owns. For embedded Python that is the WSGI call; for php-fpm the FastCGI stream; for Node the HTTP request/response; for static nothing. The ADR must define what "a route was exercised" means for a static route and for a Node SSR route so the four-way completeness check stays honest.
6. **Host prerequisites.** "npm package" or "standalone binary" is a *delivery* answer to a *runtime* question: a static site needs nothing, PHP needs `php-fpm`, Node needs Node, WSGI needs CPython. The install matrix must be derived from the table above, and a channel that cannot satisfy its runtime must fail at boot with a message naming the missing runtime, not at first request.

# Acceptance

1. `docs/design/backend-runtimes.md` — the architecture: the runtime-provider contract, the taxonomy above, a per-backend section (lifecycle, uid, secrets, sockets, learn/check semantics, failure modes), and the explicit statement that ingress/scheduler/proxies are shared and unmodified per backend. Include a decision table of what each backend *cannot* do (e.g. per-route uid with a single user-run Node process; secret stripping for a process blastit did not spawn).
2. ADR — the decisions this forces: (a) capability-gated Python backend with a clean refusal, (b) blastit spawns app runtimes it can sandbox, and a documented weaker mode (or a refusal) when it cannot, (c) the `Upstream` + runtime-provider seam, (d) how per-route isolation maps onto each runtime and where memory cost is paid, (e) what "exercised" means per backend for `check`.
3. A distribution section that *follows* from the architecture rather than driving it: which artifact carries which capability, what each host must have installed, and how a missing runtime fails at boot (with the message).
4. Explicitly rejected alternatives, with the invariant each one breaks — e.g. "bundle every runtime into one binary" (licensing, CVE surface, version pinning of every language), "one process per route for Node by default" if the memory cost is unacceptable, "let the user's Node process keep its own secrets" if that forfeits the stage-0 barrier.
5. Critic acceptance (`security`, `implementability`, `testability`, `consistency`) before any packaging, feature-flag or MB.4 code. `dx` + `ai-ergonomics` on the install matrix once it exists.

# Notes

- This is deliberately **before** MB.4 (Node) and before the npm/standalone packaging work: the packaging is a consequence, not the premise.
- Do not design the MB.4 addon here; it is one option for Node and gets its own ADR (MB.4.1). This epic fixes the surrounding architecture the addon must fit into.
- Per-route isolation, secret stripping and fail-closed production are non-negotiable; where a backend cannot honour one, say so plainly and choose the weaker-but-honest mode rather than implying parity.
- PHP is the useful test of the architecture: it is the first backend blastit does **not** own end to end.
