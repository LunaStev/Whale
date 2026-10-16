// SPDX-License-Identifier: MPL-2.0
//! Shared rendering boundary. Component APIs retain their original typed causes.
use std::fmt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceLocation {
    pub source: String,
    pub line: u64,
    pub column: u64,
    pub end_line: Option<u64>,
    pub end_column: Option<u64>,
    pub start_byte: Option<u64>,
    pub end_byte: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Context {
    Source(SourceLocation),
    Object { name: String, member: Option<String> },
    Input(usize),
    Section { index: usize, name: Option<String> },
    FileOffset(u64),
    SectionOffset(u64),
    Symbol(String),
    Function(u32),
    Block(u32),
    Instruction(usize),
    OriginalNode(u64),
}

/// `component`, `phase` and `code` are stable machine-readable identifiers.
/// Renderers must use these fields, never classify `message` by English text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub component: &'static str,
    pub phase: &'static str,
    pub code: &'static str,
    pub message: String,
    pub contexts: Vec<Context>,
}

impl Diagnostic {
    pub fn new(
        component: &'static str,
        phase: &'static str,
        code: &'static str,
        message: impl Into<String>,
    ) -> Self {
        Self { component, phase, code, message: message.into(), contexts: Vec::new() }
    }

    pub fn with_context(mut self, context: Context) -> Self {
        self.contexts.push(context);
        self
    }
}

/// Convert the same component cause for human or structured rendering.
pub trait ToDiagnostic {
    fn diagnostic(&self) -> Diagnostic;
}

impl ToDiagnostic for Diagnostic {
    fn diagnostic(&self) -> Diagnostic {
        self.clone()
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)?;
        for context in &self.contexts {
            match context {
                Context::Source(s) => write!(f, " at {}:{}:{}", s.source, s.line, s.column)?,
                Context::Object { name, member } => {
                    write!(f, " in {name}")?;
                    if let Some(m) = member {
                        write!(f, "({m})")?;
                    }
                },
                Context::Input(i) => write!(f, " input {i}")?,
                Context::Section { index, name } => {
                    write!(f, " section {index}")?;
                    if let Some(n) = name {
                        write!(f, " {n:?}")?;
                    }
                },
                Context::FileOffset(at) => write!(f, " at file offset {at}")?,
                Context::SectionOffset(at) => write!(f, " at section offset {at}")?,
                Context::Symbol(s) => write!(f, " symbol {s:?}")?,
                Context::Function(id) => write!(f, " function @f{id}")?,
                Context::Block(id) => write!(f, " block %b{id}")?,
                Context::Instruction(i) => write!(f, " instruction {i}")?,
                Context::OriginalNode(id) => write!(f, " original @n{id}")?,
            }
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendering_preserves_machine_fields_and_contexts() {
        let d = Diagnostic::new("object", "read", "invalid-index", "symbol section is absent")
            .with_context(Context::Object { name: "lib.a".into(), member: Some("part.o".into()) })
            .with_context(Context::Section { index: 3, name: Some(".text".into()) })
            .with_context(Context::FileOffset(64));
        let render = d.to_string();
        assert!(
            render.contains("lib.a(part.o)") && render.contains(".text") && render.contains("64")
        );
        assert_eq!(d.diagnostic(), d);
        assert_eq!(d.code, "invalid-index");
    }
}
