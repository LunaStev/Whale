use std::fs::File;
use std::io::Write;
use std::process::Command;

pub fn write_elf(asm: &str, output_name: &str) {
    // Step 1: Save .asm files
    let mut asm_file = File::create("temp.asm").expect("Failed to create temp.asm");
    asm_file.write_all(asm.as_bytes()).expect("Failed to write asm");

impl ELFWriter {
    pub fn new() -> Self {
        ELFWriter {}
    }

    pub fn write(&self) {
        println!("Writing ELF file...");
    }
}