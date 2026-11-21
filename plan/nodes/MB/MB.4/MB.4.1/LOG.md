# Log MB.4.1

## 2026-11-13T22:15 [conductor]

created (task) Decide Node integration topology: napi-rs/neon embedding vs sidecar fd handoff, and the fair-queue CPU model (ADR)

## 2026-11-13T22:19 [conductor]

Owner direction (2nd): keep standard Vite workflows seamless; a blastit-vite binary is acceptable. ADR must weigh the Vite middlewareMode seam (Blastit owns the listener, Vite mounts as middleware) vs a plugins:[blastit()] integration, keep 'vite build + adapter handler behind blastit serve' as the prod path, and decide subcommand vs own binary + packaging.

## 2026-11-13T22:24 [conductor]

Owner settled both open points: (1) CPU is debited against the flow, approximate attribution acceptable — credit still replenished on a scheduler tick, per-frame for websockets, app-side CPU as a signal; (2) trust boundary non-negotiable, so in-process parsing/secrets are out and the addon may only pass the listening fd + present facades. Conductor fixed the 6-step sequence (research -> DX comparison -> ADR -> spike with proof -> implementation leaves -> e2e/node2shell) in NODE.md.

## 2026-11-13T22:27 [conductor]

Owner correction (supersedes the per-frame note): fair queuing charges CPU time ONLY, never bytes and never frames. Accounting is sticky/monotonic per flow (per connection), never reset per request or message; a websocket is charged for inbound-message handling, reply work and attributable timer/callback CPU since the connection opened; a request includes its async continuations. App-side attribution via AsyncLocalStorage+perf_hooks is approximate but must stay sticky across sampling gaps. Budget replenished per scheduler interval; no client-readback coupling (that is flow control).
