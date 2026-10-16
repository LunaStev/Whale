use super::{
    archive,
    executable::write_executable_checked,
    selection::{select, LinkInput},
};
use crate::diagnostic::{LinkError, LinkErrorKind};
use object::ObjectFile;

pub struct Linker {
    inputs: Vec<LinkInput>,
    output_format: OutputFormat,
    entry: String,
}
pub enum OutputFormat {
    ELF64Executable,
}
impl Linker {
    pub fn new(format: OutputFormat) -> Self {
        Self {
            inputs: Vec::new(),
            output_format: format,
            entry: "_start".into(),
        }
    }
    pub fn add_object(&mut self, object: ObjectFile) {
        self.inputs.push(LinkInput::Object(object));
    }
    pub fn add_archive(&mut self, bytes: &[u8]) -> Result<(), LinkError> {
        let members = archive::read_objects(bytes)?;
        self.inputs.push(LinkInput::Archive(members));
        Ok(())
    }
    pub fn set_entry(&mut self, entry: impl Into<String>) {
        self.entry = entry.into();
    }
    pub fn link(&mut self) -> Result<Vec<u8>, String> {
        self.link_checked().map_err(|e| e.to_string())
    }
    pub fn link_checked(&mut self) -> Result<Vec<u8>, LinkError> {
        self.link_with_limit(256 * 1024 * 1024)
    }
    pub fn link_with_limit(&mut self, max_bytes: u64) -> Result<Vec<u8>, LinkError> {
        for (index, input) in self.inputs.iter().enumerate() {
            match input {
                LinkInput::Object(o) => o.validate_diagnostic().map_err(|e| LinkError {
                    kind: LinkErrorKind::Input,
                    message: format!("link input {index}: {e}"),
                    contexts: {
                        let mut contexts = e.contexts.clone();
                        contexts.push(whale_diagnostic::Context::Input(index));
                        contexts
                    },
                    input_cause: Some(Box::new(e)),
                    relocation_cause: None,
                })?,
                LinkInput::Archive(members) => {
                    for m in members {
                        m.object.validate().map_err(|e| {
                            LinkError::new(
                                LinkErrorKind::Input,
                                format!("member {:?}: {e}", m.name),
                            )
                        })?;
                    }
                }
            }
        }
        let objects = select(&self.inputs, &self.entry, max_bytes)
            .map_err(|e| LinkError::new(LinkErrorKind::Selection, e))?;
        // Resolve first to preserve the public unresolved-reference diagnostic boundary.
        let mut symbols = super::symbol_table::SymbolTable::new();
        symbols
            .resolve(&objects)
            .map_err(|e| LinkError::new(LinkErrorKind::Symbols, e))?;
        match self.output_format {
            OutputFormat::ELF64Executable => {
                write_executable_checked(&objects, &self.entry, max_bytes)
            }
        }
    }
}
