# HUMAN inbox

Requests for the project owner. Agents add items with `uv run --no-project tools/plan.py --agent <role> human add "..."`.

To answer, write under **Answer:**, save, and run `/human` (or tell the conductor "answered"). Never write raw secrets here — put credentials in `.env` and reply "added to .env".

## H-001 [closed] (node MA)

- asked: 2026-11-12T01:39 by conductor

Raise expensive_cap_usd in plan/BUDGET.json (currently $238.88 spent of $100 cap = 239%, status EXHAUSTED). The autopilot plugin now blocks strategist/architect/expert, so (a) the MA milestone gate cannot run, (b) core DX design acceptance cannot be escalated, (c) any expert rescue on hard Rust/Linux/WASM work is unavailable. I have re-scoped MA.1 to the cheap doc-writer/designer tier so the roadmap keeps moving, but MA/MB/MC/MD gates will need strategist. Please raise the cap (e.g. to $400) or confirm the roadmap should run gates on the cheap tier instead.

**Answer:**

## H-002 [closed] (node MA)

- asked: 2026-11-12T22:58 by lead

Install cargo-mutants on the dev host (or allow a one-off `cargo install cargo-mutants`): the MA milestone gate requires `make mutants-rust PKG=blastit-manifest` and `PKG=blastit-sqlpolicy` (>=75% killed), but the binary is missing so no mutation audit can run locally.

**Answer:** Allowed — run a one-off `cargo install cargo-mutants` and use the real mutation audit as MA gate evidence (owner answered 2026-11-13).

## H-003 [closed] (node MA)

- asked: 2026-11-13T03:12 by conductor

OpenRouter credits exhausted: every strategist/architect/expert request fails with 'requires more credits ... you requested up to 256000 tokens, but can only afford ~79-80k' on key c048f174. The MA (0.1.1) milestone gate is mandatory for the expensive tier, so with credits gone the roadmap cannot advance past MA even though MA's work is complete and green (make check + make e2e PASS at 4338f88). Please raise the key's weekly limit (or top up credits). Also still open: H-002 install cargo-mutants for the gate's literal mutation audit.

**Answer:** Add $100 to the budget — `expensive_cap_usd` raised from 100 to 200 (owner answered 2026-11-13).

