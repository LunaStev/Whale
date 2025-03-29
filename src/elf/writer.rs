use std::fs::File;
use std::io::Write;
use std::process::Command;

pub fn write_elf(asm: &str, output_name: &str) {
    // Step 1: Save .asm files
    let mut asm_file = File::create("temp.asm").expect("Failed to create temp.asm");
    asm_file.write_all(asm.as_bytes()).expect("Failed to write asm");

    // Step 2: Create .o file (nasm)
    let nasm_status = Command::new("nasm")
        .args(["-felf64", "temp.asm", "-o", "temp.o"])
        .status()
        .expect("Failed to run nasm");

    if !nasm_status.success() {
        panic!("nasm failed");
    }

    // Step 3: Create ELF executable (ld)
    let ld_status = Command::new("ld")
        .args(["-o", output_name, "temp.o"])
        .status()
        .expect("Failed to run ld");

    if !ld_status.success() {
        panic!("ld failed");
    }
}