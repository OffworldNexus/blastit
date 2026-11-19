# MB.40.4.1 — `crates/blastit/src/config.rs` split + serde wire structs

STATUS: done

Behaviour-preserving refactor. `crates/blastit/src/config.rs` (4,463 lines: 2,081
prod LOC over lines 1–2,990 plus 1,177 test lines) is now a cohesive module tree
under `crates/blastit/src/config/`, and the stage-1 document is read through a
`#[derive(Deserialize)]` wire struct instead of 18 hand-rolled `json_*`
accessors.

## Module layout

| File | Lines | Prod (non-blank, non-`//`) | What lives there |
|---|---:|---:|---|
| `config/mod.rs` | 65 | 30 | Module docs + map, submodule decls, the public re-exports (unchanged paths), `invalid`/`document` constructors |
| `config/types.rs` | 416 | 260 | `LogFormat`, `Mode`, `Privilege` (+`lifecycle`, `from_parts`), `SpawnMode`, `Immortalize`, `Warning`, `ConfigError` |
| `config/settings.rs` | 416 | 196 | `Config`, `Default`, `TRUSTED_PROXIES_DEFAULT`, `MIN_DEBT_TTL`, `view_worker_cap`, `client_ip`, `fair_config`, `component_nofile`, `max_connections_for`, `limits` |
| `config/env.rs` | 515 | 421 | `Config::{from_env, from_env_with_format, for_dev, for_check}`, `client_ip_settings`, `proxy_max_connections`, `proxy_key`, `variable`, `defaulted` |
| `config/parse.rs` | 218 | 172 | The 14 `BLASTIT_*` parsers, `seconds`, `split_list`, `check_trusted_proxies` |
| `config/preset.rs` | 331 | 173 | `Preset`, `PRESETS`, `preset_list`, `preset_help`, `PresetChoice`, `PresetPolicy`, `Settings`, `preset_choice`, `preset_assumption_warning` |
| `config/validate.rs` | 180 | 146 | `Config::validate`, `clamp_fair_queue` |
| `config/manifest_io.rs` | 218 | 111 | `MANIFEST_ENV`, `manifest_path`, `missing_manifest`, `display_path`, `ManifestReadError`, `read_manifest`, `manifest_dir`, `manifest_wiring`, `seccomp_requested`, `view_dir`, `mount_point` |
| `config/document.rs` | 340 | 310 | `Config::to_json` (moved verbatim), `millis`, `latin1` |
| `config/wire/mod.rs` | 418 | 325 | `WireConfig`, `Config::from_json`, `into_config`, `PresetMember`/`PresetWire`, `PhpMember`/`PhpWire`, `Privilege::from_json`/`PrivilegeParts`, `document_error` |
| `config/wire/field.rs` | 172 | 152 | `JsonField` trait, the field markers, `DOCUMENT_FIELDS` |
| `config/wire/read.rs` | 474 | 386 | The typed readers (`Text`, `OptText`, `WireU32`…, `Ms`, `Names`, `NameMap`, `Bytes`, `OptBytes`, `LogFmt`, `Spawn`, `Immortal`, `LevelText`, `Warns`, `ConnCaps`) |
| `config/tests.rs` | 1,673 | 1,336 | The in-file unit tests (moved unchanged, plus the contract and review-round tests) — **not production** |
| `config/tests_php.rs` | 88 | 65 | The `#[cfg(all(test, feature = "php"))]` PHP tests — **not production** |

Every production file is ≤ 421 prod LOC (bar: 500–600); the largest total file is
`env.rs` at 515 lines (doc comments included).

`wire.rs` became `wire/{mod,field,read}.rs`: the contract, its field names and
its readers are three concerns and together were ~830 prod lines, which is over
the bar in one file. `document.rs` (the writer) was separated from `wire/` (the
reader) for the same reason.

## The 18 accessors → wire structs

| Accessor (old `:line`) | Replacement |
|---|---|
| `json_string` `:2772` | `Text<F>` |
| `json_opt_string` `:2779` | `OptText<F>` |
| `optional_path` `:2793` | `OptText<F>` + `#[serde(default)]` on `libphp`/`php_shim_dir` |
| `php_runtime_from_json` `:2807` | `PhpWire` (derived) + `PhpWire::into_runtime` |
| `json_string_list` `:2834` | `Names<F>` |
| `json_optional_string_list` `:2857` | `OptNames<F>` + `#[serde(default)]` (`capabilities`) |
| `json_string_lists` `:2868` | `NameMap<F>` (`proxy_outputs`) |
| `json_u64` `:2892` | `WireU64<F>` |
| `json_u32` `:2898` | `WireU32<F>` |
| `json_u8` `:2903` | `WireU8<F>` |
| `json_opt_u32` `:2908` | `OptU32<F>` |
| `json_usize` `:2915` | `WireUsize<F>` |
| `json_i32` `:2920` | `WireI32<F>` |
| `json_u16` `:2927` | `WireU16<F>` |
| `json_f64` `:2932` | `Float<F>` |
| `json_bool` `:2938` | `Bool<F>` |
| `json_optional_bool` `:2952` | `OptBool<F>` + `#[serde(default)]` (`bounding_set`) |
| `json_duration` `:2959` | `Ms<F>` (ms-as-`f64`, `seconds()` bound, “is out of range”) |
| `field()` helper `:2766` | `document_error`, which maps serde's `missing field \`x\`` back to `x: the field is missing` |
| `preset_from_json` `:2077` | `PresetMember` + `PresetWire` (hand impl, same sentences) |
| `Privilege::from_json` `:269` | kept as a public method, now `PrivilegeParts` + `Privilege::from_parts` |

Each reader writes its own `<json key>: <reason>` sentence. `document_error`
splits it back into `ConfigError::Document { field, reason }`, so **no
`serde_json` wording reaches an operator**: the user-visible text is the sentence
the accessor gave, unchanged.

## Proof the `--config-fd` contract is unchanged

1. **The writer is the same code.** `to_json` was moved byte-for-byte into
   `document.rs` (`Method::` calls, key order, `millis()` `f64` durations, the
   `null`s, latin-1 paths), so stage 0 writes exactly the same bytes.
2. **The reader is pinned by a new test**,
   `config::tests::the_document_key_set_is_the_stage_0_contract`: it builds a
   fully populated `Config`, asserts the *exact* key set (60 keys without `php`,
   61 with the feature), the D6 nesting (`privilege`/`capabilities`/`bounding_set`), the
   preset's `name`/`supplied`/`overridden`, reads it back through the wire structs
   (`Config::from_json`) to the identical `Config`, and asserts
   `to_json → from_json → to_json` is byte-identical. It also proves the two
   absent-tolerant members (`libphp`, `php_shim_dir`, `php`) still read as “none”
   when a pre-`MB.3.13.2` document lacks them, and that a *required* member
   (`request_timeout_ms`) still fails with its own name.
3. **Every pre-existing document test is unchanged and green** — including
   `the_json_document_round_trips`, `a_document_missing_a_field_names_it`,
   `a_privilege_mode_the_document_cannot_carry_names_the_field`,
   `a_sys_path_entry_outside_latin1_is_refused`, the `cwd` round trip,
   `the_configuration_warnings_travel_to_stage_1` and
   `the_stage_one_document_carries_the_preset`. Only the `use` lines of the moved
   test module changed (the private names it glob-imports now live in sibling
   modules); no assertion, test name or attribute was edited.
4. `git diff --stat -- crates/blastit/tests` is empty: no protected or
   integration test was touched.
5. Public surface unchanged: `pub mod config;` in `lib.rs:54` untouched; the 20
   public names (`Config`, `ConfigError`, `LogFormat`, `Mode`, `Privilege`,
   `SpawnMode`, `Immortalize`, `Warning`, `Preset`, `PRESETS`, `preset_list`,
   `preset_help`, `PresetChoice`, `PresetPolicy`, `TRUSTED_PROXIES_DEFAULT`,
   `MIN_DEBT_TTL`, `view_dir`, `seccomp_requested`, `mount_point`) and the
   `pub(crate)` ones (`MANIFEST_ENV`, `manifest_path`, `missing_manifest`,
   `display_path`, `read_manifest`, `ManifestReadError`) are re-exported from
   `config/mod.rs` at their old paths.

Deliberate, documented difference (fails closed, never on a valid document):
`capabilities` is now type-checked in *both* privilege modes. The old reader only
looked at it in `capabilities` mode, so a root-mode document with a malformed
list was ignored; it is now refused with `capabilities: …`. Stage 0 always writes
an array, so no document either stage can produce changes behaviour. Pinned by
`config::tests::a_capability_list_that_is_not_strings_is_refused_in_either_mode`.

## Review round (fix-up)

Four findings from review, all fixed:

1. **Regression (major): an absent `privilege` was refused.** `WireConfig` and
   `PrivilegeParts` now carry `#[serde(default)]` on `privilege`, so all three
   pre-D6 members (`privilege`, `capabilities`, `bounding_set`) may be absent —
   `Privilege::from_json` read them with `get`, and an absent key always meant
   "this boot detected nothing". Pinned by
   `config::tests::a_document_without_the_privilege_members_reads_as_nothing_detected`
   (all three absent → `privilege: None`, document equal to the base-shaped
   `Config`; and a mode without the list/fact → rootless, no capabilities,
   `bounding_set_dropped: true`), and the contract test's “older document” block
   now strips the D6 trio as well and asserts equality with the base-shaped
   `Config`.
2. **`proxy_max_connections` absent sentence restored.** The member is read as an
   `Option` and `into_config` refuses `None` with
   `proxy_max_connections: must be an object` — the sentence `.get()` produced —
   instead of serde's “the field is missing”.
3. **No `serde` type names in user-facing text.** `php` is read through
   `PhpMember`, whose wrong-shape sentence is `php: must be an object or null`,
   and `Privilege::from_json` returns `Ok(None)` for a value that is not an
   object (what `get` did) instead of describing `PrivilegeParts`. Pinned by
   `config::tests::a_wrong_shaped_member_keeps_its_own_sentence`, which also
   asserts neither `PhpWire` nor `invalid type` appears.
4. **The fail-closed `capabilities` change is pinned** in both modes (above).

## Verification

```
$ cargo fmt --all -- --check            # exit 0
$ cargo test -p blastit --locked --lib config
test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 208 filtered out
$ cargo test -p blastit --locked
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   (integration)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    (doctests)
  ... all suites ok; exit 0
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.00s     # exit 0, no warnings
$ make fast-check
    ... test result: ok (every crate and suite) ...
All checks passed!
uvx --offline ruff format --check python tools
67 files already formatted
uv run --no-project tools/plan.py validate
plan OK
  exit 0
```

Re-run after the review fixes — the same commands, all exit 0:

```
$ cargo fmt --all -- --check            # exit 0
$ cargo test -p blastit --locked --lib config
test result: ok. 49 passed; 0 failed; 0 ignored; 0 measured; 208 filtered out
$ cargo clippy -p blastit --all-targets --locked -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.48s
$ make fast-check                       # exit 0; 148 suites reported "test result: ok"
uvx --offline ruff check python tools
All checks passed!
uvx --offline ruff format --check python tools
67 files already formatted
uv run --no-project tools/plan.py validate
plan OK
```

`grep` for the 18 accessor names across `crates/blastit/src/` returns nothing.

`git status --porcelain`:

```
 D crates/blastit/src/config.rs
?? crates/blastit/src/config/
```
(the plan/ churn in the same worktree belongs to the orchestrator, not this
node).

## Follow-ups

- None for this node. `config/tests.rs` (1,557 lines) is the moved unit-test
  module and is not production code; if the file-size bar is ever applied to
  tests too, it splits by theme (parsers / document / presets / environment)
  without touching a single assertion.
