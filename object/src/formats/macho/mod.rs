// SPDX-License-Identifier: MPL-2.0
//! Bounded little-endian MH_OBJECT output; this is not a Mach-O image linker.
mod headers;

mod sections;

mod symbols;

pub(crate) mod relocations;

use crate::{ObjectError, ObjectErrorKind, ObjectFile, ObjectFormat, ObjectResource};

use std::collections::HashSet;

fn invalid(message: impl Into<String>) -> ObjectError {
    ObjectError::new("write", ObjectErrorKind::InvalidEncoding, message)
}

fn grow(cursor: &mut usize, bytes: usize, limit: usize) -> Result<usize, ObjectError> {
    let start = *cursor;
    *cursor = cursor
        .checked_add(bytes)
        .filter(|n| *n <= limit)
        .ok_or_else(|| ObjectError::limit("write", ObjectResource::OutputBytes))?;
    Ok(start)
}

pub fn write(obj: &ObjectFile, max_output_size: u64) -> Result<Vec<u8>, ObjectError> {
    obj.validate_diagnostic()?;
    if obj.target.format != ObjectFormat::MachO64 {
        return Err(invalid("Mach-O writer requires MachO64 target"));
    }
    if obj.sections.len() > 255 {
        return Err(invalid("Mach-O section ordinal exceeds 255"));
    }
    let limit = usize::try_from(max_output_size.min(u32::MAX.into())).unwrap_or(usize::MAX);
    let mut cursor = 0;
    grow(&mut cursor, 232 + obj.sections.len() * 80, limit)?;
    if obj.symbols.len().saturating_add(obj.relocations.len()) > limit / 16 {
        return Err(ObjectError::limit("write", ObjectResource::OutputBytes));
    }

    let mut memory = 0;
    let mut layouts = Vec::new();
    let mut names = HashSet::new();
    for section in &obj.sections {
        let layout = sections::plan(section, &mut memory, &mut cursor, limit)?;
        if !names.insert((layout.segment, layout.name.clone())) {
            return Err(invalid("duplicate mapped Mach-O section name"));
        }
        layouts.push(layout);
    }
    let data_end = cursor;
    let data =
        layouts.iter().filter_map(|l| (l.data != 0).then_some(l.data)).min().unwrap_or(data_end);
    memory = memory.max((data_end - data) as u64);
    let mut relocs = vec![Vec::new(); obj.sections.len()];
    let mut occupied = Vec::new();
    for r in &obj.relocations {
        let (_, width) = relocations::encoding(r.kind, obj.target.machine).map_err(invalid)?;
        if r.offset > i32::MAX as usize {
            return Err(invalid("Mach-O relocation address exceeds signed32"));
        }
        occupied.push((r.section_index, r.offset, r.offset + width));
        relocs[r.section_index].push(r);
    }
    occupied.sort_unstable();
    for pair in occupied.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].2 > pair[1].1 {
            return Err(invalid("overlapping Mach-O relocation fields"));
        }
    }
    let pad = (4 - cursor % 4) % 4;
    grow(&mut cursor, pad, limit)?;
    for (layout, records) in layouts.iter_mut().zip(&mut relocs) {
        records.sort_by_key(|r| std::cmp::Reverse(r.offset));
        layout.count = records.len();
        layout.relocs =
            if records.is_empty() { 0 } else { grow(&mut cursor, records.len() * 8, limit)? };
    }
    let symbols = symbols::plan(obj, limit)?;
    if symbols.entries.len() > 0x1000000 {
        return Err(invalid("Mach-O symbol index exceeds 24 bits"));
    }
    let at = grow(&mut cursor, symbols.entries.len() * 16, limit)?;
    let strings = grow(&mut cursor, symbols.strings.len(), limit)?;
    let mut out = Vec::new();
    out.try_reserve_exact(cursor).map_err(|_| {
        ObjectError::new(
            "write",
            ObjectErrorKind::HostAllocation,
            "Mach-O output allocation failed",
        )
    })?;
    out.resize(cursor, 0);
    headers::file(
        &mut out,
        headers::FileLayout {
            machine: obj.target.machine,
            sections: obj.sections.len(),
            data,
            data_end,
            memory,
            symbols: at,
            count: symbols.entries.len() as u32,
            strings,
            string_size: symbols.strings.len(),
            groups: symbols.groups,
        },
    );
    for (i, (section, layout)) in obj.sections.iter().zip(&layouts).enumerate() {
        sections::emit(&mut out, i, layout, section);
        for (j, r) in relocs[i].iter().enumerate() {
            let (code, pc, width) = relocations::patch(
                &mut out[layout.data..layout.data + section.data.len()],
                r,
                obj.target.machine,
            )
            .map_err(invalid)?;
            let at = layout.relocs + j * 8;
            headers::u32_at(&mut out, at, r.offset as u32);
            headers::u32_at(
                &mut out,
                at + 4,
                symbols.indices[r.symbol.as_str()]
                    | ((pc as u32) << 24)
                    | ((if width == 8 { 3 } else { 2 }) << 25)
                    | (1 << 27)
                    | (code << 28),
            );
        }
    }
    symbols::emit(&mut out, at, &symbols, &layouts)?;
    out[strings..].copy_from_slice(&symbols.strings);
    Ok(out)
}
