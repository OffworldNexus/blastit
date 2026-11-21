# Architecture Decision Records (ADRs)

Numbered 1-page records (`ADR-NNNN-<slug>.md`) of lasting UX, protocol, and security decisions. See skill `design-docs`.

| ADR | Title | Status | Node |
|---|---|---|---|
| [ADR-0001](ADR-0001-unprivileged-dev-simulation.md) | Unprivileged Single-UID `blastit dev` Simulation vs Fail-Closed Production | Accepted | MA.5 |
| [ADR-0002](ADR-0002-manifest-discovery-and-precedence.md) | Manifest Auto-Discovery, App Wiring Keys, and Setting Precedence | Accepted | MA.5 |
| [ADR-0003](ADR-0003-wire-learn-and-literal-redaction.md) | Wire-Level `--learn`, SQL Literal Redaction/Refusal, and Atomic Manifest Writes | Accepted | MA.5 |
| [ADR-0004](ADR-0004-framework-agnostic-check-and-json-contract.md) | Framework-Agnostic `blastit check` Architecture and `--json` Machine Contract | Accepted | MA.5 |
| [ADR-0005](ADR-0005-deployment-presets-and-seccomp-gate.md) | Deployment Presets as Overridable Defaults with Mandatory Seccomp Gate | Accepted | MA.5 |
| [ADR-0006](ADR-0006-route-ingress-body-gate.md) | Fail-Closed `[route.ingress]` Body Validation and Canonical Re-Encoding | Accepted (decision 3 superseded by `ADR-0011`) | MA.5 |
| [ADR-0007](ADR-0007-static-asset-serving-and-rewriting.md) | Static Asset Serving and Rewrite Precedence Inside the Scheduler | Proposed | MB.2.2 |
| [ADR-0008](ADR-0008-fastcgi-backend-and-param-lockdown.md) | Injected `Upstream` Contract (`Upstream` contract lives on in ADR-0009; the FastCGI/pool design it described was withdrawn before release) | Withdrawn (`ADR-0010` replaced it) | MB.3.3 / MB.3.32 |
| [ADR-0009](ADR-0009-runtime-providers-and-capability-gating.md) | Runtime Providers, Capability-Gated Python, CPU Fair Queuing & Distribution | Accepted (superseded in part by `ADR-0016`) | MB.12 |
| [ADR-0010](ADR-0010-php-embedded-via-libphp-embed.md) | Embedded PHP Execution via Host NTS `libphp.so` and Per-Minor SAPI Shims (`0.2.0`) | Accepted (superseded in part by `ADR-0016`: the distribution command spelling) | MB.12 |
| [ADR-0011](ADR-0011-raw-ingress-boot-refusal.md) | `body = "raw"` Without `allow_raw = true` Refuses the Production Boot (supersedes `ADR-0006` decision 3) | Accepted | MB.3.53 |
| [ADR-0012](ADR-0012-blastfile-dsl.md) | The Blastfile DSL: LL(1) grammar, no header, zero-dependency lexer/parser, lossless CST, and the `workers` removal | Accepted | MB.21 |
| [ADR-0013](ADR-0013-blastfile-lsp-and-zed.md) | `blastit lsp` and the Zed Blastfile extension | Accepted | MB.22 |
| [ADR-0014](ADR-0014-dev-fanotify-live-reload.md) | Live reload in `blastit dev`: one unprivileged file-handle group, exact loaded-file tracking, one all-views reload | Accepted | MB.23 |
| [ADR-0016](ADR-0016-view-crates-over-wire.md) | View Crates Over the Wire: `blastit-view` as Transport and One Crate per View Kind (`0.2.0`) | Proposed | MB.36.1 |
| [ADR-0017](ADR-0017-universal-distribution.md) | Universal `libpython`-Free `blastit` Binary, Per-Minor Python & PHP 8.3–8.6 Shims, Signed APT/RPM Repositories & Safe Local CLI Handoff | Accepted | MB.42.1 |
