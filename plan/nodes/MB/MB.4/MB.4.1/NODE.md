---
id: MB.4.1
title: "Decide Node integration topology: napi-rs/neon embedding vs sidecar fd handoff, and the fair-queue CPU model (ADR)"
kind: task
status: ready
owner: architect
priority: 1
depends_on: []
paths: ["docs/decisions/**", docs/frameworks/node.md, "docs/research/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T22:15"
updated: "2026-11-13T22:15"
---

# Goal

Decide — with evidence — how Blastit integrates with Node/Vite for 0.2.2: an in-process native addon (napi-rs or neon) that replaces Node's HTTP listening/parsing, or Blastit's existing sidecar/upstream topology, or a hybrid (addon as a thin socket-passing facade). Produce an ADR that keeps Blastit's security invariants intact and still ships Vite/node adapter compatibility.

Owner's direction (2026-11-13), to be evaluated rather than assumed:
> "Use something like napi-rs or neon-rs to get going. Be as compatible with Vite node adapter as possible, but replace the HTTP listening/parsing part. The whole thing is wrapped in node, the JS calls the init, the init does its magic, etc. Regarding fair queuing, every request gets instrumented with some kind of flag, and every CPU time spent inside this request counts towards this request. Every time the request sends data to the client, the CPU time of that particular request gets counted as quantum for this flow. This way we keep things fair even for web sockets."

# Context

Why this is a fork rather than a task: today's topology is what makes Blastit a security boundary. The supervisor owns privileges, the ingress parser runs in its own process, each route gets its own uid, and credentials are stripped at `execve` and injected only by the proxies. An addon loaded into the app's Node process puts the app and the parser in one address space, and any native module sees `process.env`.

Constraints the ADR must respect (from `docs/VISION.md`, `ARCHITECTURE.md`, `docs/decisions/`):

1. **The parser must not share an address space with untrusted app code.** That is the entire premise (`php2shell`/`node2shell` classes assume the parser is reachable from the app).
2. **Secrets must not be readable by app code.** `execve` env stripping is the current mechanism; `delete process.env.X` inside one process is strictly weaker — `/proc/self/environ` still shows the original environment, and a pre-`init()` read sees everything.
3. **Per-route isolation:** separate uids, `0600` sockets, `seccomp`, capability checks, fail-closed production.
4. **Vite/node adapter compatibility:** frameworks ship handlers of the shape `(req, res) => void` (`@sveltejs/adapter-node`, Nuxt, Remix, Hono, Next standalone), and `vite dev`/`vite preview` own their own listeners. Compatibility means synthesizing Node-shaped `IncomingMessage`/`ServerResponse` facades over whatever Blastit does internally — `req` is a `Readable` with `headers`/`rawHeaders`/`socket`, `res` a `Writable` with `writeHead`/`end`, plus HTTP `upgrade` for websockets. Node's own parser internals are not public API, so a facade is the only portable route.

Questions the ADR must answer:

- **Binding:** napi-rs vs neon vs `node-api` directly. Weigh maintenance activity, N-API stable ABI and prebuilt binaries (so an npm package works across Node majors), threadsafe functions for async, `tokio` integration, memory/ownership across the boundary, license, and build/distribution cost (per-platform cdylibs vs the current wheel pipeline).
- **Where the socket lives.** Options: (a) addon parses in-process (fastest, weakest boundary); (b) addon only calls `listen()` and passes the fd via `SCM_RIGHTS` to the privileged supervisor, which parses in its own uid and tunnels frames back (keeps the boundary, removes Node's parser); (c) no addon — `blastit serve --backend node --upstream` as today. Compare latency, syscalls, and what each one does to invariants 1–3.
- **What `init()` actually means** in each option: does it spawn the supervisor and route workers (as `blastit dev` does today), or is it purely in-process? If it spawns, what is the app's lifecycle (exit codes, signals, re-exec, cleanup, `SIGTERM` while websockets are open)?
- **Secrets:** in each option, name exactly how `DATABASE_URL`/API keys stay unreadable by app code, and prove it (a test that reads `process.env` and `/proc/self/environ` from inside the app and finds nothing).
- **Fair queuing quantum.** Evaluate the owner's model — instrument each request, charge CPU spent inside it, and treat CPU-to-client as that flow's quantum so websockets stay fair. Key facts to settle with measurements: Blastit's own per-request CPU is measurable in Rust (the scheduler already has credit/fair-queue accounting); *app* CPU across `await` requires `AsyncLocalStorage` + `perf_hooks` sampling and is approximate; websocket traffic is per-frame rather than per-request. Decide whether CPU is a hard quantum, a scheduling signal on top of bytes/credit, or only used for blastit-side work, and say how a chatty websocket is prevented from starving HTTP flows.
- **Compatibility contract to document:** which Node/Vite versions, dev (`vite dev`) vs preview vs production, SSR + static split, streaming/responses, `upgrade`/websocket, HTTP/2 (probably out of scope), and what does not work.

Deliverables, in this order (DX before design, per the Iron Law — but the DX page here *compares two developer-facing shapes*, so it comes first and informs the ADR):

1. A lean DX comparison page under `docs/frameworks/node.md` (or a clearly marked draft section): what a developer types and sees in each shape — sidecar (`blastit serve --backend node`) vs addon (`import { init } from "@blastit/node"`), each across Local (zero `sudo`), CI/CD (`blastit check`), Deployments (fail-closed presets). Reviewed by `dx` and `ai-ergonomics` critics.
2. The ADR (`docs/decisions/ADR-000N-<slug>.md`) with the decision, the rejected options and why, the invariants each option breaks, and the fair-queuing model chosen.
3. A short measurement note (`docs/research/`) backing the perf/fairness claims: latency and syscalls per request per option where measurable, and the CPU-attribution feasibility result.

# Acceptance

- DX comparison page written and accepted by `dx` and `ai-ergonomics` goldfish critics, with zero-`sudo` local steps and a runnable `blastit check` path.
- ADR merged with an explicit decision, and a one-paragraph answer to *"can a compromised app read the secrets or touch the parser under this design?"* for the chosen option.
- If the addon is chosen: a spike proving it loads (`napi-rs` hello-world built and `require`d on this host), and a written note on how prebuilt binaries ship for the supported Node majors/platforms.
- If the sidecar/hybrid is chosen: a proof that `(req, res)` facades satisfy a real adapter (`@sveltejs/adapter-node` or Hono) end to end.
- Fair-queuing: a measured or explicitly-modelled statement of how CPU is attributed (including across `await`), and how a websocket flow is prevented from starving HTTP flows; if a hard quantum is rejected, say what replaces it.
- Any change to MB.4's ROADMAP exit criteria is escalated to `strategist` (never edited directly).

# Notes

- Do not silently re-architect: if the addon cannot preserve invariants 1–2, the ADR must say so plainly and recommend the hybrid, even though it is less elegant.
- The performance work just shipped (`MA.6`, 53 k req/s, −46 % CPU) is the baseline any embedding option must beat or match to be worth its risk.
- This node blocks MB.4 implementation and MB.4's docs, but not MB.2/MB.3.

# Owner direction (second message, 2026-11-13)

> "For the vite/http situation you do what you need to do in order to keep standard ways of working seamless (it would be acceptable to have a `blastit-vite` binary to facilitate things)."

Treat seamless-with-standard-workflows as a hard requirement, not a nice-to-have. The ADR must evaluate and pick a developer-facing shape that keeps the ordinary Vite loop intact:

1. **Dev — the `middlewareMode` seam.** Vite can run with `server.middlewareMode: true`, where the integrator supplies the HTTP server and Vite only handles transforms/HMR/SSR middleware. That is the natural place for Blastit to own the listener without fighting `vite dev`: a `blastit-vite` binary can boot Blastit's listener, mount Vite's middleware, and print the usual `Local: http://127.0.0.1:5173/` line. Evaluate that against a plain Vite plugin (`plugins: [blastit()]` with `configureServer`/`configurePreviewServer`) and say which one a developer actually prefers, including what happens on `vite.config.ts` reload and HMR websocket routing.
2. **Prod — no addon required.** `vite build` + the framework's node adapter handler behind `blastit serve --backend node` must stay the documented path, so nobody has to adopt the addon to deploy.
3. **Naming and packaging.** Decide whether `blastit-vite` is a subcommand (`blastit vite dev`) or its own binary, and how it is installed (`uv`/wheel vs npm package). Weigh: one more entry point vs "it just works when you type the command you already type". Prefer the shape that keeps `npm run dev`/`npm run preview` working with only a config change.
4. **Compatibility contract.** The ADR must state which of `vite dev`, `vite preview`, `vite build` output, HMR, SSR with an adapter handler, static assets (MB.2), and websocket `upgrade` are supported, and which are explicitly not — and the `docs/frameworks/node.md` page must match that contract verbatim.

# Owner clarifications (2026-11-13) and the decided sequence

**Fair queuing — settled: CPU time only.** Fair queuing charges the flow for CPU time consumed on its behalf, and nothing else. No byte quanta, no per-frame quanta. Bytes/backpressure stay where they belong — flow control — and must not be mixed into the fair-queue quantum.
- **Sticky accounting.** The measurement accumulates monotonically per flow (per connection), and is never reset per request or per message. A websocket's charge is everything its handler consumed since the connection opened: each inbound message that drives handler work, the work to produce what is sent back, and any timer/callback work attributable to that flow. A request's charge likewise includes its asynchronous continuations, not just the synchronous slice.
- **Approximate is fine, but say how.** Blastit-side CPU is measurable near-exactly; app-side CPU across `await` (`AsyncLocalStorage` + `perf_hooks` sampling) is approximate. Document the attribution boundaries and the error the scheduler tolerates, and keep the accumulator sticky across sampling gaps instead of dropping unmeasured slices.
- **Replenishment is per interval.** Each flow gets a budget per scheduler interval; because the charge is CPU, an idle flow costs nothing, and fairness is "equal CPU per interval", not "equal bytes" or "equal messages". A flow that blocks its own progress still occupies its slot — that is a liveness concern for flow control, not for the fair-queue quantum.
- **No client-readback coupling.** Credit must never be replenished by how much the client has read; readback is flow control's business.

**Trust boundary — settled: super tight, non-negotiable.** Invariants 1 and 2 are not tradeable for ergonomics, so an in-process parser (option a) and in-process secret handling are out. The addon, if any, may only pass the listening fd to the privileged supervisor (`SCM_RIGHTS`) and present facades; the parser and credentials stay outside the app's address space. If the spike shows the hybrid cannot be made to work, the answer is the plain sidecar — not a weakening.

## Sequence to follow (per owner delegation "you work out the sequence")

1. **Research** (`researcher`): napi-rs vs neon vs `node-api`; `SCM_RIGHTS` fd passing from a Node addon on Linux; how `@sveltejs/adapter-node`/Hono/Nuxt handlers consume `(req, res)`; Vite `middlewareMode` + `configureServer`/`configurePreviewServer`; what `vite dev`'s HMR websocket needs. Cited note under `docs/research/`.
2. **DX comparison page** (`doc-writer`) under `docs/frameworks/node.md`: the developer-facing shapes — sidecar (`blastit serve --backend node`), `blastit vite dev` wrapper, addon `init()` — each across Local (zero `sudo`), CI/CD (`blastit check`), Deployments (fail-closed presets). `dx` + `ai-ergonomics` critics must pass. **Docs before design still holds**: this page is what the developer types, not how it is built.
3. **ADR** (`architect`, escalated): the decision, the rejected options and the invariant each breaks, the fair-queue credit model above, the compatibility contract, and the packaging/naming answer (`blastit-vite` as alias vs its own binary).
4. **Spike with proof, before any production code**: addon loads on this host; fd passing to a separate uid works; a synthesized `(req, res)` facade satisfies a real adapter end to end; a test that reads `process.env` and `/proc/self/environ` from inside the app and finds no secrets.
5. **Implementation leaves** (`implementer`, split by seam): supervisor fd intake, facade layer, `blastit vite dev` wrapper, production `--backend node` upstream, `--learn`/`check` attribution for Node routes.
6. **e2e + `node2shell` suite + `test-curator` audit**, then MB.4's epic gate and a `RELEASE_READY: 0.2.2`.

Steps 1–3 may run while MB.3 is still finishing, but step 4 must not start until the ADR is accepted, and no step may be reordered to put code before the accepted DX page.
