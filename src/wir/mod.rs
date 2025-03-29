pub mod instruction;
pub mod value;
pub mod builder;

use instruction::Instruction;

pub struct WIRModule {
    pub instructions: Vec<Instruction>,
}

impl WIRModule {
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
        }
    }

    pub fn push(&mut self, instr: Instruction) {
        self.instructions.push(instr);
    }

    pub fn get(&self) -> &Vec<Instruction> {
        &self.instructions
    }
}
