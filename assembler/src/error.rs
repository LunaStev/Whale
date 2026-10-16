use std::fmt;

/// Half-open UTF-8 byte range; line/column are one-based Unicode-scalar coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone)]
pub enum AsmError {
    LexerError(String),
    ParserError(String),
    EncodeError(String),
    SymbolError(String),
    UnexpectedToken(String),
    Diagnostic {
        source_name: String,
        span: SourceSpan,
        related: Option<SourceSpan>,
        error: Box<AsmError>,
    },
}

impl AsmError {
    pub fn at(
        self,
        source_name: impl Into<String>,
        span: SourceSpan,
        related: Option<SourceSpan>,
    ) -> Self {
        if matches!(self, Self::Diagnostic { .. }) {
            return self;
        }
        Self::Diagnostic { source_name: source_name.into(), span, related, error: Box::new(self) }
    }

    pub fn with_source_name(self, source_name: impl Into<String>) -> Self {
        match self {
            Self::Diagnostic { span, related, error, .. } => {
                Self::Diagnostic { source_name: source_name.into(), span, related, error }
            },
            other => other,
        }
    }
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AsmError::LexerError(s) => write!(f, "Lexer error: {}", s),
            AsmError::ParserError(s) => write!(f, "Parser error: {}", s),
            AsmError::EncodeError(s) => write!(f, "Encode error: {}", s),
            AsmError::SymbolError(s) => write!(f, "Symbol error: {}", s),
            AsmError::UnexpectedToken(s) => write!(f, "Unexpected token: {}", s),
            AsmError::Diagnostic { source_name, span, related, error } => {
                write!(
                    f,
                    "{error} at {source_name}: line {}, column {} (bytes {}..{})",
                    span.line, span.column, span.start, span.end
                )?;
                if let Some(previous) = related {
                    write!(f, "; previous declaration at {source_name}: line {}, column {} (bytes {}..{})", previous.line, previous.column, previous.start, previous.end)?;
                }
                Ok(())
            },
        }
    }
}

impl std::error::Error for AsmError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Diagnostic { error, .. } => Some(error.as_ref()),
            _ => None,
        }
    }
}
