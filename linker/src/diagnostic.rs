//! Machine-readable conversion boundary for reusable linker entry points.
use std::fmt;

use diagnostic::{Context, Diagnostic, ToDiagnostic};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkErrorKind {
    Input,
    Selection,
    Symbols,
    Entry,
    Layout,
    Relocation,
    Emission,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkError {
    pub kind: LinkErrorKind,
    pub message: String,
    pub contexts: Vec<Context>,
    pub input_cause: Option<Box<object::ObjectError>>,
    pub relocation_cause: Option<Box<crate::core::relocation::RelocationError>>,
}

impl LinkError {
    pub fn new(kind: LinkErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            contexts: Vec::new(),
            input_cause: None,
            relocation_cause: None,
        }
    }
}

impl ToDiagnostic for LinkError {
    fn diagnostic(&self) -> Diagnostic {
        let (phase, code) = match self.kind {
            LinkErrorKind::Input => ("input", "link.input"),
            LinkErrorKind::Selection => ("selection", "link.selection"),
            LinkErrorKind::Symbols => ("symbols", "link.symbols"),
            LinkErrorKind::Entry => ("entry", "link.entry"),
            LinkErrorKind::Layout => ("layout", "link.layout"),
            LinkErrorKind::Relocation => ("relocation", "link.relocation"),
            LinkErrorKind::Emission => ("emit", "link.emit"),
        };

        let mut d = Diagnostic::new("linker", phase, code, self.message.clone());
        d.contexts = self.contexts.clone();
        d
    }
}

impl fmt::Display for LinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic().fmt(f)
    }
}

impl std::error::Error for LinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.input_cause.as_deref().map(|e| e as &(dyn std::error::Error + 'static)).or_else(|| {
            self.relocation_cause.as_deref().map(|e| e as &(dyn std::error::Error + 'static))
        })
    }
}
