// SPDX-License-Identifier: MPL-2.0
use super::{
    grow,
    headers::{u32_at, u64_at},
    invalid,
};

use crate::{ObjectError, Section, SectionKind};

pub(super) struct Layout {
    pub name: String,
    pub segment: &'static str,
    pub address: u64,
    pub size: u64,
    pub data: usize,
    pub relocs: usize,
    pub count: usize,
    pub alignment: u32,
    pub flags: u32,
}

pub(super) fn plan(
    section: &Section,
    memory: &mut u64,
    cursor: &mut usize,
    limit: usize,
) -> Result<Layout, ObjectError> {
    if section.name.len() > 16 {
        return Err(invalid("Mach-O section names must fit 16 non-NUL bytes"));
    }
    let align = section.align.max(1);
    if !align.is_power_of_two() || align > 1u64 << 31 {
        return Err(invalid("Mach-O section alignment exceeds 2^31"));
    }
    let name = match section.name.as_str() {
        ".text" => "__text".into(),
        ".data" => "__data".into(),
        ".rodata" => "__const".into(),
        ".bss" => "__bss".into(),
        s => {
            if s.starts_with("__") {
                s.into()
            } else {
                format!("__{}", s.strip_prefix('.').unwrap_or(s))
            }
        },
    };
    if name.len() > 16 || name.is_empty() || name.contains('\0') {
        return Err(invalid("Mach-O section names must fit 16 non-NUL bytes"));
    }
    let segment = if matches!(section.kind, SectionKind::Text | SectionKind::ReadOnlyData) {
        "__TEXT"
    } else {
        "__DATA"
    };
    *memory = memory
        .checked_add(align - 1)
        .map(|v| v & !(align - 1))
        .ok_or_else(|| invalid("Mach-O virtual alignment overflow"))?;
    let address = *memory;
    let size = section.memory_size().map_err(invalid)?;
    *memory = memory.checked_add(size).ok_or_else(|| invalid("Mach-O section extent overflow"))?;
    let data = if section.kind == SectionKind::Bss || section.data.is_empty() {
        0
    } else {
        let padding = ((align - (*cursor as u64 & (align - 1))) & (align - 1)) as usize;
        grow(cursor, padding, limit)?;
        grow(cursor, section.data.len(), limit)?
    };
    let flags = match section.kind {
        SectionKind::Text => 0x80000400,
        SectionKind::Bss => 1,
        _ => 0,
    };
    Ok(Layout {
        name,
        segment,
        address,
        size,
        data,
        relocs: 0,
        count: 0,
        alignment: align.trailing_zeros(),
        flags,
    })
}

pub(super) fn emit(out: &mut [u8], ordinal: usize, layout: &Layout, section: &Section) {
    let at = 104 + ordinal * 80;
    out[at..at + layout.name.len()].copy_from_slice(layout.name.as_bytes());
    out[at + 16..at + 16 + layout.segment.len()].copy_from_slice(layout.segment.as_bytes());
    u64_at(out, at + 32, layout.address);
    u64_at(out, at + 40, layout.size);
    u32_at(out, at + 48, layout.data as u32);
    u32_at(out, at + 52, layout.alignment);
    u32_at(out, at + 56, layout.relocs as u32);
    u32_at(out, at + 60, layout.count as u32);
    u32_at(out, at + 64, layout.flags);
    if layout.data != 0 {
        out[layout.data..layout.data + section.data.len()].copy_from_slice(&section.data);
    }
}
