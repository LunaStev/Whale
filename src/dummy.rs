#[derive(Debug)]
pub enum Expr {
    Integer(i32),
    String(String),
    Bool(bool),
    Identifier(String),
    // 나중에: BinaryOp, FunctionCall 등 추가
}

#[derive(Debug)]
pub enum Statement {
    Print(String),
    Assign(String, Expr),
}

#[derive(Debug)]
pub struct AST {
    pub statements: Vec<Statement>,
}