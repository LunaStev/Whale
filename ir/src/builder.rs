// SPDX-License-Identifier: MPL-2.0

use crate::{
    BasicBlock, BinOp, BlockId, Callee, CheckedOp, CmpOp, ConstValue, DataLayout, Function, Global,
    ICmpPred, Instruction, Module, Param, Terminator, Type, ValueId,
};

/// Construction-order errors returned by the `try_*` builder operations.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BuilderError {
    UnknownBlock(BlockId),
    UnknownParameter { index: usize, count: usize },
    UnknownValue(ValueId),
    UnknownGlobal(crate::GlobalId),
    InvalidPointerOperands,
    TerminatedBlock(BlockId),
    PhiAfterInstruction(BlockId),
}

impl std::fmt::Display for BuilderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBlock(id) => write!(f, "unknown builder block {id:?}"),
            Self::UnknownParameter { index, count } => {
                write!(f, "parameter {index} is outside 0..{count}")
            },
            Self::UnknownValue(id) => write!(f, "unknown builder value {id:?}"),
            Self::UnknownGlobal(id) => write!(f, "unknown addressable global {id:?}"),
            Self::InvalidPointerOperands => f.write_str("matching data-pointer operands required"),
            Self::TerminatedBlock(id) => write!(f, "builder block {id:?} is already terminated"),
            Self::PhiAfterInstruction(id) => {
                write!(f, "phi must precede other instructions in {id:?}")
            },
        }
    }
}

impl std::error::Error for BuilderError {}

pub struct ModuleBuilder {
    module: Module,
    next_value: u32,
    next_block: u32,
    next_global: u32,
    #[cfg(feature = "socket")]
    source_work: usize,
    source_shifts: std::collections::HashMap<crate::FunctionId, (usize, usize)>,
    source_mapped_functions: std::collections::HashSet<crate::FunctionId>,
}

impl<'a> FunctionBuilder<'a> {
    pub fn try_global_addr(&mut self, global: crate::GlobalId) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        let ty = self
            .mb
            .module
            .global_storage
            .iter()
            .find(|g| g.id == global)
            .ok_or(BuilderError::UnknownGlobal(global))?
            .ty
            .clone();
        let dst = self.define_value(Type::ptr_to(ty.clone()));
        self.cur_block_mut().instructions.push(Instruction::GlobalAddr { dst, global, ty });
        Ok(dst)
    }

    pub fn try_lifetime_end(&mut self, ptr: ValueId) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[ptr])?;
        if !matches!(self.func.value_type(ptr), Some(Type::Ptr(_))) {
            return Err(BuilderError::InvalidPointerOperands);
        }
        self.cur_block_mut().instructions.push(Instruction::LifetimeEnd { ptr });
        Ok(())
    }

    pub fn try_ptrdiff(&mut self, lhs: ValueId, rhs: ValueId) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        let ty = self.func.value_type(lhs).unwrap();
        if !matches!(ty, Type::Ptr(_)) || self.func.value_type(rhs) != Some(ty) {
            return Err(BuilderError::InvalidPointerOperands);
        }
        let dst = self.define_value(Type::I128);
        self.cur_block_mut().instructions.push(Instruction::PtrDiff { dst, lhs, rhs });
        Ok(dst)
    }

    pub fn current_block_id(&self) -> BlockId {
        self.insert_block
    }

    pub fn try_not(&mut self, ty: Type, src: ValueId) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[src])?;
        Ok(self.not(ty, src))
    }

    pub fn not(&mut self, ty: Type, src: ValueId) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Not { dst, ty, src });
        dst
    }

    /// Append a phi at a block's instruction prefix. Inputs are checked for
    /// existence; the verifier checks predecessor edges, dominance and types.
    pub fn try_phi(
        &mut self,
        ty: Type,
        incomings: Vec<(ValueId, BlockId)>,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        for (value, block) in &incomings {
            self.ensure_values(&[*value])?;
            self.ensure_block(*block)?;
        }
        if self._cur_block().instructions.iter().any(|i| !matches!(i, Instruction::Phi { .. })) {
            return Err(BuilderError::PhiAfterInstruction(self.insert_block));
        }
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Phi { dst, ty, incomings });
        Ok(dst)
    }

    pub fn entry_block(&self) -> BlockId {
        self.func.entry
    }

    /// Legacy diagnostic construction; verification rejects this instruction.
    pub fn undef(&mut self, ty: Type) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Undef { dst, ty });
        dst
    }

    fn _cur_block(&self) -> &BasicBlock {
        self.func.blocks.iter().find(|b| b.id == self.insert_block).expect("insert block not found")
    }

    pub fn is_current_block_terminated(&self) -> bool {
        self.func
            .blocks
            .iter()
            .find(|b| b.id == self.insert_block)
            .map(|b| b.terminator.is_some())
            .unwrap_or(false)
    }

    pub fn alloca_in_entry(&mut self, ty: Type, align: u32) -> ValueId {
        let dst = self.define_value(Type::Ptr(Box::new(ty.clone())));
        let entry = self.func.entry;

        let entry_block =
            self.func.blocks.iter_mut().find(|b| b.id == entry).expect("entry block not found");

        let pos = entry_block
            .instructions
            .iter()
            .position(|ins| !matches!(ins, Instruction::Alloca { .. }))
            .unwrap_or(entry_block.instructions.len());

        entry_block.instructions.insert(pos, Instruction::Alloca { dst, ty, align });

        if self.mb.source_mapped_functions.contains(&self.func.id) {
            self.mb
                .source_shifts
                .entry(self.func.id)
                .and_modify(|(_, count)| *count += 1)
                .or_insert((pos, 1));
        }

        dst
    }
}

impl ModuleBuilder {
    pub fn new(target: impl Into<String>, datalayout: DataLayout) -> Self {
        Self {
            module: Module::new(target, datalayout),
            next_value: 0,
            next_block: 0,
            next_global: 0,
            #[cfg(feature = "socket")]
            source_work: crate::IrLimits::default().max_analysis_work,
            source_shifts: std::collections::HashMap::new(),
            source_mapped_functions: std::collections::HashSet::new(),
        }
    }

    pub fn add_global(
        &mut self,
        name: impl Into<String>,
        ty: Type,
        init: ConstValue,
        align: u32,
    ) -> crate::GlobalId {
        self.add_global_const(name, crate::ConstExpr::literal(ty, init.clone()), init, align)
    }

    pub fn add_global_const(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        init: ConstValue,
        align: u32,
    ) -> crate::GlobalId {
        let id = crate::GlobalId(self.next_global);
        self.next_global += 1;
        self.module.globals.push(Global {
            id,
            name: name.into(),
            ty: expression.ty.clone(),
            init,
            init_expr: expression,
            align,
        });
        id
    }

    /// Add initialized or explicitly uninitialized addressable scalar storage.
    /// The shared global namespace and explicit external names are checked atomically.
    pub fn try_add_global_storage(
        &mut self,
        mut storage: crate::GlobalStorage,
    ) -> Result<crate::GlobalId, crate::GlobalStorageError> {
        if let Err(error) = storage.validate_with_limits(crate::IrLimits::default()) {
            crate::limits::discard_type(storage.ty);
            return Err(error);
        }
        if self.module.globals.iter().any(|g| g.name == storage.name)
            || self.module.global_storage.iter().any(|g| g.name == storage.name)
        {
            return Err(crate::GlobalStorageError::DuplicateName);
        }
        if storage.link_name.as_ref().is_some_and(|link| {
            self.module.declarations.iter().any(|d| d.link_name.as_ref() == Some(link))
                || self.module.global_storage.iter().any(|g| g.link_name.as_ref() == Some(link))
        }) {
            return Err(crate::GlobalStorageError::LinkNameCollision);
        }
        let id = crate::GlobalId(self.next_global);
        self.next_global =
            self.next_global.checked_add(1).ok_or(crate::GlobalStorageError::DuplicateIdentity)?;
        storage.id = id;
        self.module.global_storage.push(storage);
        Ok(id)
    }

    pub fn declare_function(
        &mut self,
        name: impl Into<String>,
        signature: crate::FunctionSignature,
        linkage: crate::Linkage,
        link_name: Option<String>,
    ) -> Result<crate::FunctionId, crate::CallError> {
        self.declare_function_with_limits(
            name,
            signature,
            linkage,
            link_name,
            crate::IrLimits::default(),
        )
    }

    pub fn declare_function_with_limits(
        &mut self,
        name: impl Into<String>,
        signature: crate::FunctionSignature,
        linkage: crate::Linkage,
        link_name: Option<String>,
        limits: crate::IrLimits,
    ) -> Result<crate::FunctionId, crate::CallError> {
        let name = name.into();
        let candidate = crate::FunctionDecl {
            id: crate::FunctionId(self.module.declarations.len() as u32),
            name,
            signature,
            linkage,
            link_name,
        };
        if let Err(error) = crate::function::validate_decl_with_limits(&candidate, limits) {
            crate::limits::discard_signature(candidate.signature);
            return Err(error);
        }
        if let Some(existing) = self.module.declarations.iter().find(|d| d.name == candidate.name) {
            let mut same = candidate.clone();
            same.id = existing.id;
            return if &same == existing {
                Ok(existing.id)
            } else {
                Err(crate::CallError::ConflictingDeclaration(candidate.name))
            };
        }
        if let Some(link) = &candidate.link_name {
            if self.module.declarations.iter().any(|d| d.link_name.as_ref() == Some(link)) {
                return Err(crate::CallError::LinkNameCollision(link.clone()));
            }
        }
        let id = candidate.id;
        self.module.declarations.push(candidate);
        Ok(id)
    }

    pub fn function_id(&self, name: &str) -> Result<crate::FunctionId, crate::CallError> {
        self.module
            .declarations
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.id)
            .ok_or_else(|| crate::CallError::UnknownFunctionName(name.into()))
    }

    pub fn begin_declared_function(
        &mut self,
        id: crate::FunctionId,
        names: Vec<String>,
    ) -> Result<FunctionBuilder<'_>, crate::CallError> {
        let decl = self
            .module
            .declarations
            .iter()
            .find(|d| d.id == id)
            .ok_or(crate::CallError::UnknownFunction(id))?;
        crate::function::validate_signature(&decl.signature)?;
        let decl = decl.clone();
        if self.module.functions.iter().any(|f| f.id == id) {
            return Err(crate::CallError::DuplicateDefinition(id));
        }
        if names.len() != decl.signature.params.len() {
            return Err(crate::CallError::DefinitionMismatch(id));
        }
        let params = names.into_iter().zip(decl.signature.params).collect();
        Ok(self.build_function(id, decl.name, params, decl.signature.ret))
    }

    pub fn begin_function(
        &mut self,
        name: impl Into<String>,
        params: Vec<(String, Type)>,
        ret_ty: Type,
    ) -> FunctionBuilder<'_> {
        let name = name.into();
        // This legacy convenience API is intentionally infallible. The verifier
        // diagnoses duplicate names and invalid signatures, as before.
        let id = crate::FunctionId(self.module.declarations.len() as u32);
        self.module.declarations.push(crate::FunctionDecl {
            id,
            name: name.clone(),
            signature: crate::FunctionSignature::whale(
                params.iter().map(|(_, t)| t.clone()).collect(),
                ret_ty.clone(),
            ),
            linkage: crate::Linkage::Internal,
            link_name: None,
        });
        self.build_function(id, name, params, ret_ty)
    }

    fn build_function(
        &mut self,
        id: crate::FunctionId,
        name: String,
        params: Vec<(String, Type)>,
        ret_ty: Type,
    ) -> FunctionBuilder<'_> {
        let mut p = Vec::new();
        let mut value_types = Vec::new();

        for (nm, ty) in params {
            let id = self.fresh_value();
            value_types.push((id, ty.clone()));
            p.push(Param { name: nm, id, ty });
        }

        let entry = self.fresh_block();
        let entry_block = BasicBlock::new(entry, "entry");

        let func =
            Function { id, name, params: p, ret_ty, blocks: vec![entry_block], entry, value_types };

        FunctionBuilder { mb: self, func, insert_block: entry }
    }

    fn fresh_value(&mut self) -> ValueId {
        let id = self.next_value;
        self.next_value += 1;
        ValueId(id)
    }

    fn fresh_block(&mut self) -> BlockId {
        let id = self.next_block;
        self.next_block += 1;
        BlockId(id)
    }

    pub fn finish(self) -> Module {
        self.module
    }
}

/// Legacy convenience operations panic on invalid construction order. Use the
/// matching `try_*` operations for externally supplied IDs and recoverable errors.
pub struct FunctionBuilder<'a> {
    mb: &'a mut ModuleBuilder,
    func: Function,
    insert_block: BlockId,
}

#[cfg(feature = "socket")]
pub(crate) struct SourceCheckpoint(Vec<(BlockId, usize, usize, bool)>);

#[cfg(feature = "socket")]
impl FunctionBuilder<'_> {
    fn charge_source_work(&mut self, amount: usize) -> Result<(), crate::LowerError> {
        self.mb.charge_source_work(amount)
    }

    pub(crate) fn retain_const_sources(
        &mut self,
        expression: &crate::lower_ast::frontend::Expr,
    ) -> Result<(), crate::LowerError> {
        self.mb.retain_const_sources(expression)
    }

    /// Flush the final hoisting adjustment while resource failures can still be
    /// returned to the frontend. Public finish then has no pending metadata scan.
    pub(crate) fn finish_source_mapping(&mut self) -> Result<(), crate::LowerError> {
        if self.mb.source_shifts.contains_key(&self.func.id) {
            self.charge_source_work(self.mb.module.sources.mappings.len())?;
            self.flush_source_shifts();
        }
        Ok(())
    }

    pub(crate) fn source_checkpoint(&mut self) -> Result<SourceCheckpoint, crate::LowerError> {
        self.charge_source_work(self.func.blocks.len())?;
        let work = self.func.blocks.iter().map(|b| b.instructions.len()).sum::<usize>();
        self.charge_source_work(work + self.mb.module.sources.mappings.len())?;
        self.flush_source_shifts();
        Ok(SourceCheckpoint(
            self.func
                .blocks
                .iter()
                .map(|b| {
                    (
                        b.id,
                        b.instructions.len(),
                        b.instructions
                            .iter()
                            .take_while(|i| matches!(i, Instruction::Alloca { .. }))
                            .count(),
                        b.terminator.is_some(),
                    )
                })
                .collect(),
        ))
    }

    pub(crate) fn annotate_source(
        &mut self,
        annotation: &crate::SourceAnnotation,
        before: SourceCheckpoint,
    ) -> Result<(), crate::LowerError> {
        self.charge_source_work(self.func.blocks.len() + self.mb.module.sources.files.len())?;
        let work = self.func.blocks.iter().map(|b| b.instructions.len()).sum::<usize>()
            + self.mb.module.sources.mappings.len()
            + self.mb.module.sources.originals.len()
            + self.mb.module.sources.files.iter().map(|f| f.name.len()).sum::<usize>()
            + annotation.source.as_ref().map_or(0, |s| s.file.len());
        self.charge_source_work(work)?;
        self.flush_source_shifts();
        let sources = &mut self.mb.module.sources;
        sources.add_frontend_original(annotation);
        let existing: std::collections::HashSet<_> =
            sources.mappings.iter().map(|m| m.target).collect();
        let before: std::collections::HashMap<_, _> = before
            .0
            .into_iter()
            .map(|(id, len, allocas, terminated)| (id, (len, allocas, terminated)))
            .collect();
        for b in &self.func.blocks {
            let (old_len, old_allocas, old_term) =
                before.get(&b.id).copied().unwrap_or((0, 0, false));
            let allocas = b
                .instructions
                .iter()
                .take_while(|i| matches!(i, Instruction::Alloca { .. }))
                .count();
            let new_allocas = allocas.saturating_sub(old_allocas);
            for index in 0..b.instructions.len() {
                let new = if b.id == self.func.entry {
                    (old_allocas..allocas).contains(&index) || index >= old_len + new_allocas
                } else {
                    index >= old_len
                };
                let target =
                    crate::SourceTarget::Instruction { function: self.func.id, block: b.id, index };
                if new && !existing.contains(&target) {
                    sources.mappings.push(crate::SourceMapping { target, original: annotation.id });
                }
            }
            let target = crate::SourceTarget::Terminator { function: self.func.id, block: b.id };
            if b.terminator.is_some() && !old_term && !existing.contains(&target) {
                sources.mappings.push(crate::SourceMapping { target, original: annotation.id });
            }
        }
        self.mb.source_mapped_functions.insert(self.func.id);
        Ok(())
    }
}

impl FunctionBuilder<'_> {
    fn flush_source_shifts(&mut self) {
        if let Some((position, count)) = self.mb.source_shifts.remove(&self.func.id) {
            for mapping in &mut self.mb.module.sources.mappings {
                if let crate::SourceTarget::Instruction { function, block, index } =
                    &mut mapping.target
                {
                    if *function == self.func.id && *block == self.func.entry && *index >= position
                    {
                        *index += count;
                    }
                }
            }
        }
    }
}

#[cfg(feature = "socket")]
impl ModuleBuilder {
    pub(crate) fn set_source_work_limit(&mut self, work: usize) {
        self.source_work = work;
    }

    fn charge_source_work(&mut self, amount: usize) -> Result<(), crate::LowerError> {
        self.source_work = self
            .source_work
            .checked_sub(amount)
            .ok_or(crate::LowerError::ResourceLimit(crate::AstLimitError::SourceWork))?;
        Ok(())
    }

    fn retain_source(
        &mut self,
        annotation: &crate::SourceAnnotation,
    ) -> Result<(), crate::LowerError> {
        let work = 1
            + self.module.sources.originals.len()
            + self.module.sources.files.len()
            + self.module.sources.files.iter().map(|f| f.name.len()).sum::<usize>()
            + annotation.source.as_ref().map_or(0, |s| s.file.len());
        self.charge_source_work(work)?;
        self.module.sources.add_frontend_original(annotation);
        Ok(())
    }

    pub(crate) fn retain_const_sources(
        &mut self,
        expression: &crate::lower_ast::frontend::Expr,
    ) -> Result<(), crate::LowerError> {
        use crate::lower_ast::frontend::Expr;
        let mut pending = vec![expression];
        while let Some(expression) = pending.pop() {
            self.charge_source_work(1)?;
            match expression {
                Expr::Located { annotation, expression } => {
                    self.retain_source(annotation)?;
                    pending.push(expression);
                },
                Expr::Binary { left, right, .. } | Expr::Cmp { left, right, .. } => {
                    pending.push(right);
                    pending.push(left);
                },
                Expr::BitNot(value) => pending.push(value),
                _ => {},
            }
        }
        Ok(())
    }

    pub(crate) fn annotate_global_source(
        &mut self,
        id: crate::GlobalId,
        annotation: &crate::SourceAnnotation,
    ) -> Result<(), crate::LowerError> {
        self.retain_source(annotation)?;
        self.module.sources.mappings.push(crate::SourceMapping {
            target: crate::SourceTarget::Global(id),
            original: annotation.id,
        });
        Ok(())
    }
}

impl<'a> FunctionBuilder<'a> {
    fn ensure_block(&self, bb: BlockId) -> Result<(), BuilderError> {
        if self.func.blocks.iter().any(|b| b.id == bb) {
            Ok(())
        } else {
            Err(BuilderError::UnknownBlock(bb))
        }
    }

    fn ensure_insertable(&self) -> Result<(), BuilderError> {
        let block = self
            .func
            .blocks
            .iter()
            .find(|b| b.id == self.insert_block)
            .ok_or(BuilderError::UnknownBlock(self.insert_block))?;
        if block.terminator.is_some() {
            Err(BuilderError::TerminatedBlock(block.id))
        } else {
            Ok(())
        }
    }

    fn ensure_values(&self, values: &[ValueId]) -> Result<(), BuilderError> {
        for value in values {
            if self.func.value_type(*value).is_none() {
                return Err(BuilderError::UnknownValue(*value));
            }
        }
        Ok(())
    }

    pub fn try_param_value(&self, index: usize) -> Result<ValueId, BuilderError> {
        self.func
            .params
            .get(index)
            .map(|p| p.id)
            .ok_or(BuilderError::UnknownParameter { index, count: self.func.params.len() })
    }

    pub fn try_set_insert_point(&mut self, bb: BlockId) -> Result<(), BuilderError> {
        self.ensure_block(bb)?;
        self.insert_block = bb;
        Ok(())
    }

    pub fn try_undef(&mut self, ty: Type) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.undef(ty))
    }

    pub fn try_const_decl(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        value: ConstValue,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.const_decl(name, expression, value))
    }

    pub fn try_const_bool(&mut self, v: bool) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.const_bool(v))
    }

    pub fn try_const_i32(&mut self, v: i32) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.const_i32(v))
    }

    pub fn try_add(
        &mut self,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        Ok(self.add(ty, lhs, rhs))
    }

    pub fn try_bin(
        &mut self,
        op: BinOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        Ok(self.bin(op, ty, lhs, rhs))
    }

    pub fn try_icmp(
        &mut self,
        pred: ICmpPred,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        Ok(self.icmp(pred, ty, lhs, rhs))
    }

    pub fn try_cmp(
        &mut self,
        op: CmpOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        Ok(self.cmp(op, ty, lhs, rhs))
    }

    pub fn try_checked(
        &mut self,
        op: CheckedOp,
        ty: Type,
        lhs: ValueId,
        rhs: ValueId,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[lhs, rhs])?;
        Ok(self.checked(op, ty, lhs, rhs))
    }

    pub fn try_extract(
        &mut self,
        tuple_ty: Type,
        dst_ty: Type,
        tuple: ValueId,
        index: u32,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[tuple])?;
        Ok(self.extract(tuple_ty, dst_ty, tuple, index))
    }

    pub fn try_trap_if(
        &mut self,
        cond: ValueId,
        reason: impl Into<String>,
    ) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[cond])?;
        self.trap_if(cond, reason);
        Ok(())
    }

    pub fn try_br(&mut self, target: BlockId) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        self.ensure_block(target)?;
        self.br(target);
        Ok(())
    }

    pub fn try_cbr(
        &mut self,
        cond: ValueId,
        then_bb: BlockId,
        else_bb: BlockId,
    ) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[cond])?;
        self.ensure_block(then_bb)?;
        self.ensure_block(else_bb)?;
        self.cbr(cond, then_bb, else_bb);
        Ok(())
    }

    pub fn try_trap(&mut self, reason: impl Into<String>) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        self.trap(reason);
        Ok(())
    }

    pub fn try_const_int(&mut self, ty: Type, v: i128) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.const_int(ty, v))
    }

    pub fn try_const_uint(&mut self, ty: Type, v: u128) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.const_uint(ty, v))
    }

    pub fn try_const_float(&mut self, ty: Type, v: f64) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.const_float(ty, v))
    }

    pub fn try_const_float_bits(
        &mut self,
        ty: Type,
        v: crate::FloatBits,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.const_float_bits(ty, v))
    }

    pub fn try_alloca(&mut self, ty: Type, align: u32) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        Ok(self.alloca(ty, align))
    }

    pub fn try_alloca_in_entry(&mut self, ty: Type, align: u32) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_block(self.func.entry)?;
        if self.func.blocks.iter().find(|b| b.id == self.func.entry).unwrap().terminator.is_some() {
            return Err(BuilderError::TerminatedBlock(self.func.entry));
        }
        Ok(self.alloca_in_entry(ty, align))
    }

    pub fn try_load(
        &mut self,
        ty: Type,
        ptr: ValueId,
        align: u32,
    ) -> Result<ValueId, BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[ptr])?;
        Ok(self.load(ty, ptr, align))
    }

    pub fn try_uninit(&mut self, ty: Type, ptr: ValueId, align: u32) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[ptr])?;
        self.uninit(ty, ptr, align);
        Ok(())
    }

    pub fn try_store(
        &mut self,
        ty: Type,
        value: ValueId,
        ptr: ValueId,
        align: u32,
    ) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        self.ensure_values(&[value, ptr])?;
        self.store(ty, value, ptr, align);
        Ok(())
    }

    pub fn try_ret(&mut self, value: Option<ValueId>) -> Result<(), BuilderError> {
        self.ensure_insertable()?;
        if let Some(value) = value {
            self.ensure_values(&[value])?;
        }
        self.ret(value);
        Ok(())
    }

    pub fn param_value(&self, index: usize) -> ValueId {
        self.try_param_value(index).expect("invalid builder parameter")
    }

    pub fn const_decl(
        &mut self,
        name: impl Into<String>,
        expression: crate::ConstExpr,
        value: ConstValue,
    ) -> ValueId {
        let dst = self.define_value(expression.ty.clone());
        self.cur_block_mut().instructions.push(Instruction::ConstDecl {
            dst,
            name: name.into(),
            expression,
            value,
        });
        dst
    }

    pub fn create_block(&mut self, name: &str) -> BlockId {
        let id = self.mb.fresh_block();
        self.func.blocks.push(BasicBlock::new(id, name));
        id
    }

    pub fn set_insert_point(&mut self, bb: BlockId) {
        self.try_set_insert_point(bb).expect("invalid builder block");
    }

    fn cur_block_mut(&mut self) -> &mut BasicBlock {
        self.ensure_insertable().expect("invalid builder insertion");
        self.func
            .blocks
            .iter_mut()
            .find(|b| b.id == self.insert_block)
            .expect("insert block not found")
    }

    fn define_value(&mut self, ty: Type) -> ValueId {
        self.ensure_insertable().expect("invalid builder insertion");
        let id = self.mb.fresh_value();
        self.func.value_types.push((id, ty));
        id
    }

    pub fn const_bool(&mut self, v: bool) -> ValueId {
        let dst = self.define_value(Type::Bool);
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty: Type::Bool,
            value: ConstValue::Bool(v),
        });
        dst
    }

    pub fn const_i32(&mut self, v: i32) -> ValueId {
        let dst = self.define_value(Type::I32);
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty: Type::I32,
            value: ConstValue::I(v as i128),
        });
        dst
    }

    pub fn add(&mut self, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Bin {
            dst,
            op: BinOp::Add,
            ty,
            lhs,
            rhs,
        });
        dst
    }

    pub fn bin(&mut self, op: BinOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Bin { dst, op, ty, lhs, rhs });
        dst
    }

    pub fn icmp(&mut self, pred: ICmpPred, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(Type::Bool);
        self.cur_block_mut().instructions.push(Instruction::ICmp { dst, pred, ty, lhs, rhs });
        dst
    }

    pub fn cmp(&mut self, op: CmpOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst = self.define_value(Type::Bool);
        self.cur_block_mut().instructions.push(Instruction::Cmp { dst, op, ty, lhs, rhs });
        dst
    }

    pub fn checked(&mut self, op: CheckedOp, ty: Type, lhs: ValueId, rhs: ValueId) -> ValueId {
        let dst_ty = Type::Tuple(vec![ty.clone(), Type::Bool]);
        let dst = self.define_value(dst_ty);
        self.cur_block_mut().instructions.push(Instruction::Checked { dst, op, ty, lhs, rhs });
        dst
    }

    pub fn extract(&mut self, tuple_ty: Type, dst_ty: Type, tuple: ValueId, index: u32) -> ValueId {
        let dst = self.define_value(dst_ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Extract { dst, dst_ty, tuple, index });
        let _ = tuple_ty;
        dst
    }

    pub fn trap_if(&mut self, cond: ValueId, reason: impl Into<String>) {
        self.cur_block_mut().instructions.push(Instruction::TrapIf { cond, reason: reason.into() });
    }

    pub fn br(&mut self, target: BlockId) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Br { target });
    }

    pub fn cbr(&mut self, cond: ValueId, then_bb: BlockId, else_bb: BlockId) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::CBr { cond, then_bb, else_bb });
    }

    pub fn ret(&mut self, value: Option<ValueId>) {
        let ty = self.func.ret_ty.clone();
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Ret { ty, value });
    }

    pub fn trap(&mut self, reason: impl Into<String>) {
        let blk = self.cur_block_mut();
        blk.terminator = Some(Terminator::Trap { reason: reason.into() });
    }

    pub fn function_id(&self, name: &str) -> Result<crate::FunctionId, crate::CallError> {
        self.mb.function_id(name)
    }

    pub fn null_function(
        &mut self,
        signature: crate::FunctionSignature,
    ) -> Result<ValueId, crate::CallError> {
        if let Err(error) = crate::function::validate_signature(&signature) {
            crate::limits::discard_signature(signature);
            return Err(error);
        }
        self.ensure_insertable().map_err(crate::CallError::Builder)?;
        let dst = self.define_value(Type::FnPtr(Box::new(signature.clone())));
        self.cur_block_mut().instructions.push(Instruction::NullFunction { dst, signature });
        Ok(dst)
    }

    pub fn function_addr(
        &mut self,
        function: crate::FunctionId,
    ) -> Result<ValueId, crate::CallError> {
        let signature = self.callee_signature(&Callee::Direct(function))?;
        crate::function::validate_signature(&signature)?;
        self.ensure_insertable().map_err(crate::CallError::Builder)?;
        let dst = self.define_value(Type::FnPtr(Box::new(signature.clone())));
        self.cur_block_mut().instructions.push(Instruction::FunctionAddr {
            dst,
            function,
            signature,
        });
        Ok(dst)
    }

    pub fn callee_signature(
        &self,
        callee: &Callee,
    ) -> Result<crate::FunctionSignature, crate::CallError> {
        let signature = match callee {
            Callee::Direct(id) => {
                &self
                    .mb
                    .module
                    .declarations
                    .iter()
                    .find(|d| d.id == *id)
                    .ok_or(crate::CallError::UnknownFunction(*id))?
                    .signature
            },
            Callee::Indirect(value) => match self.func.value_type(*value) {
                Some(Type::FnPtr(sig)) => sig,
                Some(_) => return Err(crate::CallError::NotFunctionPointer(*value)),
                None => return Err(crate::CallError::UnknownValue(*value)),
            },
        };
        crate::function::validate_signature(signature)?;
        Ok(signature.clone())
    }

    pub fn call(
        &mut self,
        callee: Callee,
        args: Vec<ValueId>,
    ) -> Result<Option<ValueId>, crate::CallError> {
        self.ensure_insertable().map_err(crate::CallError::Builder)?;
        let signature = self.callee_signature(&callee)?;
        crate::function::validate_signature(&signature)?;
        if args.len() != signature.params.len() {
            return Err(crate::CallError::ArgumentCount {
                expected: signature.params.len(),
                got: args.len(),
            });
        }
        for (index, (arg, expected)) in args.iter().zip(&signature.params).enumerate() {
            let got = self.func.value_type(*arg).ok_or(crate::CallError::UnknownValue(*arg))?;
            if got != expected {
                return Err(crate::CallError::ArgumentType {
                    index,
                    expected: expected.clone(),
                    got: got.clone(),
                });
            }
        }
        let ret_ty = signature.ret;

        let dst = if ret_ty == Type::Void { None } else { Some(self.define_value(ret_ty.clone())) };
        self.cur_block_mut().instructions.push(Instruction::Call {
            convention: signature.convention,
            dst,
            ret_ty,
            callee,
            args,
        });
        Ok(dst)
    }

    pub fn finish(mut self) {
        self.flush_source_shifts();
        self.mb.module.functions.push(self.func);
    }

    pub fn const_int(&mut self, ty: Type, v: i128) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty,
            value: ConstValue::I(v),
        });
        dst
    }

    pub fn const_uint(&mut self, ty: Type, v: u128) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty,
            value: ConstValue::U(v),
        });
        dst
    }

    pub fn const_float(&mut self, ty: Type, v: f64) -> ValueId {
        let width = match ty {
            Type::F16 => 16,
            Type::F32 => 32,
            _ => 64,
        };
        self.const_float_bits(
            ty,
            crate::FloatBits::from_f64(width, v).expect("supported float width"),
        )
    }

    pub fn const_float_bits(&mut self, ty: Type, v: crate::FloatBits) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Const {
            dst,
            ty,
            value: ConstValue::F(v),
        });
        dst
    }

    pub fn alloca(&mut self, ty: Type, align: u32) -> ValueId {
        let dst = self.define_value(Type::Ptr(Box::new(ty.clone())));
        self.cur_block_mut().instructions.push(Instruction::Alloca { dst, ty, align });
        dst
    }

    pub fn load(&mut self, ty: Type, ptr: ValueId, align: u32) -> ValueId {
        let dst = self.define_value(ty.clone());
        self.cur_block_mut().instructions.push(Instruction::Load { dst, ty, ptr, align });
        dst
    }

    /// Revoke initialization at this program point without zeroing storage.
    pub fn uninit(&mut self, ty: Type, ptr: ValueId, align: u32) {
        self.cur_block_mut().instructions.push(Instruction::Uninit { ty, ptr, align });
    }

    pub fn store(&mut self, ty: Type, value: ValueId, ptr: ValueId, align: u32) {
        self.cur_block_mut().instructions.push(Instruction::Store { ty, value, ptr, align });
    }
}
