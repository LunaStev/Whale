#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelocKind {
    Absolute64,
    Absolute32,
    /// Sign-extended 32-bit absolute value (R_X86_64_32S), distinct from unsigned 32.
    SignedAbsolute32,
    Relative32,
    Relative8,
    GOTPCREL,
    PLT32,
}

#[derive(Debug, Clone)]
pub struct ObjectRelocation {
    pub section_index: usize,
    pub offset: usize,
    pub symbol: String,
    pub addend: i64,
    pub kind: RelocKind,
}
