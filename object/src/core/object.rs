use crate::core::reloc::ObjectRelocation;
use crate::core::section::{Section, SectionKind};
use crate::core::symbol::ObjectSymbol;
use std::collections::HashSet;

pub use whale_target::{Endian, Machine, ObjectFormat, ObjectTarget, Target};

pub struct ObjectFile {
    pub target: ObjectTarget,
    pub sections: Vec<Section>,
    pub symbols: Vec<ObjectSymbol>,
    pub relocations: Vec<ObjectRelocation>,
}

impl ObjectFile {
    /// Construct an AMD64 little-endian object in the requested format.
    pub fn new(format: ObjectFormat) -> Self {
        Self::with_target(ObjectTarget {
            format,
            ..Target::X86_64WhaleLinux.object_target()
        })
    }

    /// Retain explicit input identity; writers and link consumers validate support.
    pub fn with_target(target: ObjectTarget) -> Self {
        Self {
            target,
            sections: Vec::new(),
            symbols: Vec::new(),
            relocations: Vec::new(),
        }
    }

    pub fn add_section(&mut self, name: &str, kind: SectionKind, align: u64) -> usize {
        self.sections.push(Section {
            name: name.to_string(),
            kind,
            data: Vec::new(),
            zero_fill: 0,
            align,
        });
        self.sections.len() - 1
    }

    /// Validate in-memory structure before writing or consuming this object.
    /// This does not allocate payloads, serialize, or apply an output-size budget.
    pub fn validate(&self) -> Result<(), String> {
        self.target.validate().map_err(|e| e.to_string())?;
        let obj = self;
        for section in &obj.sections {
            section.memory_size()?;
            if section.name.contains('\0') {
                return Err(format!(
                    "section name {:?} contains embedded NUL byte",
                    section.name
                ));
            }
            // Zero means no alignment constraint and is serialized as one.
            if section.align != 0 && !section.align.is_power_of_two() {
                return Err(format!(
                    "section {:?} alignment {} must be zero or a power of two",
                    section.name, section.align
                ));
            }
        }

        // Relocations in the current object API name symbols by string. Until typed
        // symbol references are available, duplicate names are ambiguous even when
        // their metadata happens to agree.
        let mut symbol_names = HashSet::new();
        for sym in &obj.symbols {
            if sym.name.contains('\0') {
                return Err(format!(
                    "symbol name {:?} contains embedded NUL byte",
                    sym.name
                ));
            }
            if !symbol_names.insert(&sym.name) {
                return Err(format!("duplicate symbol name {:?}", sym.name));
            }
            if let Some(index) = sym.section_index {
                let section = obj.sections.get(index).ok_or_else(|| {
                    format!("symbol {:?} references invalid section {index}", sym.name)
                })?;
                let end = sym
                    .value
                    .checked_add(sym.size)
                    .ok_or_else(|| format!("symbol range overflow for {:?}", sym.name))?;
                if end > section.memory_size()? {
                    return Err(format!(
                        "symbol range for {:?} exceeds section {:?} size {}",
                        sym.name,
                        section.name,
                        section.memory_size()?
                    ));
                }
            }
        }

        for reloc in &obj.relocations {
            if reloc.symbol.contains('\0') {
                return Err(format!(
                    "relocation reference {:?} contains embedded NUL byte",
                    reloc.symbol
                ));
            }
            let section = obj.sections.get(reloc.section_index).ok_or_else(|| {
                format!(
                    "relocation for {:?} references invalid section {}",
                    reloc.symbol, reloc.section_index
                )
            })?;
            if section.kind == SectionKind::Bss {
                return Err(format!(
                    "relocations targeting BSS section {:?} are not supported",
                    section.name
                ));
            }
            let width = match reloc.kind {
                crate::RelocKind::Absolute64 => 8,
                crate::RelocKind::Relative8 => 1,
                crate::RelocKind::Absolute32
                | crate::RelocKind::Relative32
                | crate::RelocKind::GOTPCREL
                | crate::RelocKind::PLT32 => 4,
            };
            let end = reloc.offset.checked_add(width).ok_or_else(|| {
                format!(
                    "relocation range overflow in section {:?} at offset {}",
                    section.name, reloc.offset
                )
            })?;
            if end > section.data.len() {
                return Err(format!(
                    "relocation range {}..{} exceeds section {:?} size {}",
                    reloc.offset,
                    end,
                    section.name,
                    section.data.len()
                ));
            }
        }

        Ok(())
    }

    pub fn write_with_limit(&self, max_output_size: u64) -> Result<Vec<u8>, String> {
        match self.target.format {
            ObjectFormat::ELF64 => crate::formats::elf::write_elf_with_limit(self, max_output_size),
        }
    }

    pub fn write(&self) -> Result<Vec<u8>, String> {
        match self.target.format {
            ObjectFormat::ELF64 => crate::formats::elf::write_elf(self),
        }
    }
}
