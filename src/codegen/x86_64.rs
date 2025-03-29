use crate::wir::{instruction::Instruction, WIRModule};

pub fn generate(module: &WIRModule) -> String {
    let mut asm = String::new();

    // Default ELF Text Section
    asm.push_str("section .text\n");
    asm.push_str("global _start\n\n");
    asm.push_str("_start:\n");

    for instr in module.get() {
        match instr {
            Instruction::Print { format, args } => {
                asm.push_str(&generate_print(format, args));
            }
            Instruction::Store { name, value } => {
                // For testing: Store first annotate
                asm.push_str(&format!("  ; Store {} = {:?}\n", name, value));
            }
            _ => {
                asm.push_str("  ; [UNIMPLEMENTED INSTRUCTION]\n");
            }
        }
    }
}