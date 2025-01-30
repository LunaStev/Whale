use std::ffi::c_int;

struct WIR {
    instructions: Vec<Instruction>,
}

enum Instruction {
    Add { lhs: c_int, rhs: c_int },
    Sub { lhs: c_int, rhs: c_int },
    Load { dest: c_int, src: c_int },
    Store { src: c_int, dest: c_int },
}