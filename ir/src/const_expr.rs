// SPDX-License-Identifier: MPL-2.0

use crate::{ConstValue, GlobalId, Type, ValueId};

/// Resolved constant identity. Local identities belong to the containing function.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ConstRef {
    Global(GlobalId),
    Local(ValueId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstBinaryOp {
    Add,
    Sub,
    Mul,
    BitAnd,
    BitOr,
    BitXor,
    Div,
    Rem,
    ShiftLeft,
    ShiftRight,
    LogicalAnd,
    LogicalOr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstCompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// A typed compile-time expression, preserved independently of its evaluated
/// result. This tree is not a sequence of runtime instructions.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstExpr {
    pub ty: Type,
    pub kind: ConstExprKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConstExprKind {
    Not(Box<ConstExpr>),
    Literal(ConstValue),
    Reference(ConstRef),
    Binary { op: ConstBinaryOp, left: Box<ConstExpr>, right: Box<ConstExpr> },
    Compare { op: ConstCompareOp, left: Box<ConstExpr>, right: Box<ConstExpr> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConstEvalError {
    ResourceLimit(crate::LimitError),
    InvalidLiteral,
    TypeMismatch,
    UnsupportedOperation,
    DivisionByZero,
    RemainderByZero,
    UnknownReference(ConstRef),
    DuplicateDefinition(ConstRef),
    CyclicReference,
    ResultMismatch,
}

impl ConstExpr {
    pub fn literal(ty: Type, value: ConstValue) -> Self {
        Self { ty, kind: ConstExprKind::Literal(value) }
    }

    pub fn references(&self) -> Vec<ConstRef> {
        let mut pending = vec![self];
        let mut references = Vec::new();
        while let Some(expr) = pending.pop() {
            match &expr.kind {
                ConstExprKind::Literal(_) => {},
                ConstExprKind::Not(value) => pending.push(value),
                ConstExprKind::Reference(reference) => references.push(*reference),
                ConstExprKind::Binary { left, right, .. }
                | ConstExprKind::Compare { left, right, .. } => {
                    pending.push(right);
                    pending.push(left);
                },
            }
        }
        references
    }

    pub fn evaluate(
        &self,
        resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
    ) -> Result<ConstValue, ConstEvalError> {
        self.evaluate_with_limits(resolve, crate::IrLimits::default())
    }

    pub fn evaluate_with_limits(
        &self,
        resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
        limits: crate::IrLimits,
    ) -> Result<ConstValue, ConstEvalError> {
        self.evaluate_with_error_site(resolve, limits).map_err(|(error, _)| error)
    }

    /// Preserve the exact failing node without repeating validation, reference
    /// resolution or arithmetic. Frontend annotations stay outside typed trees.
    pub(crate) fn evaluate_with_error_site(
        &self,
        resolve: &impl Fn(ConstRef) -> Option<(Type, ConstValue)>,
        limits: crate::IrLimits,
    ) -> Result<ConstValue, (ConstEvalError, &ConstExpr)> {
        let mut budget = crate::limits::Budget::new(limits)
            .map_err(|error| (ConstEvalError::ResourceLimit(error), self))?;
        budget.expression(self).map_err(|error| (ConstEvalError::ResourceLimit(error), self))?;
        // Validate every preserved branch before evaluating lazily. Invalid IR
        // is rejected even if its expression would be skipped at execution.
        let mut inspect = vec![self];
        let mut resolved = std::collections::HashMap::new();
        while let Some(expr) = inspect.pop() {
            match &expr.kind {
                ConstExprKind::Literal(v) => {
                    if !crate::constant::valid_constant(&expr.ty, v) {
                        return Err((ConstEvalError::InvalidLiteral, expr));
                    }
                },
                ConstExprKind::Reference(reference) => {
                    let (ty, value) = resolve(*reference)
                        .ok_or((ConstEvalError::UnknownReference(*reference), expr))?;
                    if let Err(error) = budget.ty(&ty) {
                        crate::limits::discard_type(ty);
                        return Err((ConstEvalError::ResourceLimit(error), expr));
                    }
                    if ty != expr.ty {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    if !crate::constant::valid_constant(&ty, &value) {
                        return Err((ConstEvalError::InvalidLiteral, expr));
                    }
                    resolved.insert(expr as *const ConstExpr, value);
                },
                ConstExprKind::Not(value) => {
                    if expr.ty != value.ty {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    if ty_int_bits(&expr.ty).is_none() {
                        return Err((ConstEvalError::UnsupportedOperation, expr));
                    }
                    inspect.push(value);
                },
                ConstExprKind::Binary { op, left, right } => {
                    if left.ty != right.ty || expr.ty != left.ty {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    let supported =
                        if matches!(op, ConstBinaryOp::LogicalAnd | ConstBinaryOp::LogicalOr) {
                            expr.ty == Type::Bool
                        } else if matches!(expr.ty, Type::F16 | Type::F32 | Type::F64) {
                            matches!(
                                op,
                                ConstBinaryOp::Add | ConstBinaryOp::Sub | ConstBinaryOp::Mul
                            )
                        } else {
                            ty_int_bits(&expr.ty).is_some()
                        };
                    if !supported {
                        return Err((ConstEvalError::UnsupportedOperation, expr));
                    }
                    inspect.push(right);
                    inspect.push(left);
                },
                ConstExprKind::Compare { op, left, right } => {
                    if left.ty != right.ty || expr.ty != Type::Bool {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    }
                    if left.ty == Type::Bool {
                        if !matches!(op, ConstCompareOp::Eq | ConstCompareOp::Ne) {
                            return Err((ConstEvalError::UnsupportedOperation, expr));
                        }
                    } else if ty_int_bits(&left.ty).is_none()
                        && !matches!(left.ty, Type::F16 | Type::F32 | Type::F64)
                    {
                        return Err((ConstEvalError::UnsupportedOperation, expr));
                    }
                    inspect.push(right);
                    inspect.push(left);
                },
            }
        }
        // An explicit evaluation stack preserves left-to-right order and lazy
        // boolean semantics without recursing on the caller's tree.
        let mut pending = vec![(self, 0u8)];
        let mut values = Vec::new();
        while let Some((expr, phase)) = pending.pop() {
            let value = match &expr.kind {
                ConstExprKind::Literal(value) => value.clone(),
                ConstExprKind::Reference(_) => resolved[&(expr as *const ConstExpr)].clone(),
                ConstExprKind::Not(value) if phase == 0 => {
                    pending.push((expr, 1));
                    pending.push((value, 0));
                    continue;
                },
                ConstExprKind::Not(_) => const_not(&expr.ty, &values.pop().expect("not operand"))
                    .map_err(|error| (error, expr))?,
                ConstExprKind::Binary {
                    op: ConstBinaryOp::LogicalAnd | ConstBinaryOp::LogicalOr,
                    left,
                    right,
                } => {
                    if phase == 0 {
                        pending.push((expr, 1));
                        pending.push((left, 0));
                        continue;
                    }
                    let ConstValue::Bool(value) = values.pop().expect("logical operand") else {
                        return Err((ConstEvalError::TypeMismatch, expr));
                    };
                    let ConstExprKind::Binary { op, .. } = expr.kind else { unreachable!() };
                    if phase == 1
                        && ((op == ConstBinaryOp::LogicalAnd && value)
                            || (op == ConstBinaryOp::LogicalOr && !value))
                    {
                        pending.push((expr, 2));
                        pending.push((right, 0));
                        continue;
                    }
                    ConstValue::Bool(value)
                },
                ConstExprKind::Binary { left, right, .. }
                | ConstExprKind::Compare { left, right, .. }
                    if phase == 0 =>
                {
                    pending.push((expr, 1));
                    pending.push((right, 0));
                    pending.push((left, 0));
                    continue;
                },
                ConstExprKind::Binary { op, left, .. } => {
                    let right = values.pop().expect("right operand");
                    let left_value = values.pop().expect("left operand");
                    const_bin(*op, &left.ty, &left_value, &right).map_err(|error| (error, expr))?
                },
                ConstExprKind::Compare { op, left, .. } => {
                    let right = values.pop().expect("right operand");
                    let left_value = values.pop().expect("left operand");
                    ConstValue::Bool(
                        const_cmp(*op, &left.ty, &left_value, &right)
                            .map_err(|error| (error, expr))?,
                    )
                },
            };
            if !crate::constant::valid_constant(&expr.ty, &value) {
                return Err((ConstEvalError::InvalidLiteral, expr));
            }
            values.push(value);
        }
        Ok(values.pop().expect("root expression result"))
    }
}

pub(crate) fn same_value(left: &ConstValue, right: &ConstValue) -> bool {
    match (left, right) {
        // Compare the stored payload, including signed zero and NaN bits.
        (ConstValue::F(a), ConstValue::F(b)) => a == b,
        _ => left == right,
    }
}

// Preserve the existing scalar constant evaluation rules.
fn ty_int_bits(ty: &Type) -> Option<u32> {
    Some(match ty {
        Type::I1 | Type::U1 => 1,
        Type::I8 | Type::U8 => 8,
        Type::I16 | Type::U16 => 16,
        Type::I32 | Type::U32 => 32,
        Type::I64 | Type::U64 => 64,
        Type::I128 | Type::U128 => 128,
        _ => return None,
    })
}

fn is_signed_int(ty: &Type) -> bool {
    matches!(ty, Type::I1 | Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::I128)
}

fn wrap_u(v: u128, ty: &Type) -> u128 {
    let bits = ty_int_bits(ty).unwrap_or(128);
    if bits >= 128 {
        return v;
    }
    let mask = (1u128 << bits) - 1;
    v & mask
}

fn wrap_i(v: i128, ty: &Type) -> i128 {
    let bits = ty_int_bits(ty).unwrap_or(128);
    if bits >= 128 {
        return v;
    }
    let mask = (1u128 << bits) - 1;
    let u = (v as u128) & mask;
    let sign = 1u128 << (bits - 1);
    if (u & sign) != 0 {
        // sign-extend
        (u | (!mask)) as i128
    } else {
        u as i128
    }
}

fn shift_count(ty: &Type, raw: u128) -> u32 {
    let bits = ty_int_bits(ty).expect("integer expression validated");
    (wrap_u(raw, ty) % u128::from(bits)) as u32
}

fn const_not(ty: &Type, value: &ConstValue) -> Result<ConstValue, ConstEvalError> {
    match value {
        ConstValue::I(v) if is_signed_int(ty) => Ok(ConstValue::I(wrap_i(!v, ty))),
        ConstValue::U(v) if ty_int_bits(ty).is_some() => Ok(ConstValue::U(wrap_u(!v, ty))),
        _ => Err(ConstEvalError::UnsupportedOperation),
    }
}

fn const_bin(
    op: ConstBinaryOp,
    ty: &Type,
    l: &ConstValue,
    r: &ConstValue,
) -> Result<ConstValue, ConstEvalError> {
    // float
    if matches!(ty, Type::F16 | Type::F32 | Type::F64) {
        let (ConstValue::F(a), ConstValue::F(b)) = (l, r) else {
            return Err(ConstEvalError::UnsupportedOperation);
        };
        let width = a.width();
        let (a, b) = (a.to_f64(), b.to_f64());
        let out = match op {
            ConstBinaryOp::Add => a + b,
            ConstBinaryOp::Sub => a - b,
            ConstBinaryOp::Mul => a * b,
            _ => return Err(ConstEvalError::UnsupportedOperation),
        };
        let value = if out.is_nan() {
            match width {
                16 => crate::FloatBits::F16(0x7e00),
                32 => crate::FloatBits::F32(0x7fc00000),
                _ => crate::FloatBits::F64(0x7ff8000000000000),
            }
        } else {
            crate::FloatBits::from_f64(width, out).expect("supported float width")
        };
        return Ok(ConstValue::F(value));
    }

    // int
    if ty_int_bits(ty).is_some() && !matches!(ty, Type::Bool) {
        if is_signed_int(ty) {
            let (ConstValue::I(a), ConstValue::I(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            let raw = match op {
                ConstBinaryOp::Add => a.wrapping_add(*b),
                ConstBinaryOp::Sub => a.wrapping_sub(*b),
                ConstBinaryOp::Mul => a.wrapping_mul(*b),
                ConstBinaryOp::BitAnd => a & b,
                ConstBinaryOp::BitOr => a | b,
                ConstBinaryOp::BitXor => a ^ b,
                ConstBinaryOp::Div if *b == 0 => return Err(ConstEvalError::DivisionByZero),
                ConstBinaryOp::Rem if *b == 0 => return Err(ConstEvalError::RemainderByZero),
                ConstBinaryOp::Div => a.wrapping_div(*b),
                ConstBinaryOp::Rem => a.wrapping_rem(*b),
                ConstBinaryOp::ShiftLeft => a.wrapping_shl(shift_count(ty, *b as u128)),
                ConstBinaryOp::ShiftRight => a.wrapping_shr(shift_count(ty, *b as u128)),
                _ => return Err(ConstEvalError::UnsupportedOperation),
            };
            return Ok(ConstValue::I(wrap_i(raw, ty)));
        } else {
            let (ConstValue::U(a), ConstValue::U(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            let raw = match op {
                ConstBinaryOp::Add => a.wrapping_add(*b),
                ConstBinaryOp::Sub => a.wrapping_sub(*b),
                ConstBinaryOp::Mul => a.wrapping_mul(*b),
                ConstBinaryOp::BitAnd => a & b,
                ConstBinaryOp::BitOr => a | b,
                ConstBinaryOp::BitXor => a ^ b,
                ConstBinaryOp::Div if *b == 0 => return Err(ConstEvalError::DivisionByZero),
                ConstBinaryOp::Rem if *b == 0 => return Err(ConstEvalError::RemainderByZero),
                ConstBinaryOp::Div => a / b,
                ConstBinaryOp::Rem => a % b,
                ConstBinaryOp::ShiftLeft => a.wrapping_shl(shift_count(ty, *b)),
                ConstBinaryOp::ShiftRight => a.wrapping_shr(shift_count(ty, *b)),
                _ => return Err(ConstEvalError::UnsupportedOperation),
            };
            return Ok(ConstValue::U(wrap_u(raw, ty)));
        }
    }

    Err(ConstEvalError::UnsupportedOperation)
}

fn const_cmp(
    op: ConstCompareOp,
    ty: &Type,
    l: &ConstValue,
    r: &ConstValue,
) -> Result<bool, ConstEvalError> {
    // float
    if matches!(ty, Type::F16 | Type::F32 | Type::F64) {
        let (ConstValue::F(a), ConstValue::F(b)) = (l, r) else {
            return Err(ConstEvalError::UnsupportedOperation);
        };
        let (a, b) = (a.to_f64(), b.to_f64());
        return Ok(match op {
            ConstCompareOp::Eq => a == b,
            ConstCompareOp::Ne => a != b,
            ConstCompareOp::Lt => a < b,
            ConstCompareOp::Le => a <= b,
            ConstCompareOp::Gt => a > b,
            ConstCompareOp::Ge => a >= b,
        });
    }

    // bool은 eq/ne만
    if matches!(ty, Type::Bool) {
        let (ConstValue::Bool(a), ConstValue::Bool(b)) = (l, r) else {
            return Err(ConstEvalError::UnsupportedOperation);
        };
        return Ok(match op {
            ConstCompareOp::Eq => a == b,
            ConstCompareOp::Ne => a != b,
            _ => return Err(ConstEvalError::UnsupportedOperation),
        });
    }

    // int
    if ty_int_bits(ty).is_some() && !matches!(ty, Type::Bool) {
        if is_signed_int(ty) {
            let (ConstValue::I(a), ConstValue::I(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            return Ok(match op {
                ConstCompareOp::Eq => a == b,
                ConstCompareOp::Ne => a != b,
                ConstCompareOp::Lt => a < b,
                ConstCompareOp::Le => a <= b,
                ConstCompareOp::Gt => a > b,
                ConstCompareOp::Ge => a >= b,
            });
        } else {
            let (ConstValue::U(a), ConstValue::U(b)) = (l, r) else {
                return Err(ConstEvalError::UnsupportedOperation);
            };
            return Ok(match op {
                ConstCompareOp::Eq => a == b,
                ConstCompareOp::Ne => a != b,
                ConstCompareOp::Lt => a < b,
                ConstCompareOp::Le => a <= b,
                ConstCompareOp::Gt => a > b,
                ConstCompareOp::Ge => a >= b,
            });
        }
    }

    Err(ConstEvalError::UnsupportedOperation)
}
