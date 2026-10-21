// SPDX-License-Identifier: MPL-2.0
//! Conservative observable-effect classification, independent of optimizer policy.
use crate::{BinOp, CastOp, Instruction, Terminator, Type};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Effects {
    pub reads_memory: bool,
    pub writes_memory: bool,
    pub volatile: bool,
    pub calls: bool,
    pub may_trap: bool,
    pub changes_lifetime: bool,
}

impl Effects {
    /// Necessary, never sufficient, for deleting an unused instruction. A pass
    /// must also prove no SSA/source/CFG identity or required observation is lost.
    pub fn permits_unused_removal(self) -> bool {
        self == Self::default()
    }
}

impl Instruction {
    pub fn effects(&self) -> Effects {
        use Instruction::*;
        let mut e = Effects::default();
        match self {
            VolatileLoad { .. } => {
                e.reads_memory = true;
                e.volatile = true;
                e.may_trap = true;
            },
            VolatileStore { .. } => {
                e.writes_memory = true;
                e.volatile = true;
                e.may_trap = true;
            },
            Load { .. } => {
                e.reads_memory = true;
                e.may_trap = true;
            },
            Store { .. } | Uninit { .. } | Memset { .. } => {
                e.writes_memory = true;
                e.may_trap = true;
            },
            Memcpy { .. } => {
                e.reads_memory = true;
                e.writes_memory = true;
                e.may_trap = true;
            },
            Alloca { .. } | LifetimeEnd { .. } => {
                e.changes_lifetime = true;
                e.may_trap = true;
            },
            Call { .. } => {
                e.calls = true;
                e.reads_memory = true;
                e.writes_memory = true;
                e.may_trap = true;
            },
            Gep { .. } | PtrDiff { .. } | TrapIf { .. } | Undef { .. } => e.may_trap = true,
            Bin { op: BinOp::UDiv | BinOp::SDiv | BinOp::URem | BinOp::SRem, .. } => {
                e.may_trap = true
            },
            Cast { op: CastOp::CheckedInt | CastOp::FToI_S | CastOp::FToI_U, .. } => {
                e.may_trap = true
            },
            Cmp { ty: Type::Ptr(_), .. } | ICmp { ty: Type::Ptr(_), .. } => e.may_trap = true,
            Const { .. }
            | ConstDecl { .. }
            | Mov { .. }
            | Bin { .. }
            | Neg { .. }
            | Not { .. }
            | Cmp { .. }
            | ICmp { .. }
            | FCmp { .. }
            | Select { .. }
            | Cast { .. }
            | Phi { .. }
            | Extract { .. }
            | Checked { .. }
            | NullFunction { .. }
            | FunctionAddr { .. }
            | GlobalAddr { .. } => {},
        }
        e
    }
}

impl Terminator {
    pub fn effects(&self) -> Effects {
        Effects { may_trap: matches!(self, Self::Trap { .. }), ..Effects::default() }
    }
}
