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
        self.link_checked().map_err(|error| error.to_string())
    }

    /// Structured conversion boundary; components retain typed input causes.
    pub fn link_checked(&mut self) -> Result<Vec<u8>, crate::LinkError> {
        use crate::{LinkError, LinkErrorKind};
        use whale_diagnostic::Context;
        for (index, object) in self.objects.iter().enumerate() {
            object.validate_diagnostic().map_err(|error| {
                let mut result =
                    LinkError::new(LinkErrorKind::Input, format!("link input {index}: {error}"));
                result.contexts = error.contexts.clone();
                result.input_cause = Some(Box::new(error));
                result.contexts.push(Context::Input(index));
                result
            })?;
        }
        let mut symbols = super::symbol_table::SymbolTable::new();
        symbols
            .resolve(&self.objects)
            .map_err(|error| LinkError::new(LinkErrorKind::Symbols, error))?;
        match self.output_format {
            OutputFormat::ELF64Executable => self
                .link_elf64()
                .map_err(|error| LinkError::new(LinkErrorKind::Emission, error)),
        }
    }

    fn link_elf64(&mut self) -> Result<Vec<u8>, String> {
        Err("ELF executable linking is not implemented".into())
    }
}
