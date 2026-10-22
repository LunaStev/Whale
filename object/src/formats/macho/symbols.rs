// SPDX-License-Identifier: MPL-2.0
use std::collections::{HashMap, HashSet};

use crate::{ObjectError, ObjectFile, SymbolBinding, SymbolVisibility};

use super::{
    headers::{u32_at, u64_at},
    invalid,
    sections::Layout,
};

pub(super) struct Symbols<'a> {
    pub entries: Vec<Symbol<'a>>,
    pub indices: HashMap<&'a str, u32>,
    pub groups: [u32; 3],
    pub strings: Vec<u8>,
}

pub(super) struct Symbol<'a> {
    pub name: &'a str,
    pub section: Option<usize>,
    pub value: u64,
    pub global: bool,
    pub string: u32,
}

pub(super) fn plan<'a>(obj: &'a ObjectFile, limit: usize) -> Result<Symbols<'a>, ObjectError> {
    let mut groups = [Vec::new(), Vec::new(), Vec::new()];
    let mut known = HashSet::new();
    for s in &obj.symbols {
        if s.binding == SymbolBinding::Weak || s.visibility != SymbolVisibility::Default {
            return Err(invalid("Mach-O weak/hidden symbols are unsupported"));
        }
        if s.section_index.is_none() && s.binding != SymbolBinding::Global {
            return Err(invalid("Mach-O undefined symbols must be global"));
        }
        let group = if s.section_index.is_none() {
            2
        } else if s.binding == SymbolBinding::Global {
            1
        } else {
            0
        };
        known.insert(s.name.as_str());
        groups[group].push(Symbol {
            name: &s.name,
            section: s.section_index,
            value: s.value,
            global: s.binding == SymbolBinding::Global,
            string: 0,
        });
    }
    for r in &obj.relocations {
        if known.insert(&r.symbol) {
            groups[2].push(Symbol {
                name: &r.symbol,
                section: None,
                value: 0,
                global: true,
                string: 0,
            });
        }
    }
    let counts = groups.each_ref().map(|g| g.len() as u32);
    let mut entries = Vec::new();
    let mut indices = HashMap::new();
    let mut strings = vec![0];
    for mut s in groups.into_iter().flatten() {
        let end = strings
            .len()
            .checked_add(s.name.len())
            .and_then(|v| v.checked_add(1))
            .filter(|n| *n <= limit)
            .ok_or_else(|| {
                crate::ObjectError::limit("write", crate::ObjectResource::OutputBytes)
            })?;
        strings
            .try_reserve_exact(end - strings.len())
            .map_err(|_| invalid("Mach-O string allocation failed"))?;
        s.string = strings.len() as u32;
        strings.extend_from_slice(s.name.as_bytes());
        strings.push(0);
        indices.insert(s.name, entries.len() as u32);
        entries.push(s);
    }
    Ok(Symbols { entries, indices, groups: counts, strings })
}

pub(super) fn emit(
    out: &mut [u8],
    at: usize,
    symbols: &Symbols<'_>,
    sections: &[Layout],
) -> Result<(), ObjectError> {
    for (i, s) in symbols.entries.iter().enumerate() {
        let at = at + i * 16;
        u32_at(out, at, s.string);
        out[at + 4] = (if s.section.is_some() { 0x0e } else { 0 }) | u8::from(s.global);
        out[at + 5] = s.section.map_or(0, |n| (n + 1) as u8);
        let value = match s.section {
            Some(n) => sections[n]
                .address
                .checked_add(s.value)
                .ok_or_else(|| invalid("Mach-O symbol value overflow"))?,
            None => 0,
        };
        u64_at(out, at + 8, value);
    }
    Ok(())
}
