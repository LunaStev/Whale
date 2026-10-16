// SPDX-License-Identifier: MPL-2.0
//! Checked, borrowed ELF layout primitives and the supported ET_REL importer.
use crate::{
    ObjectError as Error, ObjectErrorKind as Kind, ObjectFile, ObjectRelocation,
    ObjectResource as Resource, ObjectSymbol, RelocKind, Section, SectionKind, SymbolBinding,
    SymbolKind, SymbolVisibility, Target,
};

use std::collections::HashSet;

type Result<T> = std::result::Result<T, Error>;

fn error(kind: Kind, message: impl Into<String>) -> Error {
    Error::new("read", kind, message)
}

#[derive(Clone, Copy, Debug)]
pub struct ObjectReadLimits {
    pub max_input_bytes: usize,
    pub max_sections: usize,
    pub max_program_headers: usize,
    pub max_symbols: usize,
    pub max_relocations: usize,
    /// Sum of copied section payloads; aliases cannot multiply input storage.
    pub max_payload_bytes: usize,
    /// Cumulative names inspected and copied, including repeated references.
    pub max_string_bytes: usize,
    pub max_work: usize,
}

impl Default for ObjectReadLimits {
    fn default() -> Self {
        Self {
            max_input_bytes: 256 * 1024 * 1024,
            max_sections: 65_279,
            max_program_headers: 4096,
            max_symbols: 1_000_000,
            max_relocations: 1_000_000,
            max_payload_bytes: 256 * 1024 * 1024,
            max_string_bytes: 16 * 1024 * 1024,
            max_work: 512 * 1024 * 1024,
        }
    }
}

/// Actual successful reader charges, including raw auxiliary table records and
/// repeated name inspections/copies. Callers can subtract these from shared limits.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectReadUsage {
    pub input_bytes: usize,
    pub sections: usize,
    pub program_headers: usize,
    pub symbols: usize,
    pub relocations: usize,
    pub payload_bytes: usize,
    pub string_bytes: usize,
    pub work: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ElfHeader {
    pub kind: u16,
    pub entry: u64,
    pub program_offset: u64,
    pub section_offset: u64,
    pub program_entry_size: u16,
    pub program_count: usize,
    pub section_entry_size: u16,
    pub section_count: usize,
    pub string_section: usize,
}

#[derive(Clone, Copy, Debug)]
pub struct ElfSectionHeader {
    pub name: u32,
    pub kind: u32,
    pub flags: u64,
    pub address: u64,
    pub offset: u64,
    pub size: u64,
    pub link: u32,
    pub info: u32,
    pub align: u64,
    pub entry_size: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct ElfProgramHeader {
    pub kind: u32,
    pub flags: u32,
    pub offset: u64,
    pub virtual_address: u64,
    pub physical_address: u64,
    pub file_size: u64,
    pub memory_size: u64,
    pub align: u64,
}

/// Header/table access never casts host structs or indexes unchecked bytes.
/// Container-specific semantics (ET_REL or ET_EXEC) are validated by consumers.
#[derive(Clone, Copy, Debug)]
pub struct ElfView<'a> {
    bytes: &'a [u8],
    pub header: ElfHeader,
}

impl<'a> ElfView<'a> {
    pub fn parse(bytes: &'a [u8], limits: ObjectReadLimits) -> Result<Self> {
        if bytes.len() > limits.max_input_bytes {
            return Err(Error::limit("read", Resource::InputBytes));
        }
        if bytes.len() < 64 {
            return Err(
                error(Kind::Truncated, "ELF64 header requires 64 bytes").at(bytes.len() as u64)
            );
        }
        if &bytes[..4] != b"\x7fELF" {
            return Err(error(Kind::InvalidEncoding, "invalid ELF magic").at(0));
        }
        if bytes[4] != 2 || bytes[5] != 1 || bytes[6] != 1 {
            return Err(error(
                Kind::Unsupported,
                "only ELF64 little-endian version 1 is supported",
            )
            .at(4));
        }
        if !matches!(bytes[7], 0 | 3) || bytes[8] != 0 || bytes[9..16].iter().any(|b| *b != 0) {
            return Err(
                error(Kind::Unsupported, "unsupported ELF ABI or identification padding").at(7)
            );
        }
        let mut view = Self {
            bytes,
            header: ElfHeader {
                kind: 0,
                entry: 0,
                program_offset: 0,
                section_offset: 0,
                program_entry_size: 0,
                program_count: 0,
                section_entry_size: 0,
                section_count: 0,
                string_section: 0,
            },
        };
        if view.u16(18)? != 62 {
            return Err(error(Kind::InvalidTarget, "only AMD64 ELF machine 62 is supported").at(18));
        }
        if view.u32(20)? != 1 || view.u32(48)? != 0 || view.u16(52)? != 64 {
            return Err(
                error(Kind::Unsupported, "unsupported ELF version, flags or header size").at(20)
            );
        }
        view.header = ElfHeader {
            kind: view.u16(16)?,
            entry: view.u64(24)?,
            program_offset: view.u64(32)?,
            section_offset: view.u64(40)?,
            program_entry_size: view.u16(54)?,
            program_count: usize::from(view.u16(56)?),
            section_entry_size: view.u16(58)?,
            section_count: usize::from(view.u16(60)?),
            string_section: usize::from(view.u16(62)?),
        };
        let h = view.header;
        if h.section_count >= 0xff00
            || h.program_count == 0xffff
            || h.string_section == 0xffff
            || (h.section_count == 0 && h.section_offset != 0)
        {
            return Err(error(Kind::Unsupported, "ELF extended numbering is unsupported").at(56));
        }
        if h.section_count > limits.max_sections {
            return Err(Error::limit("read", Resource::Sections));
        }
        if h.program_count > limits.max_program_headers {
            return Err(Error::limit("read", Resource::ProgramHeaders));
        }
        if h.section_count > 0 {
            if h.section_entry_size != 64 || h.string_section >= h.section_count {
                return Err(error(
                    Kind::InvalidSection,
                    "invalid section table stride or string index",
                )
                .at(58));
            }
            view.table(h.section_offset, h.section_count, 64)?;
        } else if h.string_section != 0 {
            return Err(
                error(Kind::InvalidSection, "absent section table has a string index").at(62)
            );
        }
        if h.program_count > 0 {
            if h.program_entry_size != 56 {
                return Err(error(Kind::InvalidEncoding, "invalid program header stride").at(54));
            }
            view.table(h.program_offset, h.program_count, 56)?;
        }
        Ok(view)
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn range(&self, offset: u64, size: u64) -> Result<&'a [u8]> {
        let end = offset
            .checked_add(size)
            .ok_or_else(|| error(Kind::Overflow, "ELF range overflow").at(offset))?;
        let start = usize::try_from(offset)
            .map_err(|_| error(Kind::OutOfBounds, "ELF offset exceeds host range").at(offset))?;
        let end = usize::try_from(end)
            .map_err(|_| error(Kind::OutOfBounds, "ELF range exceeds host range").at(offset))?;
        self.bytes
            .get(start..end)
            .ok_or_else(|| error(Kind::Truncated, "ELF range exceeds input").at(offset))
    }

    fn table(&self, offset: u64, count: usize, stride: u64) -> Result<()> {
        let size = (count as u64)
            .checked_mul(stride)
            .ok_or_else(|| error(Kind::Overflow, "ELF table size overflow").at(offset))?;
        self.range(offset, size).map(|_| ())
    }

    pub fn u16(&self, at: u64) -> Result<u16> {
        Ok(u16::from_le_bytes(self.range(at, 2)?.try_into().unwrap()))
    }

    pub fn u32(&self, at: u64) -> Result<u32> {
        Ok(u32::from_le_bytes(self.range(at, 4)?.try_into().unwrap()))
    }

    pub fn u64(&self, at: u64) -> Result<u64> {
        Ok(u64::from_le_bytes(self.range(at, 8)?.try_into().unwrap()))
    }

    pub fn i64(&self, at: u64) -> Result<i64> {
        Ok(i64::from_le_bytes(self.range(at, 8)?.try_into().unwrap()))
    }

    pub fn section(&self, index: usize) -> Result<ElfSectionHeader> {
        if index >= self.header.section_count {
            return Err(error(Kind::InvalidSection, "section index outside table").section(index));
        }
        let at = self
            .header
            .section_offset
            .checked_add(
                (index as u64)
                    .checked_mul(64)
                    .ok_or_else(|| error(Kind::Overflow, "section table index overflow"))?,
            )
            .ok_or_else(|| error(Kind::Overflow, "section table offset overflow"))?;
        Ok(ElfSectionHeader {
            name: self.u32(at)?,
            kind: self.u32(at + 4)?,
            flags: self.u64(at + 8)?,
            address: self.u64(at + 16)?,
            offset: self.u64(at + 24)?,
            size: self.u64(at + 32)?,
            link: self.u32(at + 40)?,
            info: self.u32(at + 44)?,
            align: self.u64(at + 48)?,
            entry_size: self.u64(at + 56)?,
        })
    }

    pub fn program(&self, index: usize) -> Result<ElfProgramHeader> {
        if index >= self.header.program_count {
            return Err(error(Kind::OutOfBounds, "program index outside table"));
        }
        let at = self
            .header
            .program_offset
            .checked_add(
                (index as u64)
                    .checked_mul(56)
                    .ok_or_else(|| error(Kind::Overflow, "program table index overflow"))?,
            )
            .ok_or_else(|| error(Kind::Overflow, "program table offset overflow"))?;
        Ok(ElfProgramHeader {
            kind: self.u32(at)?,
            flags: self.u32(at + 4)?,
            offset: self.u64(at + 8)?,
            virtual_address: self.u64(at + 16)?,
            physical_address: self.u64(at + 24)?,
            file_size: self.u64(at + 32)?,
            memory_size: self.u64(at + 40)?,
            align: self.u64(at + 48)?,
        })
    }
}

struct Budget {
    limits: ObjectReadLimits,
    work: usize,
    strings: usize,
    payload: usize,
}

impl Budget {
    fn charge(&mut self, n: usize) -> Result<()> {
        self.work = self.work.checked_add(n).ok_or_else(|| Error::limit("read", Resource::Work))?;
        if self.work > self.limits.max_work {
            return Err(Error::limit("read", Resource::Work));
        }
        Ok(())
    }

    fn strings(&mut self, n: usize) -> Result<()> {
        self.strings = self
            .strings
            .checked_add(n)
            .ok_or_else(|| Error::limit("read", Resource::StringBytes))?;
        if self.strings > self.limits.max_string_bytes {
            return Err(Error::limit("read", Resource::StringBytes));
        }
        Ok(())
    }

    fn own(&mut self, s: &str) -> Result<String> {
        self.strings(s.len())?;
        self.charge(s.len())?;
        let mut result = String::new();
        result
            .try_reserve_exact(s.len())
            .map_err(|_| error(Kind::HostAllocation, "name allocation failed"))?;
        result.push_str(s);
        Ok(result)
    }

    fn name<'a>(
        &mut self,
        view: ElfView<'a>,
        table: ElfSectionHeader,
        index: u32,
    ) -> Result<&'a str> {
        if table.kind != 3 || u64::from(index) >= table.size {
            return Err(
                error(Kind::InvalidName, "invalid string table or name offset").at(table.offset)
            );
        }
        let start = table
            .offset
            .checked_add(u64::from(index))
            .ok_or_else(|| error(Kind::Overflow, "string offset overflow"))?;
        let tail = view.range(start, table.size - u64::from(index))?;
        let remaining = self.limits.max_work.saturating_sub(self.work);
        let search = &tail[..tail.len().min(remaining.saturating_add(1))];
        let Some(end) = search.iter().position(|b| *b == 0) else {
            if tail.len() > remaining {
                return Err(Error::limit("read", Resource::Work));
            }
            return Err(error(Kind::InvalidName, "unterminated ELF name").at(start));
        };
        self.charge(end + 1)?;
        self.strings(end)?;
        std::str::from_utf8(&tail[..end])
            .map_err(|_| error(Kind::InvalidName, "ELF name is not UTF-8").at(start))
    }
}

fn reserve<T>(v: &mut Vec<T>, n: usize) -> Result<()> {
    v.try_reserve_exact(n)
        .map_err(|_| error(Kind::HostAllocation, "ELF metadata allocation failed"))
}

#[derive(Clone, Copy)]
struct RawSymbol<'a> {
    name: &'a str,
    kind: u8,
    binding: u8,
    visibility: u8,
    section: usize,
    value: u64,
    size: u64,
}

pub fn read_elf(bytes: &[u8], limits: ObjectReadLimits) -> Result<ObjectFile> {
    read_elf_with_usage(bytes, limits).map(|(object, _)| object)
}

pub fn read_elf_with_usage(
    bytes: &[u8],
    limits: ObjectReadLimits,
) -> Result<(ObjectFile, ObjectReadUsage)> {
    let view = ElfView::parse(bytes, limits)?;
    let h = view.header;
    if h.kind != 1 {
        return Err(error(Kind::Unsupported, "ObjectFile::read requires ELF ET_REL").at(16));
    }
    if h.entry != 0 || h.program_count != 0 || h.program_offset != 0 {
        return Err(error(
            Kind::InvalidEncoding,
            "ET_REL cannot carry an entry or program headers",
        ));
    }
    if h.section_count == 0 || h.string_section == 0 {
        return Err(error(Kind::InvalidSection, "ET_REL requires a section-name table"));
    }
    let mut budget = Budget { limits, work: 0, strings: 0, payload: 0 };
    let mut headers = Vec::new();
    reserve(&mut headers, h.section_count)?;
    for index in 0..h.section_count {
        budget.charge(1)?;
        let section = view.section(index)?;
        if index == 0 {
            if view.range(h.section_offset, 64)?.iter().any(|b| *b != 0) {
                return Err(error(Kind::InvalidSection, "section zero is not null").section(0));
            }
        } else {
            if section.align != 0 && !section.align.is_power_of_two() {
                return Err(error(
                    Kind::InvalidAlignment,
                    "section alignment must be zero or power of two",
                )
                .section(index));
            }
            if section.address != 0 {
                return Err(error(Kind::InvalidSection, "ET_REL section has a virtual address")
                    .section(index));
            }
            // NOBITS sh_offset is conceptual and can exceed EOF (alignment
            // alone may place it there); there is no file range to access.
            if section.kind != 8 {
                view.range(section.offset, section.size)?;
            }
        }
        headers.push(section);
    }
    let names = headers[h.string_section];
    if names.kind != 3 || view.range(names.offset, names.size)?.first() != Some(&0) {
        return Err(
            error(Kind::InvalidName, "invalid section-name string table").section(h.string_section)
        );
    }
    let mut object = ObjectFile::with_target(Target::X86_64WhaleLinux.object_target());
    let mut mapping = Vec::new();
    reserve(&mut mapping, h.section_count)?;
    mapping.resize(h.section_count, None);
    let mut symbol_table = None;
    for (index, &section) in headers.iter().enumerate().skip(1) {
        let name = budget.name(view, names, section.name).map_err(|e| e.section(index))?;
        match section.kind {
            1 | 8 => {
                let kind = match (section.kind, section.flags) {
                    (1, 6) => SectionKind::Text,
                    (1, 3) => SectionKind::Data,
                    (1, 2) => SectionKind::ReadOnlyData,
                    (8, 3) => SectionKind::Bss,
                    _ => {
                        return Err(error(Kind::Unsupported, "unsupported ELF section flags")
                            .section(index))
                    },
                };
                if section.link != 0 || section.info != 0 || section.entry_size != 0 {
                    return Err(error(Kind::Unsupported, "unsupported payload section metadata")
                        .section(index));
                }
                let mut data = Vec::new();
                if section.kind != 8 {
                    let n = usize::try_from(section.size)
                        .map_err(|_| Error::limit("read", Resource::PayloadBytes))?;
                    budget.payload = budget
                        .payload
                        .checked_add(n)
                        .ok_or_else(|| Error::limit("read", Resource::PayloadBytes))?;
                    if budget.payload > limits.max_payload_bytes {
                        return Err(Error::limit("read", Resource::PayloadBytes));
                    }
                    budget.charge(n)?;
                    reserve(&mut data, n)?;
                    data.extend_from_slice(view.range(section.offset, section.size)?);
                }
                mapping[index] = Some(object.sections.len());
                object.sections.push(Section {
                    name: budget.own(name)?,
                    kind,
                    data,
                    zero_fill: if section.kind == 8 { section.size } else { 0 },
                    align: section.align,
                });
            },
            2 => {
                if symbol_table.replace(index).is_some() {
                    return Err(error(
                        Kind::Unsupported,
                        "multiple ELF symbol tables are unsupported",
                    )
                    .section(index));
                }
            },
            3 => {
                if section.flags != 0
                    || view.range(section.offset, section.size)?.first() != Some(&0)
                {
                    return Err(error(Kind::InvalidName, "invalid ELF string table").section(index));
                }
            },
            4 => {
                if section.flags & !0x40 != 0 {
                    return Err(
                        error(Kind::Unsupported, "unsupported RELA section flags").section(index)
                    );
                }
            },
            _ => {
                return Err(error(
                    Kind::Unsupported,
                    format!("unsupported ELF section type {}", section.kind),
                )
                .section(index))
            },
        }
    }
    let sym_index =
        symbol_table.ok_or_else(|| error(Kind::InvalidSymbol, "ET_REL requires a symbol table"))?;
    let symtab = headers[sym_index];
    if symtab.flags != 0
        || symtab.entry_size != 24
        || symtab.size % 24 != 0
        || symtab.link as usize >= headers.len()
    {
        return Err(
            error(Kind::InvalidSymbol, "invalid ELF symbol table metadata").section(sym_index)
        );
    }
    let strings = headers[symtab.link as usize];
    if strings.kind != 3 {
        return Err(error(Kind::InvalidSymbol, "symbol table link is not a string table")
            .section(sym_index));
    }
    let count =
        usize::try_from(symtab.size / 24).map_err(|_| Error::limit("read", Resource::Symbols))?;
    if count == 0 || count > limits.max_symbols {
        return Err(Error::limit("read", Resource::Symbols));
    }
    if symtab.info == 0 || symtab.info as usize > count {
        return Err(
            error(Kind::InvalidSymbol, "invalid first-global symbol index").section(sym_index)
        );
    }
    let mut raw = Vec::new();
    reserve(&mut raw, count)?;
    let mut used_names = HashSet::new();
    used_names
        .try_reserve(count)
        .map_err(|_| error(Kind::HostAllocation, "symbol name allocation failed"))?;
    for index in 0..count {
        budget.charge(1)?;
        let at = symtab.offset + index as u64 * 24;
        let name = budget.name(view, strings, view.u32(at)?)?;
        let info = view.range(at + 4, 2)?;
        let symbol = RawSymbol {
            name,
            kind: info[0] & 15,
            binding: info[0] >> 4,
            visibility: info[1],
            section: usize::from(view.u16(at + 6)?),
            value: view.u64(at + 8)?,
            size: view.u64(at + 16)?,
        };
        if index == 0 {
            if view.range(at, 24)?.iter().any(|b| *b != 0) {
                return Err(error(Kind::InvalidSymbol, "symbol zero is not null").at(at));
            }
        } else if (index < symtab.info as usize) != (symbol.binding == 0) {
            return Err(error(
                Kind::InvalidSymbol,
                "local/global symbol ordering contradicts sh_info",
            )
            .at(at));
        }
        if !matches!(symbol.kind, 3 | 4) {
            used_names.insert(name);
        }
        raw.push(symbol);
    }
    let mut symbol_mapping = Vec::new();
    reserve(&mut symbol_mapping, count)?;
    symbol_mapping.resize(count, None);
    let mut helper_names = HashSet::new();
    for (index, &symbol) in raw.iter().enumerate().skip(1) {
        let binding = match symbol.binding {
            0 => SymbolBinding::Local,
            1 => SymbolBinding::Global,
            2 => SymbolBinding::Weak,
            _ => {
                return Err(
                    error(Kind::Unsupported, "unsupported ELF symbol binding").symbol(symbol.name)
                )
            },
        };
        let visibility = match symbol.visibility {
            0 => SymbolVisibility::Default,
            2 => SymbolVisibility::Hidden,
            _ => {
                return Err(error(Kind::Unsupported, "unsupported ELF symbol visibility")
                    .symbol(symbol.name))
            },
        };
        if symbol.kind == 4 {
            if binding != SymbolBinding::Local
                || symbol.section != 0xfff1
                || symbol.value != 0
                || symbol.size != 0
            {
                return Err(error(Kind::InvalidSymbol, "invalid STT_FILE auxiliary symbol")
                    .symbol(symbol.name));
            }
            continue; // file provenance is metadata, never a link definition
        }
        let (kind, name, section) = if symbol.kind == 3 {
            if binding != SymbolBinding::Local
                || visibility != SymbolVisibility::Default
                || symbol.value != 0
                || symbol.size != 0
            {
                return Err(error(Kind::InvalidSymbol, "invalid STT_SECTION auxiliary symbol")
                    .symbol(symbol.name));
            }
            let section = mapping.get(symbol.section).copied().flatten().ok_or_else(|| {
                error(Kind::InvalidSection, "section symbol references unsupported section")
                    .symbol(symbol.name)
            })?;
            let mut name = format!("__whale_elf_section_{index}");
            while used_names.contains(name.as_str()) || helper_names.contains(&name) {
                budget.charge(name.len() + 1)?;
                name.push('$');
            }
            budget.strings(
                name.len()
                    .checked_mul(2)
                    .ok_or_else(|| Error::limit("read", Resource::StringBytes))?,
            )?;
            helper_names.insert(name.clone());
            (SymbolKind::NoType, name, Some(section))
        } else {
            let kind = match symbol.kind {
                0 => SymbolKind::NoType,
                1 => SymbolKind::Object,
                2 => SymbolKind::Function,
                _ => {
                    return Err(
                        error(Kind::Unsupported, "unsupported ELF symbol type").symbol(symbol.name)
                    )
                },
            };
            let section = if symbol.section == 0 {
                None
            } else {
                Some(mapping.get(symbol.section).copied().flatten().ok_or_else(|| {
                    error(Kind::Unsupported, "unsupported absolute/common/section symbol index")
                        .symbol(symbol.name)
                })?)
            };
            (kind, budget.own(symbol.name)?, section)
        };
        symbol_mapping[index] = Some(object.symbols.len());
        object.symbols.push(ObjectSymbol {
            kind,
            name,
            section_index: section,
            value: symbol.value,
            size: symbol.size,
            binding,
            visibility,
        });
    }
    let mut relocation_count = 0usize;
    for (index, &section) in headers.iter().enumerate().filter(|(_, s)| s.kind == 4) {
        if section.entry_size != 24 || section.size % 24 != 0 || section.link as usize != sym_index
        {
            return Err(error(Kind::InvalidRelocation, "invalid RELA stride or symbol-table link")
                .section(index));
        }
        let target = mapping.get(section.info as usize).copied().flatten().ok_or_else(|| {
            error(Kind::InvalidRelocation, "RELA target is not a supported payload section")
                .section(index)
        })?;
        let count = usize::try_from(section.size / 24)
            .map_err(|_| Error::limit("read", Resource::Relocations))?;
        relocation_count = relocation_count
            .checked_add(count)
            .ok_or_else(|| Error::limit("read", Resource::Relocations))?;
        if relocation_count > limits.max_relocations {
            return Err(Error::limit("read", Resource::Relocations));
        }
        reserve(&mut object.relocations, count)?;
        for n in 0..count {
            budget.charge(1)?;
            let at = section.offset + n as u64 * 24;
            let info = view.u64(at + 8)?;
            let symbol =
                symbol_mapping.get((info >> 32) as usize).copied().flatten().ok_or_else(|| {
                    error(
                        Kind::InvalidRelocation,
                        "RELA references absent/null/file auxiliary symbol",
                    )
                    .at(at + 8)
                    .section(index)
                })?;
            let kind = match info as u32 {
                1 => RelocKind::Absolute64,
                10 => RelocKind::Absolute32,
                11 => RelocKind::SignedAbsolute32,
                2 => RelocKind::Relative32,
                15 => RelocKind::Relative8,
                9 => RelocKind::GOTPCREL,
                4 => RelocKind::PLT32,
                _ => {
                    return Err(error(
                        Kind::Unsupported,
                        format!("unsupported AMD64 RELA type {}", info as u32),
                    )
                    .at(at + 8)
                    .section(index))
                },
            };
            let offset = usize::try_from(view.u64(at)?).map_err(|_| {
                error(Kind::OutOfBounds, "relocation offset exceeds host range").at(at)
            })?;
            let name = budget.own(&object.symbols[symbol].name)?;
            object.relocations.push(ObjectRelocation {
                section_index: target,
                offset,
                symbol: name,
                addend: view.i64(at + 16)?,
                kind,
            });
        }
    }
    object.validate_diagnostic()?;
    let usage = ObjectReadUsage {
        input_bytes: bytes.len(),
        sections: h.section_count,
        program_headers: h.program_count,
        symbols: count,
        relocations: relocation_count,
        payload_bytes: budget.payload,
        string_bytes: budget.strings,
        work: budget.work,
    };
    Ok((object, usage))
}
