// SPDX-License-Identifier: MPL-2.0
use super::headers::{u16_at, u32_at};

use super::{grow, invalid, number, relocations, symbols::Strings};

use crate::{Machine, ObjectError, ObjectRelocation, Section, SectionKind};

use std::collections::HashMap;

pub(super) struct Layout {
    pub name: [u8; 8],
    pub symbol_name: [u8; 8],
    pub data: usize,
    pub relocation: usize,
    pub size: u32,
    flags: u32,
}

pub(super) fn plan(
    section: &Section,
    count: usize,
    strings: &mut Strings,
    cursor: &mut usize,
    limit: usize,
) -> Result<Layout, ObjectError> {
    if count >= u16::MAX as usize {
        return Err(invalid("COFF relocation overflow records are unsupported"));
    }
    let data = if section.kind == SectionKind::Bss || section.data.is_empty() {
        0
    } else {
        // The specification recommends four-byte alignment for object payloads.
        grow(cursor, (4 - *cursor % 4) % 4, limit)?;
        grow(cursor, section.data.len(), limit)?
    };
    Ok(Layout {
        name: strings.name(&section.name, true)?,
        symbol_name: strings.name(&section.name, false)?,
        data,
        relocation: if count == 0 { 0 } else { grow(cursor, count * 10, limit)? },
        size: u32::try_from(section.memory_size().map_err(invalid)?)
            .map_err(|_| invalid("COFF section size exceeds 32 bits"))?,
        flags: characteristics(section).map_err(invalid)?,
    })
}

pub(super) fn emit(
    out: &mut [u8],
    index: usize,
    layout: &Layout,
    section: &Section,
) -> Result<(), ObjectError> {
    let h = 20 + index * 40;
    out[h..h + 8].copy_from_slice(&layout.name);
    u32_at(out, h + 16, layout.size);
    u32_at(out, h + 20, number(layout.data)?);
    u32_at(out, h + 24, number(layout.relocation)?);
    u32_at(out, h + 36, layout.flags);
    if layout.data != 0 {
        out[layout.data..layout.data + section.data.len()].copy_from_slice(&section.data);
    }
    Ok(())
}

pub(super) fn emit_relocations(
    out: &mut [u8],
    index: usize,
    layout: &Layout,
    machine: Machine,
    records: &[&ObjectRelocation],
    indices: &HashMap<&str, u32>,
) -> Result<(), ObjectError> {
    u16_at(out, 20 + index * 40 + 32, records.len() as u16);
    for (n, r) in records.iter().enumerate() {
        let (code, width) = relocations::encoding(r.kind, machine).map_err(invalid)?;
        relocations::apply(
            &mut out[layout.data + r.offset..layout.data + r.offset + width],
            r.kind,
            r.addend,
        )
        .map_err(invalid)?;
        let at = layout.relocation + n * 10;
        u32_at(out, at, number(r.offset)?);
        u32_at(out, at + 4, indices[r.symbol.as_str()]);
        u16_at(out, at + 8, code as u16);
    }
    Ok(())
}

pub(super) fn characteristics(section: &Section) -> Result<u32, String> {
    let align = section.align.max(1);
    if align > 8192 || !align.is_power_of_two() {
        return Err("COFF section alignment must be 1..8192 power of two".into());
    }
    let flags = match section.kind {
        SectionKind::Text => 0x60000020,
        SectionKind::Data => 0xc0000040,
        SectionKind::ReadOnlyData => 0x40000040,
        SectionKind::Bss => 0xc0000080,
    };
    Ok(flags | (align.trailing_zeros() + 1) << 20)
}
