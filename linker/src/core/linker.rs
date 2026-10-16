use object::ObjectFile;

pub struct Linker {
    objects: Vec<ObjectFile>,
    output_format: OutputFormat,
}

pub enum OutputFormat {
    ELF64Executable,
}

impl Linker {
    pub fn new(format: OutputFormat) -> Self {
        Self {
            objects: Vec::new(),
            output_format: format,
        }
    }

    pub fn add_object(&mut self, obj: ObjectFile) {
        self.objects.push(obj);
    }

    pub fn link(&mut self) -> Result<Vec<u8>, String> {
        // Resolve references before any future layout/output work. This also
        // applies the shared structural validator to every in-memory input.
        let mut symbols = super::symbol_table::SymbolTable::new();
        symbols.resolve(&self.objects)?;
        // 1. Symbol Resolution
        // 2. Section Merging & Layout
        // 3. Relocation Processing
        // 4. Output Generation
        match self.output_format {
            OutputFormat::ELF64Executable => self.link_elf64(),
        }
    }

    fn link_elf64(&mut self) -> Result<Vec<u8>, String> {
        Err("ELF executable linking is not implemented".into())
    }
}
