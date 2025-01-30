pub struct ELFWriter;

impl ELFWriter {
    pub fn new() -> Self {
        ELFWriter {}
    }

    pub fn write(&self) {
        println!("Writing ELF file...");
    }
}