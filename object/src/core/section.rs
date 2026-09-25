#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SectionKind {
    Text,
    Data,
    Bss,
    ReadOnlyData,
}

pub struct Section {
    pub name: String,
    pub kind: SectionKind,
    pub data: Vec<u8>,
    /// Additional zero-filled memory without file payload; only valid for BSS.
    pub zero_fill: u64,
    /// Zero (no constraint) or a power of two. ELF serializes zero as one.
    pub align: u64,
}

impl Section {
    /// Logical memory extent. Legacy zero-valued BSS data remains accepted.
    pub fn memory_size(&self) -> Result<u64, String> {
        if self.kind != SectionKind::Bss && self.zero_fill != 0 {
            return Err(format!(
                "section {:?}: zero_fill is only valid for BSS",
                self.name
            ));
        }
        if self.kind == SectionKind::Bss && self.data.iter().any(|b| *b != 0) {
            return Err(format!(
                "BSS section {:?} has a nonzero initializer",
                self.name
            ));
        }
        (self.data.len() as u64)
            .checked_add(self.zero_fill)
            .ok_or_else(|| format!("section {:?}: memory size overflow", self.name))
    }

    pub fn file_size(&self) -> u64 {
        if self.kind == SectionKind::Bss {
            0
        } else {
            self.data.len() as u64
        }
    }
}
