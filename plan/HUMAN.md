# HUMAN inbox

Requests for the project owner. Agents add items with `uv run --no-project tools/plan.py --agent <role> human add "..."`.

To answer, write under **Answer:**, save, and run `/human` (or tell the conductor "answered"). Never write raw secrets here — put credentials in `.env` and reply "added to .env".

## H-001 [closed] (node MA)

- asked: 2026-11-12T01:39 by conductor

Raise expensive_cap_usd in plan/BUDGET.json (currently $238.88 spent of $100 cap = 239%, status EXHAUSTED). The autopilot plugin now blocks strategist/architect/expert, so (a) the MA milestone gate cannot run, (b) core DX design acceptance cannot be escalated, (c) any expert rescue on hard Rust/Linux/WASM work is unavailable. I have re-scoped MA.1 to the cheap doc-writer/designer tier so the roadmap keeps moving, but MA/MB/MC/MD gates will need strategist. Please raise the cap (e.g. to $400) or confirm the roadmap should run gates on the cheap tier instead.

**Answer:**

## H-002 [open] (node MA)

- asked: 2026-11-12T22:58 by lead

Install cargo-mutants on the dev host (or allow a one-off `cargo install cargo-mutants`): the MA milestone gate requires `make mutants-rust PKG=blastit-manifest` and `PKG=blastit-sqlpolicy` (>=75% killed), but the binary is missing so no mutation audit can run locally.

**Answer:**

