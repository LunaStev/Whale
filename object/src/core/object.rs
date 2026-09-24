use crate::core::reloc::ObjectRelocation;
use crate::core::section::{Section, SectionKind};
use crate::core::symbol::ObjectSymbol;

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
            align,
        });
        self.sections.len() - 1
    }

    pub fn write(&self) -> Result<Vec<u8>, String> {
        match self.target.format {
            ObjectFormat::ELF64 => crate::formats::elf::write_elf(self),
        }
    }
}
