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
    /// Zero (no constraint) or a power of two. ELF serializes zero as one.
    pub align: u64,
}
