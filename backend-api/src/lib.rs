// SPDX-License-Identifier: MPL-2.0
//! Statically linked, out-of-tree code generation. This is a Rust API, not a dylib ABI.
use diagnostic::{Context, Diagnostic, ToDiagnostic};
use std::collections::BTreeSet;
use target_spec::TargetSpec;

pub enum Requirement<'a> {
    Type(&'a ir::Type),
    Declaration(&'a ir::FunctionDecl),
    Instruction(&'a ir::Instruction),
    Terminator(&'a ir::Terminator),
    Global(&'a ir::Global),
    GlobalStorage(&'a ir::GlobalStorage),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
    Direct,
    Expanded,
    Runtime(String),
    Unsupported(String),
}

pub struct Artifact {
    /// Relative portable path. Validated before returning artifacts to callers.
    pub name: String,
    pub bytes: Vec<u8>,
}

/// Only the shared checker can construct this proof of verified, supported input.
pub struct CheckedModule<'a> {
    module: &'a ir::Module,
    target: &'a TargetSpec,
    runtimes: BTreeSet<String>,
}

impl CheckedModule<'_> {
    pub fn module(&self) -> &ir::Module {
        self.module
    }
    pub fn target(&self) -> &TargetSpec {
        self.target
    }
    pub fn runtimes(&self) -> &BTreeSet<String> {
        &self.runtimes
    }
}

pub trait Backend: Send + Sync {
    fn target_id(&self) -> &str;
    fn supports_versions(&self, format: u32, semantics: u32) -> bool {
        format == ir::IR_FORMAT_VERSION && semantics == ir::SEMANTICS_VERSION
    }
    /// Must classify every requirement, including signatures and memory effects.
    fn support(&self, requirement: Requirement<'_>, target: &TargetSpec) -> Support;
    /// May return extra legalization or runtime-availability diagnostics.
    fn validate(&self, _module: &CheckedModule<'_>) -> Result<(), Vec<Diagnostic>> {
        Ok(())
    }
    fn emit(&self, module: &CheckedModule<'_>) -> Result<Vec<Artifact>, Vec<Diagnostic>>;
}

pub fn check<'a>(
    backend: &dyn Backend,
    module: &'a ir::Module,
    target: &'a TargetSpec,
    limits: ir::IrLimits,
) -> Result<CheckedModule<'a>, Vec<Diagnostic>> {
    if backend.target_id() != target.id
        || !backend.supports_versions(ir::IR_FORMAT_VERSION, ir::SEMANTICS_VERSION)
    {
        return Err(vec![Diagnostic::new(
            "backend",
            "check",
            "backend.contract",
            "backend target or IR version mismatch",
        )]);
    }
    ir::verify_module_for_target(module, target, limits).map_err(|e| vec![e.diagnostic()])?;
    let mut errors = Vec::new();
    let mut runtimes = BTreeSet::new();
    let mut inspect =
        |requirement, contexts: Vec<Context>| match backend.support(requirement, target) {
            Support::Direct | Support::Expanded => {},
            Support::Runtime(name) if !name.is_empty() => {
                runtimes.insert(name);
            },
            Support::Runtime(_) => errors.push(Diagnostic::new(
                "backend",
                "check",
                "backend.runtime",
                "runtime identity must be nonempty",
            )),
            Support::Unsupported(reason) => {
                let mut d = Diagnostic::new("backend", "check", "backend.unsupported", reason);
                d.contexts = contexts;
                errors.push(d);
            },
        };
    for global in &module.globals {
        inspect(Requirement::Global(global), vec![]);
        inspect(Requirement::Type(&global.ty), vec![]);
    }
    for global in &module.global_storage {
        inspect(Requirement::GlobalStorage(global), vec![]);
        inspect(Requirement::Type(&global.ty), vec![]);
    }
    for declaration in &module.declarations {
        inspect(Requirement::Declaration(declaration), vec![Context::Function(declaration.id.0)]);
        for ty in
            declaration.signature.params.iter().chain(std::iter::once(&declaration.signature.ret))
        {
            inspect(Requirement::Type(ty), vec![Context::Function(declaration.id.0)]);
        }
    }
    for function in &module.functions {
        let site = vec![Context::Function(function.id.0)];
        inspect(Requirement::Type(&function.ret_ty), site.clone());
        for param in &function.params {
            inspect(Requirement::Type(&param.ty), site.clone());
        }
        for (_, ty) in &function.value_types {
            inspect(Requirement::Type(ty), site.clone());
        }
        for block in &function.blocks {
            for (index, instruction) in block.instructions.iter().enumerate() {
                inspect(
                    Requirement::Instruction(instruction),
                    vec![
                        Context::Function(function.id.0),
                        Context::Block(block.id.0),
                        Context::Instruction(index),
                    ],
                );
            }
            inspect(
                Requirement::Terminator(block.terminator.as_ref().expect("verified terminator")),
                vec![Context::Function(function.id.0), Context::Block(block.id.0)],
            );
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let checked = CheckedModule { module, target, runtimes };
    backend.validate(&checked)?;
    Ok(checked)
}

pub fn compile(
    backend: &dyn Backend,
    module: &ir::Module,
    target: &TargetSpec,
    limits: ir::IrLimits,
) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
    let checked = check(backend, module, target, limits)?;
    let artifacts = backend.emit(&checked)?;
    let mut names = BTreeSet::new();
    if artifacts.is_empty() {
        return Err(vec![artifact_error("backend emitted no artifacts")]);
    }
    for artifact in &artifacts {
        if artifact.name.is_empty()
            || artifact.name.contains(['\\', ':'])
            || artifact.name.chars().any(char::is_control)
            || artifact.name.split('/').any(|part| part.is_empty() || part == "." || part == "..")
            || !names.insert(&artifact.name)
        {
            return Err(vec![artifact_error("invalid or duplicate artifact path")]);
        }
    }
    Ok(artifacts)
}

fn artifact_error(message: &str) -> Diagnostic {
    Diagnostic::new("backend", "emit", "backend.artifact", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Probe {
        version_ok: bool,
        mode: Support,
        names: Vec<String>,
        emitted: AtomicBool,
    }
    impl Backend for Probe {
        fn target_id(&self) -> &str {
            "x86_64-whale-linux"
        }
        fn supports_versions(&self, _: u32, _: u32) -> bool {
            self.version_ok
        }
        fn support(&self, _: Requirement<'_>, _: &TargetSpec) -> Support {
            self.mode.clone()
        }
        fn emit(&self, _: &CheckedModule<'_>) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
            self.emitted.store(true, Ordering::SeqCst);
            Ok(self
                .names
                .iter()
                .map(|name| Artifact { name: name.clone(), bytes: vec![42] })
                .collect())
        }
    }
    fn probe(mode: Support) -> Probe {
        Probe {
            version_ok: true,
            mode,
            names: vec!["program.bin".into()],
            emitted: AtomicBool::new(false),
        }
    }
    fn module() -> ir::Module {
        let t = ir::Target::X86_64WhaleLinux;
        let mut builder = ir::ModuleBuilder::new(t.name(), t.data_layout());
        let mut f = builder.begin_function("main", vec![], ir::Type::I32);
        let value = f.const_i32(42);
        f.ret(Some(value));
        f.finish();
        builder.finish()
    }
    #[test]
    fn unsupported_and_version_mismatch_never_reach_emission() {
        let m = module();
        let t = ir::Target::X86_64WhaleLinux.specification();
        let p = probe(Support::Unsupported("no i32".into()));
        assert!(compile(&p, &m, &t, ir::IrLimits::default()).is_err());
        assert!(!p.emitted.load(Ordering::SeqCst));
        let mut p = probe(Support::Direct);
        p.version_ok = false;
        let errors = compile(&p, &m, &t, ir::IrLimits::default()).err().unwrap();
        assert_eq!(errors[0].code, "backend.contract");
        assert!(!p.emitted.load(Ordering::SeqCst));
    }
    #[test]
    fn runtime_requirements_are_deduplicated_and_invalid_ids_fail() {
        let m = module();
        let t = ir::Target::X86_64WhaleLinux.specification();
        let p = probe(Support::Runtime("vendor.int32.v1".into()));
        let checked = check(&p, &m, &t, ir::IrLimits::default()).unwrap();
        assert_eq!(checked.runtimes().iter().collect::<Vec<_>>(), vec!["vendor.int32.v1"]);
        assert!(!p.emitted.load(Ordering::SeqCst));
        let p = probe(Support::Runtime(String::new()));
        assert!(check(&p, &m, &t, ir::IrLimits::default()).is_err());
    }
    #[test]
    fn artifact_names_and_nonempty_output_are_checked() {
        let m = module();
        let t = ir::Target::X86_64WhaleLinux.specification();
        for names in [
            vec![],
            vec!["../escape"],
            vec!["/absolute"],
            vec!["a\\b"],
            vec!["a", "a"],
            vec!["a//b"],
            vec!["C:drive"],
            vec!["bad\nname"],
        ] {
            let mut p = probe(Support::Expanded);
            p.names = names.into_iter().map(str::to_owned).collect();
            assert!(compile(&p, &m, &t, ir::IrLimits::default()).is_err());
        }
        let p = probe(Support::Expanded);
        assert_eq!(compile(&p, &m, &t, ir::IrLimits::default()).unwrap()[0].bytes, [42]);
    }
}
