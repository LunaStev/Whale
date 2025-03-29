#[derive(Debug, Clone)]
pub enum WIRType {
    I32,
    U32,
    F32,
    Str,
    Bool,
    Char,
    Byte,
    Ptr(Box<WIRType>),
    Array(Box<WIRType>),
}

#[derive(Debug, Clone)]
pub enum Value {
    Integer(i32),
    Unsigned(u32),
    Float(f32),
    String(String),
    Boolean(bool),
    Char(char),
    Byte(u8),
    Identifier(String),
    Pointer(String),
    Array(Vec<Value>),
}