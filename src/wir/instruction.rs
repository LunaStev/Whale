use crate::wir::value::Value;

pub enum Instruction {
    Print {
        format: String,
        args: Vec<Value>,
    },
    Store {
        name: String,
        value: Value,
    },
    If {
        condition: Value,
        then_block: Vec<Instruction>,
        else_block: Option<Vec<Instruction>>,
    },
    While {
        condition: Value,
        body: Vec<Instruction>,
    },
    For {
      init: Box<Instruction>,
        condition: Value,
        step: Box<Instruction>,
        body: Vec<Instruction>,
    },
    Return(Value),
    Add,
    Sub,
    Mul,
    Div,
}