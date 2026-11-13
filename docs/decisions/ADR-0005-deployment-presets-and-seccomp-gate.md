# ADR-0005: Deployment Presets as Overridable Defaults with Mandatory Seccomp Gate

- **Status:** Accepted
- **Date:** 2026-11-12
- **Node:** MA.5
- **Owner:** architect

## Context

Production deployments on DigitalOcean App Platform (root under gVisor behind DO's edge) and Docker Compose behind Traefik v3 (rootless dedicated UID with file capabilities on a private bridge network) require coordinated defaults for client-IP extraction, trusted proxies, fair queuing, and JSON logging (`ARCHITECTURE.md` §8.2). Making presets opaque "modes" would prevent operators from overriding individual knobs, while allowing `BLASTIT_SECCOMP=0` silently in production or under a preset would remove Blastit's only syscall and outbound-TCP (`egress`) wall (`S6`).

## Decision

1. **Presets are pure, inspectable default tables scoped to production `blastit` (`digitalocean-paas`, `compose-traefik`):**
   - Selected in production `blastit` (`Command::Serve`) via `--preset NAME` (`--preset=NAME`) in stage 0 or `BLASTIT_PRESET=NAME` (`--preset` wins over `BLASTIT_PRESET` as the preset selector with `Warning::PresetFlagWins`; `--stage=1`, `--role=*`, `blastit dev`, and `blastit check` reject `--preset`, and `dev`/`check` ignore `BLASTIT_PRESET` so exported shell presets never alter local/CI runs). Unknown preset names abort startup (`exit 1`) and list valid presets.
   - **Every explicitly set `BLASTIT_*` variable (and `PORT`) overrides the preset's setting values, even when empty** (`built-in default < preset values < manifest < set env`).
   - Both presets set `BLASTIT_TRUSTED_PROXIES=*`, `BLASTIT_FAIR_QUEUING=on`, and `BLASTIT_LOG_FORMAT=json`; `digitalocean-paas` sets `BLASTIT_CLIENT_IP_HEADER=do-connecting-ip` (assuming `Mode::Root`), while `compose-traefik` sets `BLASTIT_CLIENT_IP_HEADER=x-forwarded-for` (assuming `Mode::Capabilities`).
   - The `config` `boot` event records `preset`, `preset_settings` (the preset's own values), and `preset_overridden` (names overridden by explicit env vars), plus a `warn` event (`preset_expects_root` / `preset_expects_rootless`) if the detected privilege mode differs from the preset's assumption.
2. **Mandatory `seccomp` enforcement and preset hard-abort on `BLASTIT_SECCOMP=0` (`S6`):**
   - `BLASTIT_SECCOMP` defaults to `1`. If `seccomp` filter installation fails at runtime in any child role, that child exits immediately (`_exit(1)`) and `init` aborts the server (fail-closed).
   - Setting `BLASTIT_SECCOMP=0` **aborts startup (`exit 1`) whenever any preset is active** (`config.preset.is_some()`).
   - Outside a preset, setting `BLASTIT_SECCOMP=0` is honoured only for emergency debugging on exotic kernels and emits an unmistakable `WARN` `boot` / `manifest.warning` event stating that syscall and `egress` filtering are disabled.

## Consequences

- **Positive:** Single-flag deployment ergonomics (`blastit --preset digitalocean-paas`) with full transparency in `boot` logs, explicit `BLASTIT_*` override guarantee, and impossible accidental removal of `seccomp` on preset deployments.
- **Trade-offs:** Because both presets set `BLASTIT_TRUSTED_PROXIES=*` (taking `do-connecting-ip` or rightmost `X-Forwarded-For`), deployments must never publish Blastit's container port directly to untrusted networks (`docs/deploy/index.md`).
