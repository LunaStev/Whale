use crate::dummy;
use crate::dummy::{AST, Statement};
use super::{instruction::Instruction, value::Value, WIRModule};

pub fn build(ast: AST) -> WIRModule {
    let mut module = WIRModule::new();

    for stmt in ast.statements {
        match stmt {
            Statement::Print(text) => {
                // println("Hello World") → Print { format: "...", args: [] }
                let instr = Instruction::Print {
                    format: text,
                    args: vec![], // {} format factor can be added later
                };
                module.push(instr);
            }

            Statement::Assign(name, val) => {
                // Ex: var a = 10; → Store { name: a, value: ... }
                let value = match val {
                    dummy::Expr::Integer(i) => Value::Integer(i),
                    dummy::Expr::String(s) => Value::String(s),
                    dummy::Expr::Bool(b) => Value::Boolean(b),
                    dummy::Expr::Identifier(id) => Value::Identifier(id),
                };

                module.push(Instruction::Store {
                    name,
                    value,
                });
            }

            // If, When, For, Return, etc. can be added later
        }
    }

    module
}
