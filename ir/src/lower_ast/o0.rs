// SPDX-License-Identifier: MPL-2.0

use std::collections::{HashMap, HashSet};

use crate::{
    BlockId, ConstBinaryOp, ConstCompareOp, ConstExpr, ConstExprKind, ConstRef, ConstValue,
    DataLayout, GlobalId, Module, ModuleBuilder, Type, ValueId,
};

use super::{
    binding::Binding,
    frontend,
    support::{align_of, map_binop, socket_type_to_whale},
    LowerError,
};

#[derive(Clone, Copy, Debug)]
struct LoopCtx {
    cond_bb: BlockId,
    exit_bb: BlockId,
}

type ConstMap = HashMap<String, (GlobalId, Type, ConstValue)>;

pub fn lower_o0(
    program: &frontend::Program,
    target: &str,
    datalayout: DataLayout,
) -> Result<Module, LowerError> {
    let ptr_bits = datalayout.ptr_bits;
    // Functions and globals have separate namespaces. Validate each before bodies.
    let mut functions = HashSet::new();
    for f in &program.functions {
        if !functions.insert(&f.name) {
            return Err(LowerError::DuplicateFunction(f.name.clone()));
        }
        let mut parameters = HashSet::new();
        for p in &f.parameters {
            if !parameters.insert(&p.name) {
                return Err(LowerError::DuplicateParameter {
                    func: f.name.clone(),
                    param: p.name.clone(),
                });
            }
        }
    }
    let mut mb = ModuleBuilder::new(target, datalayout);

    let mut gconsts: ConstMap = HashMap::new();
    for g in &program.globals {
        if gconsts.contains_key(&g.name) {
            return Err(LowerError::DuplicateGlobal(g.name.clone()));
        }

        let decl_ty = socket_type_to_whale(&g.ty)?;
        let (expression, v) = eval_const_expr(
            &g.init,
            &ConstEvalCtx {
                local_consts: None,
                global_consts: &gconsts,
            },
        )?;

        let vty = expression.ty.clone();
        if vty != decl_ty {
            return Err(LowerError::TypeMismatch {
                expected: decl_ty,
                got: vty,
            });
        }

        let align = align_of(&vty, ptr_bits);
        let id = mb.add_global_const(g.name.clone(), expression, v.clone(), align);
        gconsts.insert(g.name.clone(), (id, vty, v));
    }

    for f in &program.functions {
        lower_function_o0(&mut mb, f, &gconsts, ptr_bits)?;
    }

    Ok(mb.finish())
}

fn lower_function_o0(
    mb: &mut ModuleBuilder,
    f: &frontend::Function,
    global_consts: &ConstMap,
    ptr_bits: u32,
) -> Result<(), LowerError> {
    let ret_ty = socket_type_to_whale(&f.return_type)?;

    let params: Vec<(String, Type)> = f
        .parameters
        .iter()
        .map(|p| Ok((p.name.clone(), socket_type_to_whale(&p.ty)?)))
        .collect::<Result<_, LowerError>>()?;

    let mut fb = mb.begin_function(f.name.clone(), params, ret_ty.clone());

    let mut env: HashMap<String, Binding> = HashMap::new();
    let mut loop_stack: Vec<LoopCtx> = Vec::new();

    // params -> stack slot (-O0)
    for (i, p) in f.parameters.iter().enumerate() {
        let param_val = fb.param_value(i);
        let ty = socket_type_to_whale(&p.ty)?;
        let align = align_of(&ty, ptr_bits);

        let slot = fb.alloca_in_entry(ty.clone(), align);
        fb.store(ty.clone(), param_val, slot, align);

        env.insert(p.name.clone(), Binding::Addr { ptr: slot, ty });
    }

    for s in &f.body {
        lower_stmt_o0(
            &mut fb,
            &mut env,
            global_consts,
            &mut loop_stack,
            s,
            &ret_ty,
            ptr_bits,
        )?;
    }

    if !fb.is_current_block_terminated() {
        if ret_ty == Type::Void {
            fb.ret(None);
        } else {
            fb.trap("missing return");
        }
    }

    fb.finish();
    Ok(())
}

fn lower_stmt_o0(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    global_consts: &ConstMap,
    loop_stack: &mut Vec<LoopCtx>,
    stmt: &frontend::Stmt,
    func_ret_ty: &Type,
    ptr_bits: u32,
) -> Result<(), LowerError> {
    if fb.is_current_block_terminated() {
        // Keep source statements after return/break/continue visible at O0.
        // A new block has no incoming edge from the terminated block, so the
        // preserved statements cannot change the original execution path.
        let dead = fb.create_block("unreachable.cont");
        fb.set_insert_point(dead);
    }

    match stmt {
        frontend::Stmt::Return(opt) => {
            if *func_ret_ty == Type::Void {
                if opt.is_some() {
                    return Err(LowerError::ValueReturnedFromVoid);
                }
                fb.ret(None);
                return Ok(());
            }

            let e = opt.as_ref().ok_or(LowerError::UnsupportedStmt)?;
            let (v, ty) = lower_expr_o0(fb, env, global_consts, e, ptr_bits)?;

            if ty != *func_ret_ty {
                return Err(LowerError::TypeMismatch {
                    expected: func_ret_ty.clone(),
                    got: ty,
                });
            }

            fb.ret(Some(v));
            Ok(())
        }

        frontend::Stmt::ConstDecl { name, ty, init } => {
            let decl_ty = socket_type_to_whale(ty)?;

            let (expression, cv) = eval_const_expr(
                init,
                &ConstEvalCtx {
                    local_consts: Some(env),
                    global_consts,
                },
            )?;

            let cty = expression.ty.clone();
            if cty != decl_ty {
                return Err(LowerError::TypeMismatch {
                    expected: decl_ty,
                    got: cty,
                });
            }

            let id = fb.const_decl(name.clone(), expression, cv.clone());
            env.insert(
                name.clone(),
                Binding::Const {
                    id,
                    ty: cty,
                    value: cv,
                },
            );
            Ok(())
        }

        frontend::Stmt::VarDecl { name, ty, init } => {
            let decl_ty = socket_type_to_whale(ty)?;
            let align = align_of(&decl_ty, ptr_bits);

            let (v, vty) = if let Some(init_expr) = init.as_ref() {
                lower_expr_o0(fb, env, global_consts, init_expr, ptr_bits)?
            } else {
                let v = fb.undef(decl_ty.clone());
                (v, decl_ty.clone())
            };

            if vty != decl_ty {
                return Err(LowerError::TypeMismatch {
                    expected: decl_ty,
                    got: vty,
                });
            }

            let slot = fb.alloca_in_entry(decl_ty.clone(), align);
            fb.store(decl_ty.clone(), v, slot, align);

            env.insert(
                name.clone(),
                Binding::Addr {
                    ptr: slot,
                    ty: decl_ty,
                },
            );
            Ok(())
        }

        frontend::Stmt::Assign { name, value } => {
            let (v, vty) = lower_expr_o0(fb, env, global_consts, value, ptr_bits)?;

            let b = env
                .get(name)
                .cloned()
                .ok_or_else(|| LowerError::UnknownVariable(name.clone()))?;

            match b {
                Binding::Const { .. } => Err(LowerError::AssignToConst(name.clone())),
                Binding::Addr { ptr, ty } => {
                    if vty != ty {
                        return Err(LowerError::TypeMismatch {
                            expected: ty,
                            got: vty,
                        });
                    }
                    let align = align_of(&ty, ptr_bits);
                    fb.store(ty, v, ptr, align);
                    Ok(())
                }
            }
        }

        frontend::Stmt::ExprStmt(e) => {
            let _ = lower_expr_o0(fb, env, global_consts, e, ptr_bits)?;
            Ok(())
        }

        frontend::Stmt::If {
            cond,
            then_body,
            else_body,
        } => {
            let (cv, cty) = lower_expr_o0(fb, env, global_consts, cond, ptr_bits)?;
            if cty != Type::Bool {
                return Err(LowerError::TypeMismatch {
                    expected: Type::Bool,
                    got: cty,
                });
            }

            let then_bb = fb.create_block("if.then");
            let else_bb = fb.create_block("if.else");
            let cont_bb = fb.create_block("if.cont");

            fb.cbr(cv, then_bb, else_bb);

            // then
            fb.set_insert_point(then_bb);
            let mut then_env = env.clone();
            for s in then_body {
                lower_stmt_o0(
                    fb,
                    &mut then_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    ptr_bits,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cont_bb);
            }

            // else
            fb.set_insert_point(else_bb);
            let mut else_env = env.clone();
            for s in else_body {
                lower_stmt_o0(
                    fb,
                    &mut else_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    ptr_bits,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cont_bb);
            }

            // cont
            fb.set_insert_point(cont_bb);
            Ok(())
        }

        frontend::Stmt::While { cond, body } => {
            let cond_bb = fb.create_block("while.cond");
            let body_bb = fb.create_block("while.body");
            let exit_bb = fb.create_block("while.exit");

            fb.br(cond_bb);

            // cond
            fb.set_insert_point(cond_bb);
            let (cv, cty) = lower_expr_o0(fb, env, global_consts, cond, ptr_bits)?;
            if cty != Type::Bool {
                return Err(LowerError::TypeMismatch {
                    expected: Type::Bool,
                    got: cty,
                });
            }
            fb.cbr(cv, body_bb, exit_bb);

            fb.set_insert_point(body_bb);

            loop_stack.push(LoopCtx { cond_bb, exit_bb });

            let mut body_env = env.clone();
            for s in body {
                lower_stmt_o0(
                    fb,
                    &mut body_env,
                    global_consts,
                    loop_stack,
                    s,
                    func_ret_ty,
                    ptr_bits,
                )?;
            }
            if !fb.is_current_block_terminated() {
                fb.br(cond_bb);
            }

            loop_stack.pop();

            // exit
            fb.set_insert_point(exit_bb);
            Ok(())
        }

        frontend::Stmt::Break => {
            let ctx = loop_stack
                .last()
                .cloned()
                .ok_or(LowerError::BreakOutsideLoop)?;
            fb.br(ctx.exit_bb);
            Ok(())
        }

        frontend::Stmt::Continue => {
            let ctx = loop_stack
                .last()
                .cloned()
                .ok_or(LowerError::ContinueOutsideLoop)?;
            fb.br(ctx.cond_bb);
            Ok(())
        }
    }
}

fn lower_expr_o0(
    fb: &mut crate::FunctionBuilder<'_>,
    env: &mut HashMap<String, Binding>,
    global_consts: &ConstMap,
    expr: &frontend::Expr,
    ptr_bits: u32,
) -> Result<(ValueId, Type), LowerError> {
    match expr {
        frontend::Expr::Var(name) => {
            if let Some(b) = env.get(name).cloned() {
                return match b {
                    Binding::Addr { ptr, ty } => {
                        let align = align_of(&ty, ptr_bits);
                        let v = fb.load(ty.clone(), ptr, align);
                        Ok((v, ty))
                    }
                    Binding::Const { id, ty, .. } => Ok((id, ty)),
                };
            }

            if let Some((_, ty, value)) = global_consts.get(name).cloned() {
                let v = emit_const_value(fb, &ty, &value);
                return Ok((v, ty));
            }

            Err(LowerError::UnknownVariable(name.clone()))
        }

        frontend::Expr::Lit(lit) => lower_lit_o0(fb, lit),

        frontend::Expr::Binary { left, op, right } => {
            let (lv, lty) = lower_expr_o0(fb, env, global_consts, left, ptr_bits)?;
            let (rv, rty) = lower_expr_o0(fb, env, global_consts, right, ptr_bits)?;

            if lty != rty {
                return Err(LowerError::TypeMismatch {
                    expected: lty,
                    got: rty,
                });
            }

            let binop = map_binop(*op, &lty)?;
            let out = fb.bin(binop, lty.clone(), lv, rv);
            Ok((out, lty))
        }

        frontend::Expr::Cmp { left, op, right } => {
            let (lv, lty) = lower_expr_o0(fb, env, global_consts, left, ptr_bits)?;
            let (rv, rty) = lower_expr_o0(fb, env, global_consts, right, ptr_bits)?;

            if lty != rty {
                return Err(LowerError::TypeMismatch {
                    expected: lty,
                    got: rty,
                });
            }

            let cmp = super::support::map_cmp(*op, &lty)?;
            let out = fb.cmp(cmp, lty.clone(), lv, rv);
            Ok((out, Type::Bool))
        }
    }
}

fn emit_const_value(fb: &mut crate::FunctionBuilder<'_>, ty: &Type, v: &ConstValue) -> ValueId {
    match v {
        ConstValue::Bool(b) => fb.const_bool(*b),
        ConstValue::I(i) => fb.const_int(ty.clone(), *i),
        ConstValue::U(u) => fb.const_uint(ty.clone(), *u),
        ConstValue::F(x) => fb.const_float(ty.clone(), *x),
    }
}

// -------------------------
// const expr evaluator
// -------------------------
struct ConstEvalCtx<'a> {
    local_consts: Option<&'a HashMap<String, Binding>>,
    global_consts: &'a ConstMap,
}

fn eval_const_expr(
    expr: &frontend::Expr,
    ctx: &ConstEvalCtx<'_>,
) -> Result<(ConstExpr, ConstValue), LowerError> {
    let expression = preserve_const_expr(expr, ctx)?;
    let value = expression
        .evaluate(&|reference| match reference {
            ConstRef::Global(id) => ctx
                .global_consts
                .values()
                .find(|(other, _, _)| *other == id)
                .map(|(_, ty, value)| (ty.clone(), value.clone())),
            ConstRef::Local(id) => ctx.local_consts.and_then(|env| {
                env.values().find_map(|binding| match binding {
                    Binding::Const {
                        id: other,
                        ty,
                        value,
                    } if *other == id => Some((ty.clone(), value.clone())),
                    _ => None,
                })
            }),
        })
        .map_err(|_| LowerError::UnsupportedExpr)?;
    Ok((expression, value))
}

fn preserve_const_expr(
    expr: &frontend::Expr,
    ctx: &ConstEvalCtx<'_>,
) -> Result<ConstExpr, LowerError> {
    match expr {
        frontend::Expr::Lit(literal) => {
            let (ty, value) = lit_to_const(literal)?;
            Ok(ConstExpr::literal(ty, value))
        }
        frontend::Expr::Var(name) => {
            if let Some(binding) = ctx.local_consts.and_then(|env| env.get(name)) {
                return match binding {
                    Binding::Const { id, ty, .. } => Ok(ConstExpr {
                        ty: ty.clone(),
                        kind: ConstExprKind::Reference(ConstRef::Local(*id)),
                    }),
                    Binding::Addr { .. } => Err(LowerError::NonConstExpr),
                };
            }
            let (id, ty, _) = ctx
                .global_consts
                .get(name)
                .ok_or_else(|| LowerError::UnknownVariable(name.clone()))?;
            Ok(ConstExpr {
                ty: ty.clone(),
                kind: ConstExprKind::Reference(ConstRef::Global(*id)),
            })
        }
        frontend::Expr::Binary { left, op, right } => {
            let left = Box::new(preserve_const_expr(left, ctx)?);
            let right = Box::new(preserve_const_expr(right, ctx)?);
            if left.ty != right.ty {
                return Err(LowerError::TypeMismatch {
                    expected: left.ty,
                    got: right.ty,
                });
            }
            let op = match op {
                frontend::BinOpRef::Add => ConstBinaryOp::Add,
                frontend::BinOpRef::Sub => ConstBinaryOp::Sub,
                frontend::BinOpRef::Mul => ConstBinaryOp::Mul,
            };
            Ok(ConstExpr {
                ty: left.ty.clone(),
                kind: ConstExprKind::Binary { op, left, right },
            })
        }
        frontend::Expr::Cmp { left, op, right } => {
            let left = Box::new(preserve_const_expr(left, ctx)?);
            let right = Box::new(preserve_const_expr(right, ctx)?);
            if left.ty != right.ty {
                return Err(LowerError::TypeMismatch {
                    expected: left.ty,
                    got: right.ty,
                });
            }
            let op = match op {
                frontend::CmpOpRef::Eq => ConstCompareOp::Eq,
                frontend::CmpOpRef::Ne => ConstCompareOp::Ne,
                frontend::CmpOpRef::Lt => ConstCompareOp::Lt,
                frontend::CmpOpRef::Le => ConstCompareOp::Le,
                frontend::CmpOpRef::Gt => ConstCompareOp::Gt,
                frontend::CmpOpRef::Ge => ConstCompareOp::Ge,
            };
            Ok(ConstExpr {
                ty: Type::Bool,
                kind: ConstExprKind::Compare { op, left, right },
            })
        }
    }
}

fn lit_to_const(l: &frontend::Lit) -> Result<(Type, ConstValue), LowerError> {
    Ok(match l {
        frontend::Lit::Bool(b) => (Type::Bool, ConstValue::Bool(*b)),
        frontend::Lit::Int {
            bits,
            signed,
            value,
        } => {
            let ty = super::support::int_type(*bits, *signed)?;
            let payload = if *signed {
                ConstValue::I(*value)
            } else {
                if *value < 0 {
                    return Err(LowerError::InvalidLiteral {
                        ty,
                        value: ConstValue::I(*value),
                    });
                }
                ConstValue::U(*value as u128)
            };
            if !crate::constant::valid_constant(&ty, &payload) {
                return Err(LowerError::InvalidLiteral { ty, value: payload });
            }
            (ty, payload)
        }
        frontend::Lit::Float { bits, value } => {
            let ty = super::support::float_type(*bits)?;
            (ty, ConstValue::F(*value))
        }
    })
}

fn lower_lit_o0(
    fb: &mut crate::FunctionBuilder<'_>,
    lit: &frontend::Lit,
) -> Result<(ValueId, Type), LowerError> {
    let (ty, value) = lit_to_const(lit)?;
    let id = match value {
        ConstValue::Bool(v) => fb.const_bool(v),
        ConstValue::I(v) => fb.const_int(ty.clone(), v),
        ConstValue::U(v) => fb.const_uint(ty.clone(), v),
        ConstValue::F(v) => fb.const_float(ty.clone(), v),
    };
    Ok((id, ty))
}
