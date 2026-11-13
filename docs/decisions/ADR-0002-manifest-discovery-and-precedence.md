# ADR-0002: Manifest Auto-Discovery, App Wiring Keys, and Setting Precedence

- **Status:** Accepted
- **Date:** 2026-11-12
- **Node:** MA.5
- **Owner:** architect

## Context

In `0.1.0`, starting Blastit required setting `BLASTIT_APP` in the environment and defaulted `BLASTIT_MANIFEST` to `/app/blastit.toml`, forcing redundant flags or env vars in local shells, CI workflows, and Dockerfiles. Meanwhile, placing top-level keys such as `groups` after a `[table]` in TOML silently attaches keys to the preceding table unless caught explicitly (`M10`), and treating empty `BLASTIT_*=""` environment variables as "unset" causes subtle CI template surprises where an explicit empty override silently reverts to a preset or manifest value.

## Decision

1. **CWD-only `./blastit.toml` auto-discovery:**
   - `BLASTIT_MANIFEST` defaults to `./blastit.toml` (resolved relative to stage-0 working directory) across `blastit`, `blastit dev`, and `blastit check` (`blastit test`).
   - **No upward parent-directory traversal** is performed: monorepos never accidentally pick up an ancestor's security policy. Neither `dev` nor `check` accepts a `--manifest` CLI flag (`dev` exits `1`, `check` exits `2`); `BLASTIT_MANIFEST` is the single override mechanism (`blastit-manifest fold` retains its explicit `--manifest PATH` argument because it operates offline on arbitrary log files).
2. **Non-secret `app` and `preload` keys in `blastit.toml`:**
   - `blastit.toml` accepts top-level `app = "module:attribute"` and `preload = "mod"` or `preload = ["mod1", "mod2"]` (both bare string and string array, symmetric with `path`; `BLASTIT_PRELOAD` remains comma-separated).
   - Neither key holds secrets; both live alongside the routes they serve so `CMD ["blastit"]`, `uv run blastit dev`, and `uv run blastit check` need zero app-wiring environment variables. At least one of `manifest.app` or `BLASTIT_APP` is required.
3. **Top-level key ordering guard:**
   - `version`, `app`, `preload`, and global `groups` must appear above the first `[table]` / `[[route]]`.
   - Writing `version`, `app`, or `preload` inside any `[table]` or `[[route]]`, or writing `groups` inside any non-`[[route]]` `[table]` (`[proxy.*]`, `[group.*]`, `[route.<proxy>]`, `[route.ingress]`), fails manifest parsing with: `<key> after a [table] is not top-level; move it above the first table`. (`groups = [...]` inside `[[route]]` itself remains the canonical route-level group attachment.)
4. **Two-part override precedence (preset selector vs setting values):**
   - **Preset selector (`blastit` production only):** `--preset NAME` overrides `BLASTIT_PRESET` (emitting `Warning::PresetFlagWins`). `blastit dev` and `blastit check` reject `--preset` and ignore `BLASTIT_PRESET`.
   - **Setting values:** **built-in default < active preset's values < `blastit.toml` (`app`, `preload`) < explicit environment variable (`BLASTIT_*`, `PORT`) < value CLI flags (`blastit dev --port`, `blastit dev --host`)**.
   - Environment presence is tested via `std::env::var_os(name).is_some()`: a **set** `BLASTIT_*` variable (or `PORT`) always wins over presets and the manifest **even when its value is `""`**. Accordingly, `BLASTIT_PRELOAD=""` clears `manifest.preload` to `[]`, and `BLASTIT_APP=""` or `BLASTIT_MANIFEST=""` fails validation rather than silently falling back (`BLASTIT_PRESET` retains whitespace-trimmed unset semantics per `ARCHITECTURE.md` §8.2).

## Consequences

- **Positive:** `Dockerfile` `CMD ["blastit"]` (with `WORKDIR /app`), `uv run blastit dev`, and `uv run blastit check` all discover `./blastit.toml` and `app` with zero boilerplate; `--preset` never overrides an explicit `BLASTIT_*` env var; TOML scoping mistakes fail at boot with an exact message.
- **Trade-offs:** CI workflows using `${{ vars.OPTIONAL }}` must omit unset `BLASTIT_*` variables rather than exporting empty strings when manifest fallback is desired (`docs/reference/settings.md`).
