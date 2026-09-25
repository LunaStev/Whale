use crate::ast::AST;
use crate::error::AsmError;
use crate::tokens::tokenize;
use crate::traits::ISA;

pub struct AssemblerOutput {
    pub sections: Vec<AsmSection>,
    pub symbols: Vec<AsmSymbol>,
}

pub struct AsmSection {
    pub name: String,
    pub data: Vec<u8>,
    /// Unbacked zero storage, used by .bss instead of allocating bytes.
    pub zero_fill: u64,
    pub relocs: Vec<Relocation>,
}

pub struct AsmSymbol {
    pub name: String,
    pub section_index: Option<usize>,
    pub offset: u64,
    pub is_global: bool,
}

#[derive(Debug, Clone)]
pub struct Relocation {
    pub offset: usize,
    pub symbol: String,
    pub kind: RelocKind,
    pub addend: i64,
}

#[derive(Debug, Clone)]
pub enum RelocKind {
    Absolute64,
    Absolute32,
    /// A PC-relative data/address reference, independent of symbol definition.
    Relative32,
    /// A near control transfer; an undefined target may require a PLT entry.
    Branch32,
    Relative8,
}

pub fn assemble(source: &str, isa: &impl ISA) -> Result<AssemblerOutput, AsmError> {
    let tokens = tokenize(source)?;

    let ast: AST = isa.parse(&tokens)?;

    let encoded = isa.encode(&ast)?;

    Ok(encoded)
}
