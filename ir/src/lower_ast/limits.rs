// SPDX-License-Identifier: MPL-2.0
//! Shared, iterative resource inspection for directly constructed frontend ASTs.
use super::frontend::*;

/// Recursive lowering/serde work must stay below this ceiling even with overrides.
/// Thirty-two AST levels also fit serde_json's JSON depth cap, including statement
/// vectors and enum-tag objects that add multiple JSON levels per AST node.
pub const MAX_AST_DEPTH: usize = 32;

/// Bounds are applied before recursive conversion, cloning, or serialization.
#[derive(Clone, Copy, Debug)]
pub struct AstLimits {
    pub max_depth: usize,
    pub max_nodes: usize,
    pub max_string_bytes: usize,
    /// Maximum pretty JSON size; enforced by a bounded serialization writer.
    pub max_output_bytes: usize,
    /// Work units for optional source mapping during O0 construction.
    pub max_source_work: usize,
}

impl Default for AstLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_AST_DEPTH,
            max_nodes: 100_000,
            max_string_bytes: 8 * 1024 * 1024,
            max_output_bytes: 8 * 1024 * 1024,
            max_source_work: 16 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AstLimitError {
    Source,
    SourceWork,
    Depth,
    Nodes,
    Strings,
    Output,
}

impl std::fmt::Display for AstLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Source => "invalid source annotation or duplicate original node identity",
            Self::SourceWork => "AST source mapping work limit exceeded",
            Self::Depth => "AST nesting limit exceeded",
            Self::Nodes => "AST node limit exceeded",
            Self::Strings => "AST string byte limit exceeded",
            Self::Output => "AST JSON exceeds output byte limit",
        })
    }
}

impl std::error::Error for AstLimitError {}

enum Node<'a> {
    Expr(&'a Expr),
    Stmt(&'a Stmt),
    Type(&'a TypeRef),
    Signature(&'a SignatureRef),
}

struct Inspect<'a> {
    limits: AstLimits,
    nodes: usize,
    strings: usize,
    pending: Vec<(Node<'a>, usize)>,
    extended_ops: u8,
    source_ids: std::collections::HashSet<u64>,
    wire_numbers: bool,
}

impl<'a> Inspect<'a> {
    fn source(&mut self, annotation: &crate::SourceAnnotation) -> Result<(), AstLimitError> {
        if !self.source_ids.insert(annotation.id) {
            return Err(AstLimitError::Source);
        }
        self.extended_ops |= 2;
        if let Some(s) = &annotation.source {
            self.string(&s.file)?;
            if s.file.is_empty()
                || s.file.contains('\0')
                || s.line == 0
                || s.column == 0
                || s.end_line == 0
                || s.end_column == 0
                || (s.line, s.column) > (s.end_line, s.end_column)
            {
                return Err(AstLimitError::Source);
            }
        }
        Ok(())
    }

    fn count(&mut self) -> Result<(), AstLimitError> {
        self.nodes = self.nodes.checked_add(1).ok_or(AstLimitError::Nodes)?;
        if self.nodes > self.limits.max_nodes {
            return Err(AstLimitError::Nodes);
        }
        Ok(())
    }

    fn string(&mut self, s: &str) -> Result<(), AstLimitError> {
        self.strings = self.strings.checked_add(s.len()).ok_or(AstLimitError::Strings)?;
        if self.strings > self.limits.max_string_bytes {
            return Err(AstLimitError::Strings);
        }
        Ok(())
    }

    fn push(&mut self, node: Node<'a>, depth: usize) -> Result<(), AstLimitError> {
        if depth > self.limits.max_depth.min(MAX_AST_DEPTH) {
            return Err(AstLimitError::Depth);
        }
        self.count()?;
        self.pending.push((node, depth));
        Ok(())
    }
}

/// Inspect an existing AST without recursively visiting it or cloning its data.
pub fn inspect_program(program: &Program, limits: AstLimits) -> Result<(), AstLimitError> {
    inspect_program_features(program, limits).map(|_| ())
}

pub(crate) fn inspect_program_features(
    program: &Program,
    limits: AstLimits,
) -> Result<u8, AstLimitError> {
    inspect_program_details(program, limits).map(|details| details.0)
}

/// JSON Schema widths and numeric spelling are syntax; value ranges and typed
/// expression compatibility remain lowering/verification responsibilities.
pub(crate) fn inspect_program_wire(program: &Program, limits: AstLimits) -> Result<u8, String> {
    let (extended, numbers) =
        inspect_program_details(program, limits).map_err(|e| e.to_string())?;
    if !numbers {
        return Err("AST numeric width or spelling does not match its versioned schema".into());
    }
    Ok(extended)
}

fn inspect_program_details(
    program: &Program,
    limits: AstLimits,
) -> Result<(u8, bool), AstLimitError> {
    let mut i = Inspect {
        limits,
        nodes: 0,
        strings: 0,
        pending: Vec::new(),
        extended_ops: 0,
        source_ids: std::collections::HashSet::new(),
        wire_numbers: true,
    };
    i.count()?; // program
    for d in &program.declarations {
        i.count()?;
        i.string(&d.name)?;
        if let Some(s) = &d.link_name {
            i.string(s)?;
        }
        i.push(Node::Signature(&d.signature), 1)?;
    }
    for g in &program.globals {
        i.count()?;
        i.string(&g.name)?;
        i.push(Node::Type(&g.ty), 1)?;
        i.push(Node::Expr(&g.init), 1)?;
    }
    for f in &program.functions {
        i.count()?;
        i.string(&f.name)?;
        if let Some(s) = &f.link_name {
            i.string(s)?;
        }
        i.push(Node::Type(&f.return_type), 1)?;
        for p in &f.parameters {
            i.count()?;
            i.string(&p.name)?;
            i.push(Node::Type(&p.ty), 1)?;
        }
        for s in &f.body {
            i.push(Node::Stmt(s), 1)?;
        }
    }
    while let Some((node, depth)) = i.pending.pop() {
        let next = depth + 1;
        match node {
            Node::Signature(s) => {
                i.push(Node::Type(&s.ret), next)?;
                for t in &s.params {
                    i.push(Node::Type(t), next)?;
                }
            },
            Node::Type(t) => match t {
                TypeRef::Int { bits, .. } => i.wire_numbers &= valid_int_width(*bits),
                TypeRef::Float { bits } => i.wire_numbers &= matches!(bits, 16 | 32 | 64),
                TypeRef::Ptr(t) | TypeRef::Array { elem: t, .. } => i.push(Node::Type(t), next)?,
                TypeRef::FnPtr(s) => i.push(Node::Signature(s), next)?,
                TypeRef::Opaque(s) => i.string(s)?,
                _ => {},
            },
            Node::Expr(e) => match e {
                Expr::Located { annotation, expression } => {
                    i.source(annotation)?;
                    i.push(Node::Expr(expression), next)?;
                },
                Expr::BitNot(value) => {
                    i.extended_ops |= 1;
                    i.push(Node::Expr(value), next)?;
                },
                Expr::FunctionRef(s) | Expr::Var(s) => i.string(s)?,
                Expr::NullFunction(s) => i.push(Node::Signature(s), next)?,
                Expr::Call { callee, args } => {
                    match callee {
                        CalleeRef::Direct(s) => i.string(s)?,
                        CalleeRef::Indirect(e) => i.push(Node::Expr(e), next)?,
                    }
                    for e in args {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Expr::Lit(Lit::Int { bits, value, .. }) => {
                    i.string(value)?;
                    let digits = value.strip_prefix('-').unwrap_or(value);
                    i.wire_numbers &= valid_int_width(*bits)
                        && !digits.is_empty()
                        && digits.bytes().all(|b| b.is_ascii_digit());
                },
                Expr::Lit(Lit::Float { bits, value }) => {
                    i.string(value)?;
                    i.wire_numbers &= matches!(bits, 16 | 32 | 64)
                        && value.len() == 2 + usize::from(*bits) / 4
                        && value.starts_with("0x")
                        && value[2..].bytes().all(|b| b.is_ascii_hexdigit());
                },
                Expr::Lit(Lit::Bool(_)) => {},
                Expr::Binary { left, op, right } => {
                    if !matches!(op, BinOpRef::Add | BinOpRef::Sub | BinOpRef::Mul) {
                        i.extended_ops |= 1;
                    }
                    i.push(Node::Expr(left), next)?;
                    i.push(Node::Expr(right), next)?;
                },
                Expr::Cmp { left, right, .. } => {
                    i.push(Node::Expr(left), next)?;
                    i.push(Node::Expr(right), next)?;
                },
            },
            Node::Stmt(s) => match s {
                Stmt::Located { annotation, statement } => {
                    i.source(annotation)?;
                    i.push(Node::Stmt(statement), next)?;
                },
                Stmt::Return(e) => {
                    if let Some(e) = e {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Stmt::ConstDecl { name, ty, init } => {
                    i.string(name)?;
                    i.push(Node::Type(ty), next)?;
                    i.push(Node::Expr(init), next)?;
                },
                Stmt::VarDecl { name, ty, init } => {
                    i.string(name)?;
                    i.push(Node::Type(ty), next)?;
                    if let Some(e) = init {
                        i.push(Node::Expr(e), next)?;
                    }
                },
                Stmt::Assign { name, value } => {
                    i.string(name)?;
                    i.push(Node::Expr(value), next)?;
                },
                Stmt::ExprStmt(e) => i.push(Node::Expr(e), next)?,
                Stmt::If { cond, then_body, else_body } => {
                    i.push(Node::Expr(cond), next)?;
                    for s in then_body.iter().chain(else_body) {
                        i.push(Node::Stmt(s), next)?;
                    }
                },
                Stmt::While { cond, body } => {
                    i.push(Node::Expr(cond), next)?;
                    for s in body {
                        i.push(Node::Stmt(s), next)?;
                    }
                },
                Stmt::Break | Stmt::Continue => {},
            },
        }
    }
    Ok((i.extended_ops, i.wire_numbers))
}

enum Owned {
    Expr(Expr),
    Stmt(Stmt),
    Type(TypeRef),
    Signature(SignatureRef),
}

enum Disposal {
    One(Owned),
    Types(std::vec::IntoIter<TypeRef>),
    Exprs(std::vec::IntoIter<Expr>),
    Stmts(std::vec::IntoIter<Stmt>),
}

/// Dispose of an owned rejected tree without its recursively derived destructor.
/// Owning iterators reuse the tree's vectors, keeping extra storage bounded by
/// depth rather than copying all siblings of a wide tree into another allocation.
pub(crate) fn discard_program(program: Program) {
    let mut pending = Vec::new();
    for d in program.declarations {
        discard(Owned::Signature(d.signature), &mut pending);
    }
    for g in program.globals {
        discard(Owned::Type(g.ty), &mut pending);
        discard(Owned::Expr(g.init), &mut pending);
    }
    for f in program.functions {
        discard(Owned::Type(f.return_type), &mut pending);
        for p in f.parameters {
            discard(Owned::Type(p.ty), &mut pending);
        }
        for s in f.body {
            discard(Owned::Stmt(s), &mut pending);
        }
    }
}

fn discard(root: Owned, pending: &mut Vec<Disposal>) {
    pending.push(Disposal::One(root));
    while let Some(work) = pending.pop() {
        let node = match work {
            Disposal::One(node) => node,
            Disposal::Types(mut iter) => {
                let Some(node) = iter.next() else { continue };
                pending.push(Disposal::Types(iter));
                Owned::Type(node)
            },
            Disposal::Exprs(mut iter) => {
                let Some(node) = iter.next() else { continue };
                pending.push(Disposal::Exprs(iter));
                Owned::Expr(node)
            },
            Disposal::Stmts(mut iter) => {
                let Some(node) = iter.next() else { continue };
                pending.push(Disposal::Stmts(iter));
                Owned::Stmt(node)
            },
        };
        match node {
            Owned::Type(t) => match t {
                TypeRef::Ptr(t) | TypeRef::Array { elem: t, .. } => {
                    pending.push(Disposal::One(Owned::Type(*t)))
                },
                TypeRef::FnPtr(s) => pending.push(Disposal::One(Owned::Signature(*s))),
                _ => {},
            },
            Owned::Signature(s) => {
                pending.push(Disposal::One(Owned::Type(s.ret)));
                pending.push(Disposal::Types(s.params.into_iter()));
            },
            Owned::Expr(e) => match e {
                Expr::Located { expression, .. } => {
                    pending.push(Disposal::One(Owned::Expr(*expression)))
                },
                Expr::BitNot(value) => pending.push(Disposal::One(Owned::Expr(*value))),
                Expr::NullFunction(s) => pending.push(Disposal::One(Owned::Signature(s))),
                Expr::Binary { left, right, .. } | Expr::Cmp { left, right, .. } => {
                    pending.push(Disposal::One(Owned::Expr(*right)));
                    pending.push(Disposal::One(Owned::Expr(*left)));
                },
                Expr::Call { callee, args } => {
                    if let CalleeRef::Indirect(e) = callee {
                        pending.push(Disposal::One(Owned::Expr(*e)));
                    }
                    pending.push(Disposal::Exprs(args.into_iter()));
                },
                _ => {},
            },
            Owned::Stmt(s) => match s {
                Stmt::Located { statement, .. } => {
                    pending.push(Disposal::One(Owned::Stmt(*statement)))
                },
                Stmt::Return(e) => {
                    if let Some(e) = e {
                        pending.push(Disposal::One(Owned::Expr(e)));
                    }
                },
                Stmt::ConstDecl { ty, init, .. } => {
                    pending.push(Disposal::One(Owned::Type(ty)));
                    pending.push(Disposal::One(Owned::Expr(init)));
                },
                Stmt::VarDecl { ty, init, .. } => {
                    pending.push(Disposal::One(Owned::Type(ty)));
                    if let Some(e) = init {
                        pending.push(Disposal::One(Owned::Expr(e)));
                    }
                },
                Stmt::Assign { value, .. } | Stmt::ExprStmt(value) => {
                    pending.push(Disposal::One(Owned::Expr(value)))
                },
                Stmt::If { cond, then_body, else_body } => {
                    pending.push(Disposal::One(Owned::Expr(cond)));
                    pending.push(Disposal::Stmts(then_body.into_iter()));
                    pending.push(Disposal::Stmts(else_body.into_iter()));
                },
                Stmt::While { cond, body } => {
                    pending.push(Disposal::One(Owned::Expr(cond)));
                    pending.push(Disposal::Stmts(body.into_iter()));
                },
                Stmt::Break | Stmt::Continue => {},
            },
        }
    }
}

fn valid_int_width(bits: u16) -> bool {
    matches!(bits, 1 | 8 | 16 | 32 | 64 | 128)
}
