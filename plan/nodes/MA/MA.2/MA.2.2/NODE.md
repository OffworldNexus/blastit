---
id: MA.2.2
title: "Manifest 0.1.1, ingress enforcement and wire learning"
kind: epic
status: done
priority: 2
depends_on: [MA.2.1]
paths:
  - "crates/blastit-manifest/**"
  - "crates/blastit-http/**"
  - "crates/blastit-scheduler/**"
  - "crates/blastit-dbproxy/**"
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-12T13:12"
updated: "2026-11-12T15:09"
---

# Goal

Container for the manifest `0.1.1` schema and the request-time enforcement path: `app`/`preload`, `[route.ingress]`, reserved-key guards and literal rejection, the comment-preserving surgical editor with atomic writes, early HTTP upload abort, scheduler body planning, and `dbproxy` learn/redaction modes.

# Context

- Technical design: `docs/design/ma-lean-dx.md` §4.2 B/D, §5, §7.2, §8 leaves 4–7; `ADR-0002`, `ADR-0003`, `ADR-0006`; requirements `R7..R8`, `R11..R16`, `R24..R26`, `R28..R29`, `R31`.
- Children: `MA.2.2.1` gates the other three; `MA.2.2.2` follows it because both touch `crates/blastit-manifest/src/lib.rs`; `MA.2.2.3` and `MA.2.2.4` then run in parallel with disjoint crate paths.
- DX status: the manifest behaviour is already documented (`docs/reference/manifest.md`); this epic must not change user docs or re-open accepted DX decisions.

# Acceptance

- [ ] `cargo test -p blastit-manifest --test critical_manifest_011 --locked`, `--test critical_manifest_edit --locked`, `-p blastit-http --test critical_early_abort --locked`, `-p blastit-scheduler --test critical_ingress_enforcement --locked`, `-p blastit-dbproxy --test critical_learn_and_redaction --locked` all pass.
- [ ] `make check` passes; every listed critical suite has an allow case and a deny/adversarial case.
- [ ] No value-bearing SQL literal can reach a `sql.denied` event, `obs_fd` record, or `check` finding (`R24`), and `apply_edits` provably re-parses unchanged fields.
- [ ] No leaf edited `docs/**`, `python/**`, or root `Cargo.toml`/`Cargo.lock`.

# Notes

- Enforcement is fail-closed: undeclared/none bodies get `413` before Python runs, and early responses close the client socket without waiting for `body_timeout`.
