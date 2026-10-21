// SPDX-License-Identifier: MPL-2.0
use super::headers::{u16_at, u32_at};

use crate::{
    ObjectError, ObjectErrorKind, ObjectResource, ObjectSymbol, SymbolBinding, SymbolKind,
    SymbolVisibility,
};

use std::collections::HashMap;

pub(super) struct Strings {
    pub bytes: Vec<u8>,
    positions: HashMap<String, u32>,
    limit: usize,
}

impl Strings {
    pub fn new(limit: usize) -> Self {
        Self { bytes: vec![0; 4], positions: HashMap::new(), limit }
    }

    pub fn add(&mut self, name: &str) -> Result<u32, ObjectError> {
        if name.is_empty() || name.contains('\0') {
            return Err(ObjectError::new(
                "write",
                ObjectErrorKind::InvalidName,
                "COFF names must be nonempty without NUL",
            ));
        }
        if let Some(&position) = self.positions.get(name) {
            return Ok(position);
        }
        let end = self
            .bytes
            .len()
            .checked_add(name.len())
            .and_then(|n| n.checked_add(1))
            .filter(|n| *n <= self.limit && u32::try_from(*n).is_ok())
            .ok_or_else(|| ObjectError::limit("write", ObjectResource::OutputBytes))?;
        let position = self.bytes.len() as u32;
        self.bytes.try_reserve(end - self.bytes.len()).map_err(|_| {
            ObjectError::new(
                "write",
                ObjectErrorKind::HostAllocation,
                "COFF string allocation failed",
            )
        })?;
        self.bytes.extend_from_slice(name.as_bytes());
        self.bytes.push(0);
        self.positions.insert(name.into(), position);
        Ok(position)
    }

    pub fn name(&mut self, name: &str, section: bool) -> Result<[u8; 8], ObjectError> {
        if name.is_empty() || name.contains('\0') {
            return Err(ObjectError::new(
                "write",
                ObjectErrorKind::InvalidName,
                "COFF names must be nonempty without NUL",
            ));
        }
        let mut field = [0; 8];
        if name.len() <= 8 && !(section && name.starts_with('/')) {
            field[..name.len()].copy_from_slice(name.as_bytes());
        } else {
            let at = self.add(name)?;
            if section {
                let text = format!("/{at}");
                if text.len() > 8 {
                    return Err(ObjectError::new(
                        "write",
                        ObjectErrorKind::Unsupported,
                        "COFF long section name offset exceeds decimal field",
                    ));
                }
                field[..text.len()].copy_from_slice(text.as_bytes());
            } else {
                field[4..].copy_from_slice(&at.to_le_bytes());
            }
        }
        Ok(field)
    }

    pub fn finish(mut self) -> Vec<u8> {
        let n = self.bytes.len() as u32;
        u32_at(&mut self.bytes, 0, n);
        self.bytes
    }
}

pub(super) fn record(symbol: &ObjectSymbol, name: [u8; 8]) -> Result<Vec<u8>, String> {
    if symbol.binding == SymbolBinding::Weak || symbol.visibility != SymbolVisibility::Default {
        return Err("COFF weak/hidden symbols are outside the initial subset".into());
    }
    if symbol.section_index.is_none()
        && (symbol.value != 0 || symbol.size != 0 || symbol.binding != SymbolBinding::Global)
    {
        return Err("COFF undefined symbols require global binding and zero value/size".into());
    }
    let aux = symbol.kind == SymbolKind::Function && symbol.size != 0;
    if aux && symbol.binding != SymbolBinding::Global {
        return Err("COFF sized local functions are unsupported".into());
    }
    let mut out = vec![0; if aux { 36 } else { 18 }];
    out[..8].copy_from_slice(&name);
    u32_at(
        &mut out,
        8,
        u32::try_from(symbol.value).map_err(|_| "COFF symbol value exceeds 32 bits")?,
    );
    u16_at(&mut out, 12, symbol.section_index.map_or(0, |n| (n + 1) as u16));
    u16_at(&mut out, 14, if symbol.kind == SymbolKind::Function { 0x20 } else { 0 });
    out[16] = if symbol.binding == SymbolBinding::Local { 3 } else { 2 };
    out[17] = u8::from(aux);
    if aux {
        u32_at(
            &mut out,
            22,
            u32::try_from(symbol.size).map_err(|_| "COFF function size exceeds 32 bits")?,
        );
    }
    Ok(out)
}

pub(super) fn section(out: &mut [u8], name: &[u8; 8], index: usize, size: u32, relocations: usize) {
    out[..8].copy_from_slice(name);
    u16_at(out, 12, (index + 1) as u16);
    out[16] = 3;
    out[17] = 1;
    u32_at(out, 18, size);
    u16_at(out, 22, relocations as u16);
}
