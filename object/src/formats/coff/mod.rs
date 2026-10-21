// SPDX-License-Identifier: MPL-2.0
//! Bounded standard COFF objects, without PE images, BigObj or COMDAT semantics.
mod headers;

pub(crate) mod relocations;

mod symbols;

mod sections;

use crate::{
    ObjectError, ObjectErrorKind, ObjectFile, ObjectResource, ObjectSymbol, SymbolBinding,
    SymbolKind, SymbolVisibility,
};

use std::collections::HashMap;

fn invalid(message: impl Into<String>) -> ObjectError {
    ObjectError::new("write", ObjectErrorKind::InvalidEncoding, message)
}

fn number(n: usize) -> Result<u32, ObjectError> {
    u32::try_from(n).map_err(|_| invalid("COFF file offset/count exceeds 32 bits"))
}

fn grow(cursor: &mut usize, bytes: usize, limit: usize) -> Result<usize, ObjectError> {
    let start = *cursor;
    *cursor = cursor
        .checked_add(bytes)
        .filter(|n| *n <= limit)
        .ok_or_else(|| ObjectError::limit("write", ObjectResource::OutputBytes))?;
    number(*cursor)?;
    Ok(start)
}

pub fn write(obj: &ObjectFile, max_output_size: u64) -> Result<Vec<u8>, ObjectError> {
    obj.validate_diagnostic()?;
    if obj.target.format != crate::ObjectFormat::COFF {
        return Err(invalid("COFF writer requires a COFF target"));
    }
    let limit = usize::try_from(max_output_size.min(u64::from(u32::MAX))).unwrap_or(usize::MAX);
    if obj.sections.len() > i16::MAX as usize {
        return Err(invalid("COFF standard section count exceeds 32767"));
    }
    let mut cursor = 0;
    // Minimum header and string-table length; metadata staging is also bounded.
    grow(&mut cursor, 24, limit)?;
    if obj.symbols.len().saturating_add(obj.relocations.len()) > limit / 10 {
        return Err(ObjectError::limit("write", ObjectResource::OutputBytes));
    }
    cursor = 20;
    grow(&mut cursor, obj.sections.len() * 40, limit)?;
    let mut strings = symbols::Strings::new(limit);
    let mut relocs = vec![Vec::new(); obj.sections.len()];
    let mut occupied = Vec::new();
    for r in &obj.relocations {
        let (_, width) = relocations::encoding(r.kind, obj.target.machine).map_err(invalid)?;
        occupied.push((r.section_index, r.offset, r.offset + width));
        relocs[r.section_index].push(r);
    }
    occupied.sort_unstable();
    for pair in occupied.windows(2) {
        if pair[0].0 == pair[1].0 && pair[0].2 > pair[1].1 {
            return Err(invalid("overlapping COFF relocation fields"));
        }
    }
    let mut layouts = Vec::new();
    for (section, records) in obj.sections.iter().zip(&relocs) {
        layouts.push(sections::plan(section, records.len(), &mut strings, &mut cursor, limit)?);
    }
    let mut records = Vec::<Vec<u8>>::new();
    let mut indices = HashMap::<&str, u32>::new();
    let mut symbol_count = number(obj.sections.len() * 2)?;
    for symbol in &obj.symbols {
        let record =
            symbols::record(symbol, strings.name(&symbol.name, false)?).map_err(invalid)?;
        indices.insert(&symbol.name, symbol_count);
        symbol_count = symbol_count
            .checked_add((record.len() / 18) as u32)
            .ok_or_else(|| invalid("COFF symbol count overflow"))?;
        records.push(record);
    }
    for r in &obj.relocations {
        if indices.contains_key(r.symbol.as_str()) {
            continue;
        }
        indices.insert(&r.symbol, symbol_count);
        symbol_count =
            symbol_count.checked_add(1).ok_or_else(|| invalid("COFF symbol count overflow"))?;
        records.push(
            symbols::record(
                &ObjectSymbol {
                    name: r.symbol.clone(),
                    section_index: None,
                    value: 0,
                    size: 0,
                    kind: SymbolKind::NoType,
                    binding: SymbolBinding::Global,
                    visibility: SymbolVisibility::Default,
                },
                strings.name(&r.symbol, false)?,
            )
            .map_err(invalid)?,
        );
    }
    let symbols = grow(
        &mut cursor,
        (symbol_count as usize)
            .checked_mul(18)
            .ok_or_else(|| invalid("COFF symbol size overflow"))?,
        limit,
    )?;
    let strings = strings.finish();
    let string_at = grow(&mut cursor, strings.len(), limit)?;
    let mut out = Vec::new();
    out.try_reserve_exact(cursor).map_err(|_| {
        ObjectError::new("write", ObjectErrorKind::HostAllocation, "COFF allocation failed")
    })?;
    out.resize(cursor, 0);
    headers::file(
        &mut out,
        obj.target.machine,
        obj.sections.len() as u16,
        number(symbols)?,
        symbol_count,
    );
    for (i, section) in obj.sections.iter().enumerate() {
        let layout = &layouts[i];
        sections::emit(&mut out, i, layout, section)?;
        sections::emit_relocations(&mut out, i, layout, obj.target.machine, &relocs[i], &indices)?;
        symbols::section(
            &mut out[symbols + i * 36..symbols + (i + 1) * 36],
            &layout.symbol_name,
            i,
            layout.size,
            relocs[i].len(),
        );
    }
    let mut at = symbols + obj.sections.len() * 36;
    for record in records {
        out[at..at + record.len()].copy_from_slice(&record);
        at += record.len();
    }
    out[string_at..].copy_from_slice(&strings);
    Ok(out)
}
