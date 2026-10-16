use std::collections::{HashMap, HashSet, VecDeque};

use super::VerifyError;

use crate::{ConstEvalError, ConstExpr, ConstRef, ConstValue, Function, Instruction, Module, Type};

type Values = HashMap<ConstRef, (Type, ConstValue)>;

fn lookup<'a>(
    locals: &'a Values,
    globals: &'a Values,
    reference: ConstRef,
) -> Option<&'a (Type, ConstValue)> {
    locals.get(&reference).or_else(|| globals.get(&reference))
}

struct Definition<'a> {
    id: ConstRef,
    ty: &'a Type,
    expression: &'a ConstExpr,
    result: &'a ConstValue,
}

fn evaluate(
    scope: &str,
    definitions: Vec<Definition<'_>>,
    mut values: Values,
    globals: &Values,
    limits: crate::IrLimits,
) -> Result<Values, VerifyError> {
    let error = |declaration, reason| VerifyError::InvalidConstExpression {
        scope: scope.into(),
        declaration,
        reason,
    };
    let mut indices = HashMap::new();
    for (i, definition) in definitions.iter().enumerate() {
        if indices.insert(definition.id, i).is_some()
            || lookup(&values, globals, definition.id).is_some()
        {
            return Err(error(definition.id, ConstEvalError::DuplicateDefinition(definition.id)));
        }
        if definition.ty != &definition.expression.ty {
            return Err(error(definition.id, ConstEvalError::TypeMismatch));
        }
    }
    let mut remaining = vec![0; definitions.len()];
    let mut users = vec![Vec::new(); definitions.len()];
    let mut ready = VecDeque::new();
    for (i, definition) in definitions.iter().enumerate() {
        let mut seen = HashSet::new();
        for reference in definition.expression.references() {
            if !seen.insert(reference) || lookup(&values, globals, reference).is_some() {
                continue;
            }
            let &dependency = indices
                .get(&reference)
                .ok_or_else(|| error(definition.id, ConstEvalError::UnknownReference(reference)))?;
            remaining[i] += 1;
            users[dependency].push(i);
        }
        if remaining[i] == 0 {
            ready.push_back(i);
        }
    }
    let mut completed = vec![false; definitions.len()];
    while let Some(i) = ready.pop_front() {
        let definition = &definitions[i];
        let result = definition
            .expression
            .evaluate_with_limits(&|reference| lookup(&values, globals, reference).cloned(), limits)
            .map_err(|reason| error(definition.id, reason))?;
        if !crate::const_expr::same_value(&result, definition.result) {
            return Err(error(definition.id, ConstEvalError::ResultMismatch));
        }
        values.insert(definition.id, (definition.ty.clone(), result));
        completed[i] = true;
        for &user in &users[i] {
            remaining[user] -= 1;
            if remaining[user] == 0 {
                ready.push_back(user);
            }
        }
    }
    if let Some(i) = completed.iter().position(|done| !done) {
        return Err(error(definitions[i].id, ConstEvalError::CyclicReference));
    }
    Ok(values)
}

pub(super) fn verify_globals(m: &Module, limits: crate::IrLimits) -> Result<Values, VerifyError> {
    evaluate(
        "module",
        m.globals
            .iter()
            .map(|g| Definition {
                id: ConstRef::Global(g.id),
                ty: &g.ty,
                expression: &g.init_expr,
                result: &g.init,
            })
            .collect(),
        Values::new(),
        &Values::new(),
        limits,
    )
}

pub(super) fn verify_locals(
    f: &Function,
    globals: &Values,
    limits: crate::IrLimits,
) -> Result<(), VerifyError> {
    let mut values = Values::new();
    let mut definitions = Vec::new();
    for ins in f.blocks.iter().flat_map(|b| &b.instructions) {
        match ins {
            Instruction::Const { dst, ty, value } => {
                values.insert(ConstRef::Local(*dst), (ty.clone(), value.clone()));
            },
            Instruction::ConstDecl { dst, expression, value, .. } => definitions.push(Definition {
                id: ConstRef::Local(*dst),
                ty: &expression.ty,
                expression,
                result: value,
            }),
            _ => {},
        }
    }
    evaluate(&f.name, definitions, values, globals, limits).map(|_| ())
}

#[cfg(test)]
mod sharing_tests {
    use super::*;

    #[test]
    fn overlay_borrows_the_verified_global_entry() {
        let id = ConstRef::Global(crate::GlobalId(7));
        let globals = Values::from([(id, (Type::I32, ConstValue::I(42)))]);
        let mut locals = Values::new();
        locals.insert(ConstRef::Local(crate::ValueId(7)), (Type::I32, ConstValue::I(9)));
        assert!(std::ptr::eq(lookup(&locals, &globals, id).unwrap(), globals.get(&id).unwrap()));
        assert_eq!(
            lookup(&locals, &globals, ConstRef::Local(crate::ValueId(7))).unwrap().1,
            ConstValue::I(9)
        );
    }
}
