---
id: MB.40.4.1
title: Split crates/blastit/src/config.rs into cohesive modules + serde wire structs
kind: task
status: done
owner: implementer
priority: 1
depends_on: []
paths: [crates/blastit/src/config.rs, "crates/blastit/src/config/**", Cargo.toml, Cargo.lock]
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T03:13"
updated: "2026-11-19T03:32"
---

# Goal

Behaviour-preserving split of `crates/blastit/src/config.rs` (4,463 lines / ~2,081 prod LOC, 61 free fns) into `crates/blastit/src/config/{mod.rs,preset.rs,parse.rs,manifest_io.rs,wire.rs}`, and replacement of the 16 hand-rolled `json_string`/`json_u64`/... accessors with `#[derive(Serialize, Deserialize)]` wire structs consumed by `Config::from_json`. Every resulting production file must stay under 500–600 prod (non-blank, non-comment) LOC.

This is a **pure refactor**: no behaviour, public API, error sentence, JSON field name or test may change.

# Exact Targets (file:line pre-diagnosed)

All line numbers are from the pre-split `crates/blastit/src/config.rs` at `MB.40.4` closure (commit `fd9e28e`); do not re-audit, use them directly.

**`wire.rs` — the JSON contract (replace, do not merely move):**
- 16 accessors at `:2772-2975`: `json_string` `:2772`, `json_opt_string` `:2779`, `optional_path` `:2793`, `php_runtime_from_json` `:2807`, `json_string_list` `:2834`, `json_optional_string_list` `:2857`, `json_string_lists` `:2868`, `json_u64` `:2892`, `json_u32` `:2898`, `json_u8` `:2903`, `json_opt_u32` `:2908`, `json_usize` `:2915`, `json_i32` `:2920`, `json_u16` `:2927`, `json_f64` `:2932`, `json_bool` `:2938`, `json_optional_bool` `:2952`, `json_duration` `:2959`.
- `Config::from_json` `:1947-2060` is the only consumer and becomes the `serde_json::from_value::<WireConfig>` call site; `Settings` `:2410` is the raw-merge struct next to it.
- `Preset::from_json` `:269`, `preset_from_json` `:2077`, `preset_choice` `:2186` — nested enum decoding that must keep its exact `ConfigError::Document { field, reason }` wording.

**`preset.rs`:** `Preset` `:63`, `preset_list` `:160`, `preset_help` `:169`, `PresetChoice` `:592`, `PresetPolicy` `:967`, `preset_choice` `:2186`, `preset_from_json` `:2077`.

**`parse.rs`:** `parse_u16` `:2486`, `parse_u32` `:2493`, `parse_i32` `:2500`, `parse_u64` `:2507`, `parse_usize` `:2514`, `parse_u8` `:2522`, `parse_client_ip_header` `:2581`, `parse_trusted_proxies` `:2593`, `parse_bool` `:2614`, `parse_immortalize` `:2624`, `parse_duration` `:2642`, `parse_size` `:2684`, `parse_size_usize` `:2724`, `parse_percent` `:2731`.

**`manifest_io.rs`:** `config_document`-style discovery helpers — `view_dir` `:2061`, `seccomp_requested` `:2163`, `manifest_path` `:2259`, `missing_manifest` `:2282`, `display_path` `:2297`, `read_manifest` `:2346`, `mount_point` `:2977`.

**`mod.rs`:** module docs, re-exports, `LogFormat` `:179`, `Privilege` `:203`, `Mode` `:221`, `SpawnMode` `:303`, `Immortalize` `:328`, `Warning` `:380`, `ConfigError` `:536`, `Config` `:673`, `impl Config` `:645` / `:975`, remaining free fns, and the in-file tests (`mod tests` `:2997`).

# Hard constraints

1. **Stage-0 re-exec contract.** The `serde_json::Value` read by `Config::from_json` is serialised by `config_document` and handed to the re-exec'd child via `--config-fd`. Wire-struct field names (`#[serde(rename = ...)]` where needed), optionality, integer widths/ranges and default handling must reproduce the hand-written accessors **exactly**.
2. **Error wording is asserted by tests.** `ConfigError::Document { field, reason }` sentences are compared in-file (`:3250-4230`) and by `crates/blastit/tests/*`. A serde error must be mapped back to the same `field`/`reason` text — no `serde_json::Error` strings leaking into user output.
3. **Public surface unchanged.** `crates/blastit/src/lib.rs:54` (`pub mod config;`) must keep resolving with no edit; `Preset`, `preset_list`, `preset_help`, `LogFormat`, `Privilege`, `Mode`, `SpawnMode`, `Immortalize`, `Warning`, `ConfigError`, `PresetChoice`, `Config`, `PresetPolicy`, `view_dir`, `seccomp_requested`, `mount_point`, `manifest_path`, `missing_manifest`, `display_path`, `read_manifest` keep identical paths/visibility/signatures.
4. **serde derive is already a dependency** (`crates/blastit/Cargo.toml:79`, features `["derive"]`). No manifest change is expected; `Cargo.toml`/`Cargo.lock` are listed in `paths` only in case a feature bump is genuinely unavoidable.
5. **No protected tests are involved.** `crates/blastit/tests/critical_*.rs` need no edit and must not be touched.
6. **No new globals** (`static`/`OnceLock`/`lazy_static`), no new traits, no `unwrap()`/`expect()` on parsed input.

# Acceptance

```
make fast-check
cargo test -p blastit --locked
cargo clippy -p blastit --all-targets --locked -- -D warnings
cargo fmt --all -- --check
# every production file under the 600-line bar:
for f in crates/blastit/src/config/*.rs; do
  printf '%s %s\n' "$(awk 'NF && $1 !~ /^\/\// ' "$f" | wc -l)" "$f"
done
```

Report the command tails in `RESULT.md`, plus one line confirming `git diff --stat` shows only `crates/blastit/src/config.rs` deleted and `crates/blastit/src/config/` added, and that `git diff crates/blastit/tests` is empty.
