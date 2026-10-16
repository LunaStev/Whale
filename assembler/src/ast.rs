#[derive(Debug, Clone)]
pub struct AST {
    pub items: Vec<ASTNode>,
}

#[derive(Debug, Clone)]
pub enum ASTNode {
    Instruction(Instruction),
    Directive(Directive),
    Label(String),
    Section(String),
    Global(String),
    Extern(String),
    Const { name: String, expr: ExprValue },
}

#[derive(Debug, Clone)]
pub struct Instruction {
    pub mnemonic: String,
    pub operands: Vec<Operand>,
}

#[derive(Debug, Clone)]
pub struct Directive {
    pub name: String,
    pub values: Vec<DirectiveValue>,
}

#[derive(Debug, Clone)]
pub enum DirectiveValue {
    Expr(ExprValue),
    StringLiteral(Vec<u8>),
}

#[derive(Debug, Clone)]
pub enum ExprValue {
    Number(i128),
    Symbol { name: String, addend: i128 },
}

#[derive(Debug, Clone)]
pub enum Operand {
    Register(String),
    Immediate(i128),
    Label(String),
    SymbolExpr { name: String, addend: i128 },
    Memory(MemoryOperand),
}

#[derive(Debug, Clone)]
pub struct MemoryOperand {
    /// Explicit data width in bits; absence permits register inference.
    pub width: Option<u8>,
    pub base: Option<String>,
    pub index: Option<String>,
    pub scale: u8,
    pub disp: i64,
    pub symbol: Option<String>,
}
