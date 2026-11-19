---
id: MB.40.3.2
title: "One shared safe ELF reader in blastit-sys::elf for host.rs and view-php discovery.rs"
kind: task
status: done
owner: implementer
priority: 2
depends_on: [MB.40.3.1]
paths:
  - "crates/blastit-sys/src/elf.rs"
  - "crates/blastit-sys/src/lib.rs"
  - crates/blastit/src/host.rs
  - "crates/blastit-view-php/src/discovery.rs"
  - Cargo.toml
  - Cargo.lock
attempts: 1
max_attempts: 3
escalation: none
created: "2026-11-19T01:45"
updated: "2026-11-19T02:19"
---

# Goal
Replace the two hand-rolled ELF readers with one shared, bounds-checked, panic-free ELF header/section-table layer in `crates/blastit-sys/src/elf.rs`, keeping each crate's domain walk (`SHT_DYNAMIC`/`.dynstr` vs `.dynsym`) as a thin caller.

# Exact Targets (`file:line` pre-diagnosed — quoted from MB.40.3/RESULT.md §Follow-ups 3)
- `crates/blastit/src/host.rs:882-980` — reads the ELF header, section table, `shstrtab` and `SHT_DYNAMIC`/`.dynstr` to derive capabilities and `RUNPATH`; must keep producing identical results from the shared reader.
- `crates/blastit-view-php/src/discovery.rs:1153-1700` — reads `.dynsym` for `libphp` symbol names and `php_version`; must keep its symbol fixtures green.
- RESULT.md: "The shared part is the header/section-table/bounds-checked-read layer (~200 LOC) — a new `blastit-sys/src/elf.rs` with each crate keeping its own domain walk, verified against both suites' capability and symbol fixtures."
- New module shape: `Elf::parse(&[u8]) -> Result<Elf, ElfError>`, 64/32-bit + little-endian header validation, program/section header iteration by index with `checked_mul`/`checked_add` arithmetic, named-section lookup via `shstrndx`, and a section-bytes accessor that returns `Err` for any `sh_offset + sh_size` past the buffer. `blastit-sys` is `unsafe`-confined already; this module needs **no** `unsafe` (byte slicing + `from_le_bytes`) and no allocation beyond the input.
- No `unwrap`/`expect`/panic on file bytes anywhere in the parsing path; `ElfError` is a typed enum with `Display`.
- Tests (new, in `crates/blastit-sys/src/elf.rs`): a real host binary (e.g. `std::env::current_exe()`) parses and finds `.dynstr`/`.dynsym`; adversarial fixtures — empty file, short magic, `EI_CLASS`/`EI_DATA` mismatch, truncated header, `e_shoff` past EOF, `e_shnum` overflow, `sh_offset + sh_size` past EOF, `e_shstrndx` out of range — each returns `Err`, never panics. Keep the existing capability/RUNPATH and libphp symbol fixtures untouched as the cross-crate regression proof.

# Acceptance
- `make fast-check`
- `cargo test -p blastit-sys --locked`
- `cargo test -p blastit-view-php --locked`
- `cargo test -p blastit --locked --lib`
- `make rs-no-default` (`discovery.rs` compiles in the feature-off matrix too)
- Evidence in RESULT.md: `wc -l crates/blastit-sys/src/elf.rs`, both old readers' LOC delta, and the exact `cargo test` tails.

# Notes
Single Cargo writer: serial with the sibling slices. Do not change attestation/extracted values — this is a pure refactor whose proof is that both suites' existing fixtures pass unchanged.
