---
id: MB.3.53
title: "body = raw without allow_raw must refuse at boot in production (stage 0 and stage 1), not just warn in check"
kind: task
status: done
owner: implementer
priority: 0
depends_on: []
paths:
  - crates/blastit/src/stage0.rs
  - crates/blastit/src/stage1.rs
  - crates/blastit/src/check.rs
  - "crates/blastit-manifest/src/validate.rs"
  - "crates/blastit/tests/**"
  - "docs/decisions/**"
  - docs/reference/manifest.md
  - docs/security.md
  - "e2e/**"
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-15T08:05"
updated: "2026-11-15T08:30"
---

# Goal

Close the fail-open: `[route.ingress] body = "raw"` without `allow_raw` must **refuse at
boot on the production path** (stage 0 and stage 1's re-read). `dev` keeps its loud
warning; `check` keeps the `raw_ingress` finding. This is the enforcement the docs have
described for two milestones and that the code has never done.

# Context

**The owner verified this by reading the code; it is not a suspicion.**

- `allow_raw` is parsed (`crates/blastit-manifest/src/parse.rs:811-856`) and read **only**
  by `audit_v1.rs:1066` (the `check` finding) and tests.
- `crates/blastit-ingress/src/lib.rs:284` — `plan_body` returns `StreamPassthrough` for
  `BodyCodec::Raw` **regardless of `allow_raw`**.
- Nothing at boot consults it (`stage0`/`stage1` have no reference).
- Consequence: a production manifest with `[route.ingress] body = "raw"` and no
  `allow_raw` boots, serves, and streams unvalidated bodies to the app. `check`'s own
  `suggest_fix` *writes* the key for the reader, and no preset runs `check`, so the only
  enforcement is a lint nobody runs.

That contradicts the product's central claim ("every inbound byte decoded, validated,
re-encoded") and is the same class as `.2`/`.28`: a capability that reports success while
doing nothing.

**Do not re-decide the policy.** `docs/design/learn-single-pass.md` (`MB.3.51`, ruling 4)
records the decision and its analysis; this node implements it and writes the ADR that
`MB.3.51` says supersedes ADR-0006 decision 3.

# Acceptance

```console
$ cargo test -p blastit --locked --test critical_raw_ingress
$ cargo test -p blastit-manifest --locked
$ make check
$ uv run --no-project tools/critical_guard.py --base b2d0fa2 --node MB.3.53
```
1. **Stage 0**: a production boot of a manifest with `[route.ingress] body = "raw"` and no
   `allow_raw` exits `1`, writes the refusal to the right stream (stage-0 refusals are raw
   text on stderr; stage-1 refusals are `app.output` JSON on stdout — check which side
   your refusal lands on and assert it), names the route **and** the key, and binds no
   listener.
2. **Stage 1's re-read**: a manifest edited between the stages cannot slip through — the
   same refusal fires on stage 1's re-parse (`stage1.rs:254-262`). Pin it by making the
   edit land between the stages.
3. **Accepted twin**: with `allow_raw = true` the same manifest boots and **serves the raw
   body intact** (byte-for-byte), so the fix is a gate, not a blanket refusal.
4. **`dev` keeps its warning** (the shim of a local workflow) and **`check` keeps the
   `raw_ingress` finding** — neither half may be deleted to make this pass.
5. **The witness is the point**: removing the refusal reddens the protected test (paste
   both runs). The bug was a dead switch; a test that cannot fail is the same bug.
6. **Presets and examples**: grep the repository for `body = "raw"` (presets, docs
   examples, `e2e/`, fixtures). Every one must now set `allow_raw = true` explicitly, and
   the page that shows it must say why — or that preset is broken at boot. List what you
   changed and anything outside your paths.
7. `docs/decisions/ADR-0011-<slug>.md`: the decision, citing
   `docs/design/learn-single-pass.md` ruling 4 for the analysis and superseding ADR-0006's
   decision 3; the three environments (production refuses, dev warns, check reports) and
   what would make it wrong.
8. `make check` exit 0; workspace clippy `-D warnings` clean; no new dependency.

# Notes

- The stage-0 refusal must be a real boot refusal (`exit 1`, nothing bound), not a startup
  warning that a request can outrun.
- Keep `check`'s `raw_ingress` finding: it is the CI half of the same rule, and it is what
  tells a developer *before* the boot.
- `docs/reference/manifest.md`'s `allow_raw` row must say what now happens without it.
