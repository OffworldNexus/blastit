---
id: MB.5
title: "Tomcat / JVM 12-Factor Backend & Unified Multi-Backend Gate (0.2.3)"
kind: epic
status: ready
priority: 2
depends_on: [MB.4]
paths: ["e2e/**"]
attempts: 0
max_attempts: 3
escalation: none
created: "2026-11-13T10:42"
updated: "2026-11-13T10:42"
---

# Goal

Ship **Tomcat / JVM** 12-factor Servlet / Spring Boot backend support (`0.2.3`) and verify unified cross-backend `blastit dev --learn` + `blastit check` workflows across Python, PHP, Node, and JVM, starting Docs-First.

# Context

- Mandatory DX-First sequence (`docs/frameworks/jvm.md` + `inventory.toml` + `dx`/`ai-ergonomics` critics -> local-socket/loopback JVM upstream & JDBC/PostgreSQL socket wiring design + ADR -> leaf implementation + protected `critical_*.rs` tests -> multi-backend `e2e/` + `MB` milestone readiness).

# Acceptance

- [ ] `docs/frameworks/jvm.md` covering Local (zero `sudo`), CI/CD (`blastit check`), and Deployments accepted by `dx` and `ai-ergonomics` critics before Rust code begins.
- [ ] Embedded/standalone Tomcat / Spring Boot services run under Blastit per-route sandboxing, stage-0 credential barrier, and PostgreSQL proxy grants without app code changes.
- [ ] `blastit dev --learn` and `blastit check` (`--json`) work identically across Python, PHP, Node, and JVM; `make check`, `make docs-check`, `visual-checker`, `make e2e`, and `test-curator` audit pass.
