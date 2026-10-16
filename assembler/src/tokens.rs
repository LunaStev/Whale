use crate::error::AsmError;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Identifier(String),
    Number(i128),
    StringLiteral(Vec<u8>),
    Comma,
    Colon,
    LBracket,
    RBracket,
    Plus,
    Minus,
    Multiply,
    Divide,
    Newline,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    /// UTF-8 byte offset into the original source.
    pub position: usize,
    pub line: usize,
    /// One-based Unicode scalar column; tabs count as one column.
    pub column: usize,
}

fn lexer_err<S: Into<String>>(msg: S, line: usize, column: usize) -> AsmError {
    AsmError::LexerError(format!(
        "{} at line {}, column {}",
        msg.into(),
        line,
        column
    ))
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, AsmError> {
    let mut chars = src.chars().peekable();
    let mut tokens = Vec::new();

    let mut pos = 0usize;
    let mut line = 1usize;
    let mut column = 1usize;

    while let Some(&ch) = chars.peek() {
        match ch {
            ';' => {
                while let Some(&c) = chars.peek() {
                    if c == '\n' || c == '\r' {
                        break;
                    }
                    chars.next();
                    pos += c.len_utf8();
                    column += 1;
                }
            }

            ' ' | '\t' => {
                chars.next();
                pos += 1;
                column += 1;
            }

            '\n' | '\r' => {
                tokens.push(Token {
                    kind: TokenKind::Newline,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                if ch == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                    pos += 1;
                }
                line += 1;
                column = 1;
            }

            ',' => {
                tokens.push(Token {
                    kind: TokenKind::Comma,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                column += 1;
            }

            ':' => {
                tokens.push(Token {
                    kind: TokenKind::Colon,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                column += 1;
            }

            '[' => {
                tokens.push(Token {
                    kind: TokenKind::LBracket,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                column += 1;
            }

            ']' => {
                tokens.push(Token {
                    kind: TokenKind::RBracket,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                column += 1;
            }

            '+' => {
                tokens.push(Token {
                    kind: TokenKind::Plus,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                column += 1;
            }

            '-' => {
                tokens.push(Token {
                    kind: TokenKind::Minus,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                column += 1;
            }

            '*' => {
                tokens.push(Token {
                    kind: TokenKind::Multiply,
                    position: pos,
                    line,
                    column,
                });
                chars.next();
                pos += 1;
                column += 1;
            }

            '"' => {
                let start_pos = pos;
                let start_line = line;
                let start_col = column;

                chars.next();
                pos += 1;
                column += 1;

                let mut s = Vec::new();
                let mut closed = false;
                // Multiline quoted strings preserve their source bytes. CRLF is
                // one display line, as are lone CR and LF.
                while let Some(c) = chars.next() {
                    pos += c.len_utf8();
                    if c == '"' {
                        column += 1;
                        closed = true;
                        break;
                    }
                    if c == '\\' {
                        let escape_line = line;
                        let escape_column = column;
                        column += 1;
                        let escaped = chars.next().ok_or_else(|| {
                            lexer_err("Truncated escape", escape_line, escape_column)
                        })?;
                        pos += escaped.len_utf8();
                        column += 1;
                        let byte = match escaped {
                            '"' => b'"',
                            '\\' => b'\\',
                            'n' => b'\n',
                            'r' => b'\r',
                            't' => b'\t',
                            '0' => 0,
                            'x' => {
                                let mut value = 0;
                                for _ in 0..2 {
                                    let digit = chars.next().ok_or_else(|| {
                                        lexer_err(
                                            "Truncated byte escape; expected two hex digits",
                                            escape_line,
                                            escape_column,
                                        )
                                    })?;
                                    pos += digit.len_utf8();
                                    column += 1;
                                    let digit = digit
                                        .to_digit(16)
                                        .filter(|_| digit.is_ascii_hexdigit())
                                        .ok_or_else(|| {
                                            lexer_err(
                                                "Invalid byte escape; expected two hex digits",
                                                escape_line,
                                                escape_column,
                                            )
                                        })?;
                                    value = value * 16 + digit as u8;
                                }
                                value
                            }
                            _ => {
                                return Err(lexer_err(
                                    "Unknown string escape",
                                    escape_line,
                                    escape_column,
                                ))
                            }
                        };
                        s.push(byte);
                        continue;
                    }
                    let mut encoded = [0; 4];
                    s.extend_from_slice(c.encode_utf8(&mut encoded).as_bytes());
                    if c == '\r' || c == '\n' {
                        if c == '\r' && chars.peek() == Some(&'\n') {
                            chars.next();
                            pos += 1;
                            s.push(b'\n');
                        }
                        line += 1;
                        column = 1;
                    } else {
                        column += 1;
                    }
                }
                if !closed {
                    return Err(lexer_err(
                        "Unterminated string literal",
                        start_line,
                        start_col,
                    ));
                }

                tokens.push(Token {
                    kind: TokenKind::StringLiteral(s),
                    position: start_pos,
                    line: start_line,
                    column: start_col,
                });
            }

            c if c.is_ascii_digit() => {
                let start_pos = pos;
                let start_line = line;
                let start_col = column;
                let mut n = String::new();

                if ch == '0' {
                    let mut look = chars.clone();
                    look.next();
                    if let Some(radix_mark) = look.next() {
                        if radix_mark == 'x' || radix_mark == 'X' {
                            chars.next();
                            chars.next();
                            pos += 2;
                            column += 2;

                            while let Some(&d) = chars.peek() {
                                if d.is_ascii_hexdigit() {
                                    n.push(d);
                                    chars.next();
                                    pos += 1;
                                    column += 1;
                                } else {
                                    break;
                                }
                            }
                            if n.is_empty() {
                                return Err(lexer_err("Invalid hex number", start_line, start_col));
                            }
                            let parsed = i128::from_str_radix(&n, 16).map_err(|_| {
                                lexer_err("Hex number out of range", start_line, start_col)
                            })?;
                            if parsed > u64::MAX as i128 {
                                return Err(lexer_err(
                                    "Number exceeds unsigned 64-bit range",
                                    start_line,
                                    start_col,
                                ));
                            }
                            tokens.push(Token {
                                kind: TokenKind::Number(parsed),
                                position: start_pos,
                                line: start_line,
                                column: start_col,
                            });
                            continue;
                        } else if radix_mark == 'b' || radix_mark == 'B' {
                            chars.next();
                            chars.next();
                            pos += 2;
                            column += 2;

                            while let Some(&d) = chars.peek() {
                                if d == '0' || d == '1' {
                                    n.push(d);
                                    chars.next();
                                    pos += 1;
                                    column += 1;
                                } else {
                                    break;
                                }
                            }
                            if n.is_empty() {
                                return Err(lexer_err(
                                    "Invalid binary number",
                                    start_line,
                                    start_col,
                                ));
                            }
                            let parsed = i128::from_str_radix(&n, 2).map_err(|_| {
                                lexer_err("Binary number out of range", start_line, start_col)
                            })?;
                            if parsed > u64::MAX as i128 {
                                return Err(lexer_err(
                                    "Number exceeds unsigned 64-bit range",
                                    start_line,
                                    start_col,
                                ));
                            }
                            tokens.push(Token {
                                kind: TokenKind::Number(parsed),
                                position: start_pos,
                                line: start_line,
                                column: start_col,
                            });
                            continue;
                        }
                    }
                }

                while let Some(&d) = chars.peek() {
                    if d.is_ascii_digit() {
                        n.push(d);
                        chars.next();
                        pos += 1;
                        column += 1;
                    } else {
                        break;
                    }
                }
                let parsed = n
                    .parse::<i128>()
                    .map_err(|_| lexer_err("Decimal number out of range", start_line, start_col))?;
                if parsed > u64::MAX as i128 {
                    return Err(lexer_err(
                        "Number exceeds unsigned 64-bit range",
                        start_line,
                        start_col,
                    ));
                }
                tokens.push(Token {
                    kind: TokenKind::Number(parsed),
                    position: start_pos,
                    line: start_line,
                    column: start_col,
                });
            }

            c if c.is_alphanumeric() || c == '_' || c == '.' => {
                let start_pos = pos;
                let start_line = line;
                let start_col = column;
                let mut id = String::new();

                while let Some(&d) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' || d == '.' {
                        id.push(d);
                        chars.next();
                        pos += d.len_utf8();
                        column += 1;
                    } else {
                        break;
                    }
                }

                tokens.push(Token {
                    kind: TokenKind::Identifier(id),
                    position: start_pos,
                    line: start_line,
                    column: start_col,
                });
            }

            _ => return Err(lexer_err(format!("Unexpected char '{}'", ch), line, column)),
        }
    }

    Ok(tokens)
}
