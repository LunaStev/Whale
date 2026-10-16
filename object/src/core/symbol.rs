#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolBinding {
    Local,
    Global,
    Weak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolVisibility {
    Default,
    Hidden,
}

/// ELF symbol classification. Legacy assembly labels remain `NoType`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    #[default]
    NoType,
    Function,
    Object,
}

#[derive(Clone)]
pub struct ObjectSymbol {
    pub kind: SymbolKind,
    pub name: String,
    pub section_index: Option<usize>,
    pub value: u64,
    pub size: u64,
    pub binding: SymbolBinding,
    pub visibility: SymbolVisibility,
}
