//! A bounds-checked, panic-free reader for the parts of an ELF object blastit
//! inspects statically.
//!
//! Two places in blastit read an ELF object **without loading it**: the boot's
//! `DT_RPATH`/`DT_RUNPATH` provenance scan (`blastit::host`) and the
//! `entrypoint php` discovery gate's `.dynsym` walk
//! (`blastit_view_php::discovery`). Both need the same three things — the
//! header, the section header table and the bytes of one section — and both are
//! handed a file a deployment may not control, so both need the same guarantee:
//! every offset and length is checked against the file's own length before a
//! byte is read, and a truncated or hostile object is an [`ElfError`], never a
//! panic.
//!
//! This module is that layer. It knows an ELF class, a section header and the
//! little-endian widths; it does **not** know what a section *means*:
//! `DT_RPATH` is the provenance scan's vocabulary and a Zend symbol the PHP
//! gate's, so each keeps its own thin walk on top of [`Elf`].
//!
//! # Rules this module implements
//!
//! * **Little-endian, both classes.** Every object blastit embeds is
//!   little-endian, so a big-endian one is refused ([`ElfError::BigEndian`])
//!   rather than misread; ELF32 and ELF64 differ only in field width.
//! * **No `unsafe`, no `mmap`, no `dlopen`.** [`Elf`] wraps a `Read + Seek` the
//!   caller already opened read-only and reads at most one section at a time.
//! * **No panics on file bytes.** Extractions go through [`slice::get`] with
//!   `checked_add`/`checked_mul`; a short read or an oversized length is an `Err`.
//! * **The caller owns the bound.** A section is read only through
//!   [`Elf::section_bytes`], which takes the limit the caller's own walk is
//!   willing to allocate: this layer never decides on its behalf.
//!
//! ```
//! use std::io::Cursor;
//!
//! use blastit_sys::elf::Elf;
//!
//! let text = Cursor::new(b"not an ELF object".to_vec());
//! assert!(Elf::parse(text).is_err(), "a text file is refused, not panicked on");
//! ```

use std::io::{self, Read, Seek, SeekFrom};

/// The four bytes every ELF object starts with.
const MAGIC: &[u8; 4] = b"\x7fELF";

/// The size of `e_ident`: the part of the header both classes spell identically.
const IDENT_LEN: usize = 16;

/// `EI_CLASS`: a 32-bit object.
const ELFCLASS32: u8 = 1;

/// `EI_CLASS`: a 64-bit object.
const ELFCLASS64: u8 = 2;

/// `EI_DATA`: little-endian. The only byte order this build reads.
const ELFDATA2LSB: u8 = 1;

/// `SHT_STRTAB`: a string table.
pub const SHT_STRTAB: u32 = 3;

/// `SHT_DYNAMIC`: the section holding an object's dynamic array.
pub const SHT_DYNAMIC: u32 = 6;

/// `SHT_DYNSYM`: the dynamic symbol table.
pub const SHT_DYNSYM: u32 = 11;

/// `STT_FUNC`: a symbol whose value is code.
pub const STT_FUNC: u8 = 2;

/// `SHN_UNDEF`: the symbol is imported, not defined by this object.
pub const SHN_UNDEF: u16 = 0;

/// The largest section any walk in blastit reads into memory.
///
/// A `libphp`/`libpython` `.dynsym` is tens of kilobytes and a `SHT_DYNAMIC`
/// array a few hundred bytes; this is the bound that keeps a hostile file from
/// being an allocation.
pub const MAX_SECTION_BYTES: u64 = 64 * 1024 * 1024;

/// `SHF_WRITE`: the section is writable — never where a compile-time string is.
pub const SHF_WRITE: u64 = 0x1;

/// `SHF_ALLOC`: the section is mapped into the process image.
pub const SHF_ALLOC: u64 = 0x2;

/// Which width of ELF header and section header a walk is reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    /// 32-bit (`e_ident[EI_CLASS] == 1`).
    Elf32,
    /// 64-bit (`e_ident[EI_CLASS] == 2`).
    Elf64,
}

impl Class {
    /// Whether this is the 64-bit class, which is the only thing most field
    /// tables turn on.
    #[must_use]
    pub const fn is_64(self) -> bool {
        matches!(self, Self::Elf64)
    }

    /// The size of the ELF header.
    #[must_use]
    pub const fn header_size(self) -> usize {
        match self {
            Self::Elf32 => 52,
            Self::Elf64 => 64,
        }
    }

    /// The smallest `e_shentsize` that is a real section header.
    #[must_use]
    pub const fn section_header_size(self) -> usize {
        match self {
            Self::Elf32 => 40,
            Self::Elf64 => 64,
        }
    }

    /// The size of one `Elf*_Sym` (`sh_entsize` of a `.dynsym`).
    #[must_use]
    pub const fn symbol_entry_size(self) -> usize {
        match self {
            Self::Elf32 => 16,
            Self::Elf64 => 24,
        }
    }

    /// The offset of `st_shndx` in one `Elf*_Sym`.
    const fn symbol_shndx_offset(self) -> usize {
        match self {
            Self::Elf32 => 14,
            Self::Elf64 => 6,
        }
    }
}

/// Why an object could not be read.
///
/// Every case is an object this build refuses to inspect, so every case is an
/// `Err` at the caller's boundary; none of them is ever a panic and none of them
/// is ever read as "no such section".
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ElfError {
    /// The first four bytes are not `\x7fELF`, or there are fewer than four.
    #[error("no ELF magic")]
    NoMagic,

    /// The magic is there, but `e_ident` is cut short.
    #[error("it is shorter than an ELF identification")]
    IdentTooShort,

    /// `e_ident[EI_CLASS]` is neither 1 (32-bit) nor 2 (64-bit). The byte stays
    /// in the error for a caller that wants it; the sentence both callers print
    /// leaves it out, because a number a broken file carried is not actionable.
    #[error("not an ELF file: unknown class")]
    UnknownClass(u8),

    /// `e_ident[EI_DATA]` says big-endian.
    #[error("a big-endian ELF object: only little-endian objects are embedded")]
    BigEndian,

    /// The file ends before the header does.
    #[error("the header is truncated")]
    HeaderTruncated,

    /// `e_shnum` is zero, so there is no section header table.
    #[error("it has no section headers")]
    NoSections,

    /// `e_shentsize` is smaller than a section header.
    #[error("a section header table whose entries are too small")]
    SectionEntriesTooSmall,

    /// `e_shentsize * e_shnum` does not fit a `u64`.
    #[error("the section header table is too large")]
    SectionTableTooLarge,

    /// `e_shoff + e_shentsize * e_shnum` is past the end of the file.
    #[error("the section header table is past the end of the file")]
    SectionTablePastEnd,

    /// A section index is past the table.
    #[error("section {index} is past the section table")]
    SectionIndexOutOfRange {
        /// The index that was asked for.
        index: usize,
    },

    /// The bytes read at a section header do not hold every field.
    #[error("the section header is truncated")]
    SectionHeaderTruncated,

    /// A section's `sh_size` is larger than the caller's own limit.
    #[error("a {size}-byte section is larger than this build will inspect")]
    SectionTooLarge {
        /// The size the section header states.
        size: u64,
    },

    /// `sh_offset + sh_size` overflowed or ran past the end of the file.
    #[error("a section runs past the end of the file")]
    SectionPastEnd,

    /// A section's length does not fit this build's `usize`.
    #[error("a section is larger than this build can address")]
    SectionTooLargeToAddress,

    /// A `.dynsym` entry is smaller than this class's `Elf*_Sym`.
    #[error("a .dynsym entry is too small to be a symbol")]
    SymbolEntriesTooSmall,

    /// The object has no `.dynsym` section.
    #[error("it has no .dynsym section")]
    NoDynsym,

    /// The object has no `SHT_DYNAMIC` section.
    #[error("it has no dynamic section")]
    NoDynamic,

    /// `.dynsym` or `SHT_DYNAMIC` does not link a string table.
    #[error("its dynamic section does not link a string table")]
    NoDynamicStringTable,

    /// A `dyn_tag` value does not name a string this build can read.
    #[error("a dynamic string entry is not a string this build can read")]
    DynamicString,

    /// `seek`/`read` on the file the caller opened failed.
    #[error("the file could not be read ({0})")]
    Io(#[from] io::Error),
}

/// The fields of one section header (`sh_*`) both walks read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionHeader {
    /// `sh_name`: the offset in the section-name table (`e_shstrndx`).
    pub name: u32,
    /// `sh_type`: one of the `SHT_*` values.
    pub kind: u32,
    /// `sh_flags`: the `SHF_*` bits.
    pub flags: u64,
    /// `sh_addr`: the address the section is mapped at, or zero.
    pub addr: u64,
    /// `sh_offset`: the section's first byte in the file.
    pub offset: u64,
    /// `sh_size`: its length in bytes.
    pub size: u64,
    /// `sh_link`: the section-table index the section refers to.
    pub link: u32,
    /// `sh_info`: the section's type-dependent extra information.
    pub info: u32,
    /// `sh_addralign`: its alignment.
    pub addralign: u64,
    /// `sh_entsize`: the size of one entry, for a table.
    pub entsize: u64,
}

/// An ELF object that is open behind `reader`, with its header checked.
///
/// [`Elf::parse`] is the only constructor, and it refuses everything a walk
/// below it could not trust: a short identification, an unknown class, the
/// wrong byte order, a truncated header, and a section header table that does
/// not fit the file. Everything after that — a section header, a section's
/// bytes, a section looked up by name — is one bounds-checked read, so the
/// callers keep their own domain walks instead of re-deriving offsets.
#[derive(Debug)]
pub struct Elf<F> {
    reader: F,
    class: Class,
    machine: u16,
    shoff: u64,
    shentsize: usize,
    shnum: usize,
    shstrndx: usize,
    file_len: u64,
}

impl<F: Read + Seek> Elf<F> {
    /// Checks the header of the object `reader` is open on.
    ///
    /// # Errors
    ///
    /// [`ElfError`] for a file that is not a little-endian ELF with a section
    /// header table inside it, and for any `seek`/`read` failure. Under four
    /// bytes is [`ElfError::NoMagic`]; `\x7fELF` under sixteen is `IdentTooShort`.
    pub fn parse(mut reader: F) -> Result<Self, ElfError> {
        let file_len = reader.seek(SeekFrom::End(0)).map_err(ElfError::Io)?;
        reader.seek(SeekFrom::Start(0)).map_err(ElfError::Io)?;
        let mut ident = Vec::with_capacity(IDENT_LEN);
        reader
            .by_ref()
            .take(IDENT_LEN as u64)
            .read_to_end(&mut ident)
            .map_err(ElfError::Io)?;
        if ident.get(..MAGIC.len()) != Some(MAGIC.as_slice()) {
            return Err(ElfError::NoMagic);
        }
        if ident.len() < IDENT_LEN {
            return Err(ElfError::IdentTooShort);
        }
        let class = match ident.get(4).copied() {
            Some(ELFCLASS32) => Class::Elf32,
            Some(ELFCLASS64) => Class::Elf64,
            Some(other) => return Err(ElfError::UnknownClass(other)),
            None => return Err(ElfError::IdentTooShort),
        };
        if ident.get(5).copied() != Some(ELFDATA2LSB) {
            return Err(ElfError::BigEndian);
        }
        let header_len = class.header_size();
        if file_len < header_len as u64 {
            return Err(ElfError::HeaderTruncated);
        }
        let mut head = vec![0_u8; header_len];
        reader.seek(SeekFrom::Start(0)).map_err(ElfError::Io)?;
        reader.read_exact(&mut head).map_err(ElfError::Io)?;
        let machine = le_u16(&head, 0x12).ok_or(ElfError::HeaderTruncated)?;
        // `e_shoff` is the only field that moves between the classes; the three
        // `u16` fields that follow it keep their order.
        let (shoff, shentsize_at, shnum_at, shstrndx_at) = match class {
            Class::Elf64 => (le_u64(&head, 0x28), 0x3a, 0x3c, 0x3e),
            Class::Elf32 => (le_u32(&head, 0x20).map(u64::from), 0x2e, 0x30, 0x32),
        };
        let shoff = shoff.ok_or(ElfError::HeaderTruncated)?;
        let shentsize = usize::from(le_u16(&head, shentsize_at).ok_or(ElfError::HeaderTruncated)?);
        let shnum = usize::from(le_u16(&head, shnum_at).ok_or(ElfError::HeaderTruncated)?);
        let shstrndx = usize::from(le_u16(&head, shstrndx_at).ok_or(ElfError::HeaderTruncated)?);
        if shnum == 0 {
            return Err(ElfError::NoSections);
        }
        if shentsize < class.section_header_size() {
            return Err(ElfError::SectionEntriesTooSmall);
        }
        let table_len = u64::try_from(shentsize)
            .ok()
            .and_then(|entsize| entsize.checked_mul(u64::try_from(shnum).ok()?))
            .ok_or(ElfError::SectionTableTooLarge)?;
        let table_end = shoff
            .checked_add(table_len)
            .ok_or(ElfError::SectionTableTooLarge)?;
        if table_end > file_len {
            return Err(ElfError::SectionTablePastEnd);
        }
        Ok(Self {
            reader,
            class,
            machine,
            shoff,
            shentsize,
            shnum,
            shstrndx,
            file_len,
        })
    }

    /// The width of this object's header and section headers.
    #[must_use]
    pub fn class(&self) -> Class {
        self.class
    }

    /// `e_machine`: which instruction set the object was built for.
    #[must_use]
    pub fn machine(&self) -> u16 {
        self.machine
    }

    /// The file's length in bytes, as [`Elf::parse`] saw it.
    #[must_use]
    pub fn file_len(&self) -> u64 {
        self.file_len
    }

    /// How many section headers the table holds.
    #[must_use]
    pub fn section_count(&self) -> usize {
        self.shnum
    }

    /// The section header at `index`.
    ///
    /// # Errors
    ///
    /// [`ElfError::SectionIndexOutOfRange`] for an index at or past
    /// [`Elf::section_count`], and [`ElfError::Io`] when the file cannot be
    /// read at the table offset.
    pub fn section(&mut self, index: usize) -> Result<SectionHeader, ElfError> {
        if index >= self.shnum {
            return Err(ElfError::SectionIndexOutOfRange { index });
        }
        let index = u64::try_from(index).map_err(|_| ElfError::SectionIndexOutOfRange { index })?;
        let entsize = u64::try_from(self.shentsize).map_err(|_| ElfError::SectionTableTooLarge)?;
        let at = index
            .checked_mul(entsize)
            .and_then(|at| self.shoff.checked_add(at))
            .ok_or(ElfError::SectionTableTooLarge)?;
        let bytes = self.read(at, self.shentsize)?;
        SectionHeader::parse(self.class, &bytes).ok_or(ElfError::SectionHeaderTruncated)
    }

    /// `len` bytes at `offset`, refused when the range is not inside the file.
    ///
    /// # Errors
    ///
    /// [`ElfError::SectionPastEnd`] when `offset + len` overflows or is past the
    /// file's end, [`ElfError::SectionTooLargeToAddress`] when `len` does not fit
    /// this build, and [`ElfError::Io`] when the read fails.
    pub fn read(&mut self, offset: u64, len: usize) -> Result<Vec<u8>, ElfError> {
        let length = u64::try_from(len).map_err(|_| ElfError::SectionTooLargeToAddress)?;
        let end = offset.checked_add(length).ok_or(ElfError::SectionPastEnd)?;
        if end > self.file_len {
            return Err(ElfError::SectionPastEnd);
        }
        self.reader
            .seek(SeekFrom::Start(offset))
            .map_err(ElfError::Io)?;
        let mut bytes = vec![0_u8; len];
        self.reader.read_exact(&mut bytes).map_err(ElfError::Io)?;
        Ok(bytes)
    }

    /// A section's bytes, refused when it is larger than `limit`.
    ///
    /// `limit` is the caller's own allocation bound: the two walks in blastit
    /// inspect a few hundred bytes of dynamic string table and tens of
    /// kilobytes of symbols, and this layer must not decide that a 4 GiB section
    /// is fine.
    ///
    /// # Errors
    ///
    /// [`ElfError::SectionTooLarge`] when `sh_size` is over `limit`,
    /// [`ElfError::SectionPastEnd`] when `sh_offset + sh_size` leaves the file,
    /// [`ElfError::SectionTooLargeToAddress`] when the size does not fit this
    /// build, and [`ElfError::Io`] when the read fails.
    pub fn section_bytes(
        &mut self,
        section: &SectionHeader,
        limit: u64,
    ) -> Result<Vec<u8>, ElfError> {
        if section.size > limit {
            return Err(ElfError::SectionTooLarge { size: section.size });
        }
        let len = usize::try_from(section.size).map_err(|_| ElfError::SectionTooLargeToAddress)?;
        self.read(section.offset, len)
    }

    /// The section whose name in the section-name table (`e_shstrndx`) is
    /// `name`, or `None` when this object names no section that way.
    ///
    /// `limit` bounds the name table's read, the same way
    /// [`Elf::section_bytes`] does.
    ///
    /// # Errors
    ///
    /// The failures [`Elf::section`] and [`Elf::section_bytes`] report for the
    /// name table itself. An object with no name table (`e_shstrndx == 0`), an
    /// `e_shstrndx` past the table, or a name table that is not `SHT_STRTAB` is
    /// `Ok(None)`: it names nothing.
    pub fn section_by_name(
        &mut self,
        name: &str,
        limit: u64,
    ) -> Result<Option<SectionHeader>, ElfError> {
        if self.shstrndx == 0 || self.shstrndx >= self.shnum {
            return Ok(None);
        }
        let names = self.section(self.shstrndx)?;
        if names.kind != SHT_STRTAB {
            return Ok(None);
        }
        let bytes = self.section_bytes(&names, limit)?;
        for index in 0..self.shnum {
            let section = self.section(index)?;
            let offset = usize::try_from(section.name).unwrap_or(usize::MAX);
            let Some(text) = c_str(&bytes, offset) else {
                continue;
            };
            if text == name.as_bytes() {
                return Ok(Some(section));
            }
        }
        Ok(None)
    }

    /// The first section of `kind`, or `None`.
    fn section_by_kind(&mut self, kind: u32) -> Result<Option<SectionHeader>, ElfError> {
        for index in 0..self.shnum {
            let section = self.section(index)?;
            if section.kind == kind {
                return Ok(Some(section));
            }
        }
        Ok(None)
    }

    /// Every dynamic entry whose `d_tag` is `tag`, rendered through `.dynstr`.
    ///
    /// This is the one owner of the `SHT_DYNAMIC` array: `DT_SONAME`,
    /// `DT_RPATH` and `DT_RUNPATH` are the same walk with a different number.
    /// `limit` is a byte budget applied to both sections.
    ///
    /// # Errors
    ///
    /// [`ElfError::NoDynamic`] for an object with no dynamic section,
    /// [`ElfError::NoDynamicStringTable`] when its `sh_link` is not a string
    /// table, [`ElfError::DynamicString`] when an entry's value is not a
    /// NUL-terminated string inside `.dynstr`, and the section-read failures.
    pub fn dynamic_strings(&mut self, tag: u64, limit: u64) -> Result<Vec<String>, ElfError> {
        let Some(dynamic) = self.section_by_kind(SHT_DYNAMIC)? else {
            return Err(ElfError::NoDynamic);
        };
        let strings = self.section(usize::try_from(dynamic.link).unwrap_or(usize::MAX))?;
        if strings.kind != SHT_STRTAB {
            return Err(ElfError::NoDynamicStringTable);
        }
        let names = self.section_bytes(&strings, limit)?;
        let entries = self.section_bytes(&dynamic, limit)?;
        let wide = self.class.is_64();
        let minimum = if wide { 16 } else { 8 };
        let entry = usize::try_from(dynamic.entsize).unwrap_or(0).max(minimum);
        let mut found = Vec::new();
        for chunk in entries.chunks_exact(entry) {
            let entry_tag = if wide {
                le_u64(chunk, 0)
            } else {
                le_u32(chunk, 0).map(u64::from)
            };
            // `DT_NULL` ends the array; everything after it is padding.
            let Some(entry_tag) = entry_tag else { break };
            if entry_tag == 0 {
                break;
            }
            if entry_tag != tag {
                continue;
            }
            let value = if wide {
                le_u64(chunk, 8)
            } else {
                le_u32(chunk, 4).map(u64::from)
            };
            let Some(value) = value else { break };
            let offset = usize::try_from(value).unwrap_or(usize::MAX);
            let Some(text) = c_str(&names, offset) else {
                return Err(ElfError::DynamicString);
            };
            let text = std::str::from_utf8(text).map_err(|_| ElfError::DynamicString)?;
            found.push(text.to_owned());
        }
        Ok(found)
    }

    /// Every dynamic entry whose `d_tag` is one of `tags`, in file order.
    ///
    /// The interleaved walk a provenance scan needs: `DT_RPATH` and
    /// `DT_RUNPATH` are read in the order the linker wrote them, not grouped by
    /// tag, so a refusal names the first entry the loader would read.
    ///
    /// # Errors
    ///
    /// The same failures [`Elf::dynamic_strings`] reports.
    pub fn dynamic_strings_any(
        &mut self,
        tags: &[u64],
        limit: u64,
    ) -> Result<Vec<String>, ElfError> {
        let Some(dynamic) = self.section_by_kind(SHT_DYNAMIC)? else {
            return Err(ElfError::NoDynamic);
        };
        let strings = self.section(usize::try_from(dynamic.link).unwrap_or(usize::MAX))?;
        if strings.kind != SHT_STRTAB {
            return Err(ElfError::NoDynamicStringTable);
        }
        let names = self.section_bytes(&strings, limit)?;
        let entries = self.section_bytes(&dynamic, limit)?;
        let wide = self.class.is_64();
        let minimum = if wide { 16 } else { 8 };
        let entry = usize::try_from(dynamic.entsize).unwrap_or(0).max(minimum);
        let mut found = Vec::new();
        for chunk in entries.chunks_exact(entry) {
            let entry_tag = if wide {
                le_u64(chunk, 0)
            } else {
                le_u32(chunk, 0).map(u64::from)
            };
            let Some(entry_tag) = entry_tag else { break };
            if entry_tag == 0 {
                break;
            }
            if !tags.contains(&entry_tag) {
                continue;
            }
            let value = if wide {
                le_u64(chunk, 8)
            } else {
                le_u32(chunk, 4).map(u64::from)
            };
            let Some(value) = value else { break };
            let offset = usize::try_from(value).unwrap_or(usize::MAX);
            let Some(text) = c_str(&names, offset) else {
                return Err(ElfError::DynamicString);
            };
            let text = std::str::from_utf8(text).map_err(|_| ElfError::DynamicString)?;
            found.push(text.to_owned());
        }
        Ok(found)
    }

    /// Every symbol in `.dynsym`, with `st_shndx` honoured: `defined` is false
    /// for `SHN_UNDEF`, so an object that merely **imports** a name is never
    /// reported as exporting it.
    ///
    /// `limit` is a byte budget applied to `.dynsym` and its linked `.dynstr`.
    ///
    /// # Errors
    ///
    /// [`ElfError::NoDynsym`], [`ElfError::SymbolEntriesTooSmall`] when
    /// `sh_entsize` is below the class's `Elf*_Sym` minimum (24/16) — refused
    /// rather than clamped, because a clamped stride reads garbage — and the
    /// section-read failures.
    pub fn dynsym_symbols(&mut self, limit: u64) -> Result<Vec<ElfSymbol>, ElfError> {
        let Some(dynsym) = self.section_by_kind(SHT_DYNSYM)? else {
            return Err(ElfError::NoDynsym);
        };
        let entry =
            usize::try_from(dynsym.entsize).map_err(|_| ElfError::SectionTooLargeToAddress)?;
        if entry < self.class.symbol_entry_size() {
            return Err(ElfError::SymbolEntriesTooSmall);
        }
        let strings = self.section(usize::try_from(dynsym.link).unwrap_or(usize::MAX))?;
        if strings.kind != SHT_STRTAB {
            return Err(ElfError::NoDynamicStringTable);
        }
        let names = self.section_bytes(&strings, limit)?;
        let table = self.section_bytes(&dynsym, limit)?;
        let shndx_at = self.class.symbol_shndx_offset();
        let mut symbols = Vec::new();
        for symbol in table.chunks_exact(entry) {
            let Some(offset) = le_u32(symbol, 0) else {
                continue;
            };
            let Some(name) = c_str(&names, usize::try_from(offset).unwrap_or(usize::MAX)) else {
                continue;
            };
            symbols.push(ElfSymbol {
                name: String::from_utf8_lossy(name).into_owned(),
                defined: le_u16(symbol, shndx_at).unwrap_or(SHN_UNDEF) != SHN_UNDEF,
                kind: symbol.get(4).copied().unwrap_or(0) & 0x0f,
                value: symbol_value(self.class, symbol),
                size: symbol_size(self.class, symbol),
            });
        }
        Ok(symbols)
    }

    /// [`Elf::dynsym_symbols`] without the body fields: the shape a gate that
    /// only asks "is this name exported here?" needs.
    ///
    /// # Errors
    ///
    /// Everything [`Elf::dynsym_symbols`] reports.
    pub fn dynsym_names(&mut self, limit: u64) -> Result<Vec<DynSymbol>, ElfError> {
        Ok(self
            .dynsym_symbols(limit)?
            .into_iter()
            .map(|symbol| DynSymbol {
                name: symbol.name,
                defined: symbol.defined,
            })
            .collect())
    }
}

/// One `.dynsym` entry, with the identity fact a gate needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DynSymbol {
    /// The symbol's name, from `.dynstr`.
    pub name: String,
    /// `st_shndx != SHN_UNDEF`: this object defines it rather than importing it.
    pub defined: bool,
}

/// One `.dynsym` entry, including the body fields a decoder reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElfSymbol {
    /// The symbol's name, from `.dynstr`.
    pub name: String,
    /// `st_shndx != SHN_UNDEF`.
    pub defined: bool,
    /// `st_info`'s type nibble (`STT_FUNC` is 2).
    pub kind: u8,
    /// `st_value`: the load address of the body.
    pub value: u64,
    /// `st_size`: the body's length in bytes.
    pub size: u64,
}

/// `st_value` of one `Elf*_Sym`.
fn symbol_value(class: Class, symbol: &[u8]) -> u64 {
    match class {
        Class::Elf64 => le_u64(symbol, 8).unwrap_or(0),
        Class::Elf32 => le_u32(symbol, 4).map(u64::from).unwrap_or(0),
    }
}

/// `st_size` of one `Elf*_Sym`.
fn symbol_size(class: Class, symbol: &[u8]) -> u64 {
    match class {
        Class::Elf64 => le_u64(symbol, 16).unwrap_or(0),
        Class::Elf32 => le_u32(symbol, 8).map(u64::from).unwrap_or(0),
    }
}

impl SectionHeader {
    /// Parses one section header of `class` from `bytes`.
    fn parse(class: Class, bytes: &[u8]) -> Option<Self> {
        match class {
            Class::Elf64 => Some(Self {
                name: le_u32(bytes, 0x00)?,
                kind: le_u32(bytes, 0x04)?,
                flags: le_u64(bytes, 0x08)?,
                addr: le_u64(bytes, 0x10)?,
                offset: le_u64(bytes, 0x18)?,
                size: le_u64(bytes, 0x20)?,
                link: le_u32(bytes, 0x28)?,
                info: le_u32(bytes, 0x2c)?,
                addralign: le_u64(bytes, 0x30)?,
                entsize: le_u64(bytes, 0x38)?,
            }),
            Class::Elf32 => Some(Self {
                name: le_u32(bytes, 0x00)?,
                kind: le_u32(bytes, 0x04)?,
                flags: u64::from(le_u32(bytes, 0x08)?),
                addr: u64::from(le_u32(bytes, 0x0c)?),
                offset: u64::from(le_u32(bytes, 0x10)?),
                size: u64::from(le_u32(bytes, 0x14)?),
                link: le_u32(bytes, 0x18)?,
                info: le_u32(bytes, 0x1c)?,
                addralign: u64::from(le_u32(bytes, 0x20)?),
                entsize: u64::from(le_u32(bytes, 0x24)?),
            }),
        }
    }
}

/// The NUL-terminated name at `offset`, or `None` when `offset` is past the
/// table.
///
/// Public because every caller that reads a `.dynstr`/`.shstrtab` (this crate's
/// walkers, the PHP gate's body decode, the boot's provenance scan) must spell
/// the same bounds check once.
#[must_use]
pub fn c_str(bytes: &[u8], offset: usize) -> Option<&[u8]> {
    let rest = bytes.get(offset..)?;
    let end = rest
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(rest.len());
    rest.get(..end)
}

/// A little-endian `u16` at `offset`, or `None` past the end.
#[must_use]
pub fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    let slice: [u8; 2] = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
    Some(u16::from_le_bytes(slice))
}

/// A little-endian `u32` at `offset`, or `None` past the end.
#[must_use]
pub fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    let slice: [u8; 4] = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(slice))
}

/// A little-endian `u64` at `offset`, or `None` past the end.
#[must_use]
pub fn le_u64(bytes: &[u8], offset: usize) -> Option<u64> {
    let slice: [u8; 8] = bytes.get(offset..offset.checked_add(8)?)?.try_into().ok()?;
    Some(u64::from_le_bytes(slice))
}

// The one surviving non-§7 suite in this crate: a malformed ELF object cannot be
// produced by any end-to-end run of the real binary (the binary either refuses to
// boot or never writes one), so these byte-level refusals are only reachable here.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::Cursor;

    use super::*;

    /// The largest section either walk would ask for, for the positive cases.
    const LIMIT: u64 = 64 * 1024 * 1024;

    /// A little-endian ELF64 header for a file of `len` bytes whose section
    /// table starts at `shoff` and holds `shnum` entries of `shentsize` bytes.
    fn elf64_file(shoff: u64, shentsize: u16, shnum: u16, len: usize) -> Vec<u8> {
        let mut file = vec![0_u8; 64.max(len)];
        file[..4].copy_from_slice(MAGIC);
        file[4] = ELFCLASS64;
        file[5] = ELFDATA2LSB;
        file[6] = 1;
        file[0x28..0x30].copy_from_slice(&shoff.to_le_bytes());
        file[0x3a..0x3c].copy_from_slice(&shentsize.to_le_bytes());
        file[0x3c..0x3e].copy_from_slice(&shnum.to_le_bytes());
        file.truncate(len);
        file
    }

    /// An ELF64 object with two sections: the null header, then one at `offset`
    /// holding `size` bytes, named by itself (`e_shstrndx = 1`).
    fn elf64_with_one_section(offset: u64, size: u64) -> Vec<u8> {
        let shoff = 128u64;
        let mut file = elf64_file(shoff, 64, 2, 256);
        file[0x3e..0x40].copy_from_slice(&1u16.to_le_bytes());
        let at = usize::try_from(shoff).unwrap() + 64;
        file[at + 4..at + 8].copy_from_slice(&SHT_STRTAB.to_le_bytes());
        file[at + 0x18..at + 0x20].copy_from_slice(&offset.to_le_bytes());
        file[at + 0x20..at + 0x28].copy_from_slice(&size.to_le_bytes());
        file
    }

    #[test]
    fn a_file_with_no_elf_magic_is_refused() {
        for (label, image) in [
            ("empty", Vec::new()),
            ("short magic", b"\x7fEL".to_vec()),
            ("text", b"this is not an ELF object".to_vec()),
        ] {
            let err = Elf::parse(Cursor::new(image)).expect_err(label);
            assert!(matches!(err, ElfError::NoMagic), "{label}: {err}");
        }
    }

    #[test]
    fn a_truncated_identification_is_refused() {
        // The magic and the class are there; `e_ident` is not.
        let err = Elf::parse(Cursor::new(elf64_file(0, 64, 1, 12))).expect_err("12-byte e_ident");
        assert!(matches!(err, ElfError::IdentTooShort), "{err}");
    }

    #[test]
    fn a_class_or_byte_order_this_build_cannot_read_is_refused() {
        let mut unknown = elf64_file(0, 64, 1, 128);
        unknown[4] = 3;
        let err = Elf::parse(Cursor::new(unknown)).expect_err("class 3");
        assert!(matches!(err, ElfError::UnknownClass(3)), "{err}");
        // The shared sentence names no number, so both callers print the same
        // clause without either of them stripping the byte.
        assert_eq!(err.to_string(), "not an ELF file: unknown class");

        let mut big = elf64_file(0, 64, 1, 128);
        big[5] = 2;
        let err = Elf::parse(Cursor::new(big)).expect_err("a big-endian object");
        assert!(matches!(err, ElfError::BigEndian), "{err}");
    }

    #[test]
    fn a_truncated_header_is_refused() {
        let err = Elf::parse(Cursor::new(elf64_file(0, 64, 1, 40))).expect_err("40 of 64 bytes");
        assert!(matches!(err, ElfError::HeaderTruncated), "{err}");
    }

    #[test]
    fn a_section_table_past_the_end_of_the_file_is_refused() {
        let err = Elf::parse(Cursor::new(elf64_file(4096, 64, 2, 128))).expect_err("e_shoff 4096");
        assert!(matches!(err, ElfError::SectionTablePastEnd), "{err}");

        // 60 000 entries of 64 bytes is a 3.7 MiB table in a 128-byte file, and
        // the widest `u16` fields must not wrap the multiplication either.
        let err =
            Elf::parse(Cursor::new(elf64_file(64, 64, 60_000, 128))).expect_err("60 000 entries");
        assert!(matches!(err, ElfError::SectionTablePastEnd), "{err}");
        let err = Elf::parse(Cursor::new(elf64_file(64, u16::MAX, u16::MAX, 256)))
            .expect_err("0xffff entries of 0xffff bytes");
        assert!(matches!(err, ElfError::SectionTablePastEnd), "{err}");
    }

    #[test]
    fn an_object_with_no_sections_or_tiny_entries_is_refused() {
        let err = Elf::parse(Cursor::new(elf64_file(64, 64, 0, 128))).expect_err("no sections");
        assert!(matches!(err, ElfError::NoSections), "{err}");

        let err = Elf::parse(Cursor::new(elf64_file(64, 32, 1, 128))).expect_err("32-byte entries");
        assert!(matches!(err, ElfError::SectionEntriesTooSmall), "{err}");
    }

    #[test]
    fn a_section_that_wraps_the_file_length_is_refused() {
        // `sh_offset = u64::MAX`, `sh_size = 16`: `offset + size` overflows.
        let mut elf = Elf::parse(Cursor::new(elf64_with_one_section(u64::MAX, 16)))
            .expect("the header is still well-formed");
        let section = elf.section(1).expect("the only section");
        let err = elf
            .section_bytes(&section, LIMIT)
            .expect_err("offset + size wraps");
        assert!(matches!(err, ElfError::SectionPastEnd), "{err}");
    }

    #[test]
    fn a_section_over_the_callers_limit_is_refused() {
        let mut elf =
            Elf::parse(Cursor::new(elf64_with_one_section(64, 16))).expect("a well-formed object");
        let section = elf.section(1).expect("the only section");
        let err = elf
            .section_bytes(&section, 4)
            .expect_err("a 16-byte section under a 4-byte limit");
        assert!(
            matches!(err, ElfError::SectionTooLarge { size } if size == 16),
            "{err}"
        );
    }

    #[test]
    fn a_section_index_past_the_table_is_refused() {
        let mut elf =
            Elf::parse(Cursor::new(elf64_with_one_section(64, 16))).expect("a well-formed object");
        let err = elf.section(2).expect_err("index 2 of 2");
        assert!(
            matches!(err, ElfError::SectionIndexOutOfRange { index: 2 }),
            "{err}"
        );
    }
}
