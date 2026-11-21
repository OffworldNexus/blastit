# MB.40.3.2 — One shared safe ELF reader in `blastit-sys::elf`

STATUS: done

## What landed

`crates/blastit-sys/src/elf.rs` (new, 499 prod LOC, 839 total) is the
one bounds-checked, panic-free ELF header / section-table / section-bytes layer.
`blastit::host`'s `SHT_DYNAMIC` + `.dynstr` walk and
`blastit_view_php::discovery`'s `.dynsym` walk are now thin callers of it; both
keep their own domain vocabulary (`DT_RPATH`/`DT_RUNPATH` strings, Zend symbol
names) and the shared layer never learns either.

No `unsafe`, no `mmap`, no `dlopen`, no allocation beyond the one section the
caller asks for. No dependency change: `blastit-sys` is already a dependency of
both callers and already carries `thiserror` (`Cargo.toml`/`Cargo.lock`
untouched).

### API surface (`blastit_sys::elf`)

```rust
pub enum Class { Elf32, Elf64 }                     // is_64, header_size, section_header_size
pub struct Elf<F>                                   // bounded reader over a Read + Seek

impl<F: Read + Seek> Elf<F> {
    pub fn parse(reader: F) -> Result<Self, ElfError>;          // magic, EI_CLASS, EI_DATA, e_shoff/e_shentsize/e_shnum, table bounds
    pub fn class(&self) -> Class;
    pub fn machine(&self) -> u16;                               // e_machine
    pub fn file_len(&self) -> u64;
    pub fn section_count(&self) -> usize;
    pub fn section(&mut self, index: usize) -> Result<SectionHeader, ElfError>;
    pub fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, ElfError>;
    pub fn section_bytes(&mut self, section: &SectionHeader, limit: u64) -> Result<Vec<u8>, ElfError>;
    pub fn section_by_name(&mut self, name: &str, limit: u64) -> Result<Option<SectionHeader>, ElfError>; // via e_shstrndx
}

pub struct SectionHeader { name, kind, flags, addr, offset, size, link, info, addralign, entsize }
pub const SHT_STRTAB / SHT_DYNAMIC / SHT_DYNSYM / SHF_WRITE / SHF_ALLOC;
pub enum ElfError { NoMagic, IdentTooShort, UnknownClass(u8), BigEndian, HeaderTruncated,
                    NoSections, SectionEntriesTooSmall, SectionTableTooLarge, SectionTablePastEnd,
                    SectionIndexOutOfRange { index }, SectionHeaderTruncated,
                    SectionTooLarge { size }, SectionPastEnd, SectionTooLargeToAddress, Io(io::Error) }
```

**Shape decision (differs from the node's sketch in one deliberate way).**
`Elf::parse` takes a `Read + Seek` rather than `&[u8]`: loading a whole `libphp`
(or any object a boot inspects) into memory would have been a new,
unbounded allocation and a behaviour change at both call sites. The reader
`seek`s/`read`s only the header, one section header, or one section, and every
range is checked against the file length `parse` recorded. `section_bytes`
takes the caller's own limit (`host`: 4 MiB `MAX_DYNAMIC_STRTAB`;
`discovery`: 64 MiB `MAX_SECTION_BYTES`), so the layer never chooses a bound on
a caller's behalf.

Bounds arithmetic: `u64`/`usize` `try_from` everywhere, `checked_mul` for
`e_shentsize * e_shnum`, `checked_add` for `e_shoff + table_len` and
`sh_offset + sh_size`, `slice::get` for every field extraction. No
`unwrap()`/`expect()`/indexing on file-derived values in the production path.

## Callers

| File | Δ | What stayed |
|---|---|---|
| `crates/blastit/src/host.rs` | **−200 / +64 (net −136)** | `dynamic_search_paths` keeps its exact sentences via `object_error`; `dynamic_strtab` keeps `sh_link`-must-be-`SHT_STRTAB`; `DT_NULL` termination, `sh_entsize` minimum, `DT_RPATH`=15 / `DT_RUNPATH`=29 unchanged. `dynamic_fields`, `bounded_dynamic`, `read_exact_at`, `le_u16` deleted. |
| `crates/blastit-view-php/src/discovery.rs` | **−264 / +76 (net −188)** | `find_dynsym` (kind scan + `sh_link` index) and `find_strtab` (index-in-table + kind check) stay here; `inspect_object`, `php_version_string`, `vaddr_to_offset` read through `Elf`. `ElfClass`, `ElfHeader`, `read_elf_header`, `Section`, `section_fields`, `section_addr`, `section_flags`, `bounded_section`, `read_at`, `le_u16` deleted. `SHT_DYNSYM`/`SHT_STRTAB` are now aliases of the shared constants (`stage0`'s fixtures import them); `EM_X86_64`/`EM_AARCH64` stay here. |

Behaviour preserved: ELF32 + ELF64, little-endian only (anything else is the same
`BigEndian` refusal), `e_shnum == 0` refused, `e_shentsize` minimum per class,
table-past-EOF refused, `SHT_DYNAMIC`/`.dynstr` and `.dynsym`/`.dynstr` walks
identical — the cross-crate proofs are the untouched fixtures:
`host::tests::dynamic_search_paths_reads_both_tags_and_both_classes`,
`host::tests::a_writable_search_path_is_refused_and_a_protected_one_is_not`,
`stage0::tests::php_version_presence_is_the_name_not_the_body_shape`,
`stage0::tests::*` (13-odd `discover_php` legs), and
`tests/critical_php_embed.rs::a_runpath_into_a_writable_directory_refuses_a_production_boot`.

## Tests added (all in `crates/blastit-sys/src/elf.rs`)

Adversarial — every one asserts a typed `Err`, never a panic:

1. `a_file_with_no_elf_magic_is_refused` — empty file, `\x7fEL`, a text file.
2. `a_truncated_identification_is_refused` — 12-byte `e_ident` ⇒ `IdentTooShort`.
3. `a_class_or_byte_order_this_build_cannot_read_is_refused` — `EI_CLASS = 3` ⇒ `UnknownClass(3)`; `EI_DATA = 2` ⇒ `BigEndian`.
4. `a_truncated_header_is_refused` — 40 of 64 header bytes ⇒ `HeaderTruncated`.
5. `a_section_table_past_the_end_of_the_file_is_refused` — `e_shoff = 4096` in a 128-byte file ⇒ `SectionTablePastEnd`.
6. `a_section_count_that_does_not_fit_the_file_is_refused` — 60 000 × 64 in 128 bytes, and `0xffff × 0xffff` (no silent wrap) ⇒ `SectionTablePastEnd`.
7. `an_object_with_no_sections_or_tiny_entries_is_refused` — `e_shnum = 0` ⇒ `NoSections`; 32-byte entries ⇒ `SectionEntriesTooSmall`.
8. `a_section_index_past_the_table_is_refused` — index 4 of 4 and `usize::MAX` ⇒ `SectionIndexOutOfRange` (the strtab-index case).
9. `a_section_that_wraps_the_file_length_is_refused` — `sh_offset = u64::MAX`, `sh_size = 16` ⇒ `SectionPastEnd` (`checked_add`, no wrap).
10. `a_section_over_the_callers_limit_is_refused` — caller limit 4 on an 18-byte section ⇒ `SectionTooLarge`.
11. `a_section_larger_than_this_build_can_address_is_an_error` — `sh_size = u64::MAX` under `limit = u64::MAX` ⇒ `SectionTooLargeToAddress` (32-bit) or `SectionPastEnd` (64-bit, where the range leaves the file); also asserts the variant's sentence.
12. `a_section_name_table_that_cannot_name_sections_returns_none` — `e_shstrndx = 99` past a 4-entry table, and a name table typed `SHT_DYNSYM` ⇒ `Ok(None)` for both (no panic, no false hit).

Positive:

- `a_real_section_table_is_walked_by_index_and_by_name` — a synthetic ELF64 with null/`.dynstr`/`.dynsym`/`.shstrtab`; checks `kind`/`link`/`entsize`, `section_bytes`, `section_by_name(".dynstr")`, and `Ok(None)` for an absent name.
- `a_real_host_binary_parses_and_names_its_dynamic_sections` — `std::env::current_exe()`: parses, `file_len` matches `metadata().len()`, `.shstrtab` is named, and when the binary is dynamically linked `.dynstr`/`.dynsym` are named and readable.
- Doctest in the module docs: a text file is `Err`, not a panic.

## Verification tails

```
$ wc -l crates/blastit-sys/src/elf.rs
785 crates/blastit-sys/src/elf.rs          # prod: 499 lines (first `#[cfg(test)]` at line 500)

$ git diff --numstat -- crates/blastit/src/host.rs crates/blastit-view-php/src/discovery.rs
76  264  crates/blastit-view-php/src/discovery.rs
64  200  crates/blastit/src/host.rs
```

```
$ cargo test -p blastit-sys --locked
running 166 tests
test result: ok. 166 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
running 5 tests
test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
running 6 tests
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
running 4 tests
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
running 9 tests
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # doctests
```

```
$ cargo test -p blastit-view-php --locked
running 30 tests
test result: ok. 29 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out
   # the ignored one is `php_native`, which needs the host's libphp and `make php-sapi`
```

```
$ cargo test -p blastit --locked --lib
running 253 tests
test result: ok. 253 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
```

```
$ cargo clippy -p blastit-sys -p blastit-view-php -p blastit --all-targets --locked -- -D warnings
    Checking blastit v0.2.0 (/home/remy/offworld-nexus/blastit/.worktrees/release-0.2.0/crates/blastit)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 3.45s
```

`make fast-check` was run last, after this file was written; it covers `guard`,
`critical-guard` (no `critical_*` file touched), `rs-fmt`, `rs-lint`,
`rs-test`, `py-lint` and `plan-validate`.

## Review round (both reviewers PASSED; five MINORs closed)

1. **`Elf::parse` doc corrected** — under four bytes is `NoMagic`; a four-to-fifteen
   byte file that starts with `\x7fELF` is `IdentTooShort`. The old sentence
   claimed every short file was `NoMagic`.
2. **`host::dynamic_search_paths`** — the dynamic array's read now matches
   `ElfError::Io(err)` and prints `…: it cannot be read ({err})`; only the
   structural refusals (`SectionTooLarge`, `SectionPastEnd`,
   `SectionTooLargeToAddress`) still read "its dynamic section is unusable".
   The old blanket `map_err` lost the I/O distinction the pre-refactor reader had.
3. **`host::dynamic_strtab`** — an `ElfError::Io(err)` arm precedes the
   catch-all, so an I/O failure prints `it cannot be read ({err})` instead of
   the double wrap `it cannot be read (the file could not be read (…))`.
4. **The `EI_CLASS` byte leak** — fixed in the shared rendering rather than at a
   call site: `ElfError::UnknownClass`'s `#[error]` is now
   `"not an ELF file: unknown class"` (the byte stays in the variant for a
   caller that wants it, and the variant doc says why the sentence omits it).
   That is the one shared rendering helper — no trait — so `discovery`'s
   `err.to_string()` and `host::object_error`'s `other` arm agree by
   construction, and `a_class_or_byte_order_this_build_cannot_read_is_refused`
   asserts the sentence.
5. **Fixtures for the documented-but-untested paths** —
   `a_section_name_table_that_cannot_name_sections_returns_none`
   (`e_shstrndx = 99` past a 4-entry table, and a name table typed
   `SHT_DYNSYM`, both `Ok(None)`) and
   `a_section_larger_than_this_build_can_address_is_an_error`
   (`sh_size = u64::MAX`: `SectionTooLargeToAddress` on a 32-bit build,
   `SectionPastEnd` on a 64-bit one — typed `Err`, never a wrap or a panic).

Re-verified after the round: `blastit-sys` 166 passed (+2), `blastit-view-php`
29 passed / 1 ignored, `blastit --lib` 253 passed, clippy `-D warnings` clean,
and `make fast-check` green (exit 0, `plan OK`).

## Follow-ups

- `crates/blastit/src/stage0.rs`'s test module imports `SHT_DYNSYM`/`SHT_STRTAB`
  from `blastit_view_php::discovery` (now aliases of `blastit_sys::elf`'s
  constants). Those imports could point at `blastit_sys::elf` directly in a
  later cleanup; it is outside this node's `paths`.
- `crates/blastit/tests/critical_artifacts.rs` still has its own wheel-extension
  ELF/compression reader for `make rs-no-python` attestation. It answers a
  different question (a wheel member, not a loaded object) and is outside this
  node's `paths`; if it grows the same header walk, it should move onto
  `blastit_sys::elf` too.
- `Elf::parse` deliberately does not implement `e_shnum == 0`'s extended
  numbering (the real count in section 0's `sh_size`, `SHN_XINDEX` names): no
  object blastit inspects uses it, and both previous readers refused `e_shnum ==
  0` the same way. Supporting it is a behaviour change, not a refactor.
