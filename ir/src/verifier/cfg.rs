//! Read-only SSA checks. Reachability never removes or simplifies O0 IR.
use std::collections::{HashMap, HashSet};

use crate::{BlockId, Function, Instruction, Terminator, ValueId};

use super::{instr_result, instr_uses, operands, VerifyError};

struct Cfg {
    indices: HashMap<BlockId, usize>,
    predecessors: Vec<Vec<usize>>,
    // None means structurally unreachable from the function entry.
    idom: Vec<Option<usize>>,
}

impl Cfg {
    // Block identity, termination and branch destinations have already passed
    // structural validation. Do not inspect constant branch conditions here.
    fn new(f: &Function) -> Result<Self, VerifyError> {
        let indices: HashMap<_, _> = f
            .blocks
            .iter()
            .enumerate()
            .map(|(i, b)| (b.id, i))
            .collect();
        let mut successors = vec![Vec::new(); f.blocks.len()];
        let mut predecessors = vec![Vec::new(); f.blocks.len()];
        for (index, block) in f.blocks.iter().enumerate() {
            let targets = match block.terminator.as_ref().unwrap() {
                Terminator::Br { target } => vec![*target],
                Terminator::CBr {
                    then_bb, else_bb, ..
                } => vec![*then_bb, *else_bb],
                Terminator::Switch {
                    default_bb, cases, ..
                } => std::iter::once(*default_bb)
                    .chain(cases.iter().map(|(_, target)| *target))
                    .collect(),
                Terminator::Ret { .. } | Terminator::Trap { .. } => vec![],
            };
            let mut seen = HashSet::new();
            for target in targets {
                let target = indices[&target];
                if seen.insert(target) {
                    successors[index].push(target);
                    predecessors[target].push(index);
                }
            }
        }

        // Iterative DFS avoids consuming the host call stack for deep CFGs.
        let entry = indices[&f.entry];
        // Entry represents a fresh invocation, never a branch destination.
        // Check all edges, including those from unreachable blocks, without
        // rewriting the original control flow.
        if let Some(&predecessor) = predecessors[entry].first() {
            return Err(VerifyError::EntryHasPredecessor {
                func: f.name.clone(),
                entry: f.entry,
                predecessor: f.blocks[predecessor].id,
            });
        }
        let mut seen = vec![false; f.blocks.len()];
        let mut stack = vec![(entry, false)];
        let mut order = Vec::new();
        while let Some((block, exiting)) = stack.pop() {
            if exiting {
                order.push(block);
                continue;
            }
            if std::mem::replace(&mut seen[block], true) {
                continue;
            }
            stack.push((block, true));
            stack.extend(successors[block].iter().rev().map(|&next| (next, false)));
        }
        order.reverse();
        let mut rank = vec![0; f.blocks.len()];
        for (i, &block) in order.iter().enumerate() {
            rank[block] = i;
        }
        let mut idom = vec![None; f.blocks.len()];
        idom[entry] = Some(entry);
        loop {
            let mut changed = false;
            for &block in order.iter().skip(1) {
                let mut next: Option<usize> = None;
                for &predecessor in &predecessors[block] {
                    if idom[predecessor].is_none() {
                        continue;
                    }
                    next = Some(match next {
                        None => predecessor,
                        Some(mut a) => {
                            let mut b = predecessor;
                            while a != b {
                                if rank[a] > rank[b] {
                                    a = idom[a].unwrap();
                                } else {
                                    b = idom[b].unwrap();
                                }
                            }
                            a
                        }
                    });
                }
                if next != idom[block] {
                    idom[block] = next;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Ok(Self {
            indices,
            predecessors,
            idom,
        })
    }

    fn dominates(&self, definition: usize, mut usage: usize) -> bool {
        loop {
            if definition == usage {
                return true;
            }
            match self.idom[usage] {
                Some(parent) if parent != usage => usage = parent,
                _ => return false,
            }
        }
    }
}

pub(super) fn verify(f: &Function) -> Result<(), VerifyError> {
    let cfg = Cfg::new(f)?;
    let mut definitions: HashMap<ValueId, Option<(usize, usize)>> =
        f.params.iter().map(|p| (p.id, None)).collect();
    for (block, b) in f.blocks.iter().enumerate() {
        for (position, ins) in b.instructions.iter().enumerate() {
            if let Some((value, _)) = instr_result(ins) {
                definitions.insert(value, Some((block, position)));
            }
        }
    }
    let available = |value: ValueId, block: usize, position: usize| {
        let definition =
            definitions
                .get(&value)
                .ok_or_else(|| VerifyError::UseOfUndefinedValue {
                    func: f.name.clone(),
                    value,
                })?;
        // Unreachable code remains in the module and still undergoes identity,
        // operand/type and CFG checks. No executable entry path exists on which
        // to require a dominating definition for its uses.
        if cfg.idom[block].is_none() {
            return Ok(());
        }
        if let Some((def_block, def_position)) = *definition {
            let valid = if def_block == block {
                def_position < position
            } else {
                cfg.dominates(def_block, block)
            };
            if !valid {
                return Err(VerifyError::NonDominatingValue {
                    func: f.name.clone(),
                    value,
                    block: f.blocks[block].id,
                });
            }
        }
        Ok(())
    };
    for (block, b) in f.blocks.iter().enumerate() {
        let mut past_phi = false;
        for (position, ins) in b.instructions.iter().enumerate() {
            if let Instruction::Phi { dst, ty, incomings } = ins {
                let invalid = |reason| VerifyError::InvalidPhi {
                    func: f.name.clone(),
                    block: b.id,
                    value: *dst,
                    reason,
                };
                if past_phi {
                    return Err(invalid("phi must precede non-phi instructions"));
                }
                if b.id == f.entry {
                    return Err(invalid("function entry has no incoming SSA value"));
                }
                if incomings.is_empty() {
                    return Err(invalid("phi requires incoming values"));
                }
                operands::verify_type_category(f, "phi", ty, operands::is_storable(ty))?;
                let mut seen = HashSet::new();
                for (value, predecessor) in incomings {
                    let Some(&pred) = cfg.indices.get(predecessor) else {
                        return Err(invalid("phi names an unknown predecessor"));
                    };
                    if !cfg.predecessors[block].contains(&pred) {
                        return Err(invalid("phi input is not a predecessor"));
                    }
                    if !seen.insert(pred) {
                        return Err(invalid("duplicate phi predecessor"));
                    }
                    operands::verify_operand(f, *value, ty)?;
                    // A phi operand is used on its incoming edge, after that
                    // predecessor's instructions, including loop backedges.
                    available(*value, pred, f.blocks[pred].instructions.len())?;
                }
                if seen.len() != cfg.predecessors[block].len() {
                    return Err(invalid("phi is missing a predecessor"));
                }
            } else {
                past_phi = true;
                for value in instr_uses(ins) {
                    available(value, block, position)?;
                }
            }
        }
        let uses = match b.terminator.as_ref().unwrap() {
            Terminator::CBr { cond, .. } => Some(*cond),
            Terminator::Switch { value, .. } => Some(*value),
            Terminator::Ret { value, .. } => *value,
            Terminator::Br { .. } | Terminator::Trap { .. } => None,
        };
        if let Some(value) = uses {
            available(value, block, b.instructions.len())?;
        }
    }
    Ok(())
}
