// SPDX-License-Identifier: MPL-2.0
use std::fmt;

use whale_diagnostic::{Context, Diagnostic, ToDiagnostic};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectErrorKind {
    InvalidTarget,
    InvalidAlignment,
    InvalidName,
    InvalidSection,
    InvalidSymbol,
    DuplicateSymbol,
    InvalidRelocation,
    OutOfBounds,
    Overflow,
    ResourceLimit,
    Unsupported,
    Truncated,
    InvalidEncoding,
    HostAllocation,
}

impl ObjectErrorKind {
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidTarget => "invalid-target",
            Self::InvalidAlignment => "invalid-alignment",
            Self::InvalidName => "invalid-name",
            Self::InvalidSection => "invalid-section",
            Self::InvalidSymbol => "invalid-symbol",
            Self::DuplicateSymbol => "duplicate-symbol",
            Self::InvalidRelocation => "invalid-relocation",
            Self::OutOfBounds => "out-of-bounds",
            Self::Overflow => "overflow",
            Self::ResourceLimit => "resource-limit",
            Self::Unsupported => "unsupported",
            Self::Truncated => "truncated",
            Self::InvalidEncoding => "invalid-encoding",
            Self::HostAllocation => "host-allocation",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectResource {
    InputBytes,
    Sections,
    ProgramHeaders,
    Symbols,
    Relocations,
    PayloadBytes,
    StringBytes,
    Work,
    OutputBytes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectError {
    pub kind: ObjectErrorKind,
    pub phase: &'static str,
    pub message: String,
    pub contexts: Vec<Context>,
    pub resource: Option<ObjectResource>,
}

impl ObjectError {
    pub fn new(phase: &'static str, kind: ObjectErrorKind, message: impl Into<String>) -> Self {
        Self { kind, phase, message: message.into(), contexts: Vec::new(), resource: None }
    }

    pub fn context(mut self, context: Context) -> Self {
        self.contexts.push(context);
        self
    }

    pub fn at(self, offset: u64) -> Self {
        self.context(Context::FileOffset(offset))
    }

    pub fn section(self, index: usize) -> Self {
        self.context(Context::Section { index, name: None })
    }

    pub fn symbol(self, name: impl Into<String>) -> Self {
        self.context(Context::Symbol(name.into()))
    }

    pub fn limit(phase: &'static str, resource: ObjectResource) -> Self {
        let mut e = Self::new(
            phase,
            ObjectErrorKind::ResourceLimit,
            format!("object {resource:?} limit exceeded"),
        );
        e.resource = Some(resource);
        e
    }
}

impl ToDiagnostic for ObjectError {
    fn diagnostic(&self) -> Diagnostic {
        Diagnostic {
            component: "object",
            phase: self.phase,
            code: self.kind.code(),
            message: self.message.clone(),
            contexts: self.contexts.clone(),
        }
    }
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.diagnostic().fmt(f)
    }
}

impl std::error::Error for ObjectError {}

pub type ObjectReadError = ObjectError;
