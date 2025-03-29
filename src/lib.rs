use crate::dummy::AST;

mod wir;
mod codegen;
mod elf;
mod utils;
mod dummy;

pub fn compile_from_ast(ast: AST) {
    let wir = wir::builder::build(ast);
    let asm = codegen::x86_64::generate(&wir);
    elf::writer::write_elf(&asm, "output");
}