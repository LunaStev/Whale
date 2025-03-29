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

    // Ending the program
    asm.push_str("  mov rax, 60\n");      // syscall: exit
    asm.push_str("  xor rdi, rdi\n");     // status 0
    asm.push_str("  syscall\n");

    asm
}

fn generate_print(format: &str, _args: &Vec<crate::wir::value::Value>) -> String {
    let mut code = String::new();

    // Simply insert a string literal into the .data section and output it as syscall
    code.push_str("  mov rax, 1\n");              // syscall: write
    code.push_str("  mov rdi, 1\n");              // fd: stdout
    code.push_str("  mov rsi, msg\n");            // msg address
    code.push_str("  mov rdx, msg_len\n");        // msg length
    code.push_str("  syscall\n\n");

    code
}
