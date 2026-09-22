use crate::assembler::AssemblerOutput;
use crate::ast::AST;
use crate::error::AsmError;

pub trait ISA {
    fn parse(&self, tokens: &[crate::tokens::Token]) -> Result<AST, AsmError>;
    fn encode(&self, ast: &AST) -> Result<AssemblerOutput, AsmError>;
}
