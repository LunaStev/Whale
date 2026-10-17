// SPDX-License-Identifier: MPL-2.0
//! A tiny executable bytecode target, deliberately independent of AMD64.
//! Encoding: CONST=1,u32 dst,i64; ADD=2,u32 dst,u32 lhs,u32 rhs; RET=3,u32 src.
use assembler::{AsmError, AsmSection, AsmSectionKind, AssemblerOutput, AssemblyLimits};
use backend_api::{Artifact, Backend, CheckedModule, Requirement, Support};
use diagnostic::Diagnostic;
use ir::{BinOp, ConstValue, Instruction, Terminator, Type};
use target_spec::{DataLayout, Endian, MemoryModel, OutputKind, StorageLayout, TargetSpec};

pub const TARGET: &str = "example-bytecode-v1";

pub fn target() -> TargetSpec {
    TargetSpec {
        id: TARGET.into(),
        output: OutputKind::Bytecode("example-v1".into()),
        memory: MemoryModel::Tracked,
        storage: StorageLayout {
            data: DataLayout { ptr_bits: 64, endian: Endian::Little },
            scalar_align: [1, 2, 4, 8, 8],
            pointer_align: 8,
            large_array: None,
        },
    }
}

pub fn register(registry: &mut toolchain::Registry) -> Result<(), String> {
    registry.register_target(target())?;
    registry.register_assembler(ExampleAssembler)?;
    registry.register_backend(ExampleBackend)
}

/// Own grammar, parser, and encoding; no shared tokens or AMD64 AST required.
/// Syntax: `value 42` creates a constant in r0 and returns it.
pub struct ExampleAssembler;
impl assembler::extension::Assembler for ExampleAssembler {
    fn id(&self) -> &str {
        TARGET
    }
    fn assemble(
        &self,
        name: &str,
        source: &str,
        limits: &AssemblyLimits,
    ) -> Result<AssemblerOutput, AsmError> {
        let words: Vec<_> = source.split_whitespace().collect();
        let error = || AsmError::ParserError("expected: value <i64>".into()).with_source_name(name);
        if words.len() != 2 || words[0] != "value" {
            return Err(error());
        }
        let value: i64 = words[1].parse().map_err(|_| error())?;
        if limits.max_output_bytes < 18 {
            return Err(AsmError::EncodeError("Assembly output limit exceeded".into()));
        }
        let mut bytes = vec![1];
        word(&mut bytes, 0);
        bytes.extend(value.to_le_bytes());
        bytes.push(3);
        word(&mut bytes, 0);
        Ok(AssemblerOutput {
            sections: vec![AsmSection {
                kind: AsmSectionKind::Text,
                align: 1,
                name: "code".into(),
                data: bytes,
                zero_fill: 0,
                relocs: vec![],
            }],
            symbols: vec![],
        })
    }
}

pub struct ExampleBackend;
impl Backend for ExampleBackend {
    fn target_id(&self) -> &str {
        TARGET
    }
    fn support(&self, requirement: Requirement<'_>, _target: &TargetSpec) -> Support {
        let supported = match requirement {
            Requirement::Type(Type::I64) => true,
            Requirement::Declaration(d) => {
                d.linkage == ir::Linkage::Internal
                    && d.signature.convention == ir::CallingConvention::Whale
                    && !d.signature.variadic
            },
            Requirement::Instruction(
                Instruction::Const { ty: Type::I64, .. }
                | Instruction::Bin { op: BinOp::Add, ty: Type::I64, .. },
            ) => true,
            Requirement::Terminator(Terminator::Ret { ty: Type::I64, value: Some(_) }) => true,
            _ => false,
        };
        if supported {
            Support::Direct
        } else {
            Support::Unsupported(
                "example target supports only i64 constants, addition and return".into(),
            )
        }
    }
    fn validate(&self, checked: &CheckedModule<'_>) -> Result<(), Vec<Diagnostic>> {
        let functions = &checked.module().functions;
        if functions.len() != 1 || !functions[0].params.is_empty() || functions[0].blocks.len() != 1
        {
            return Err(vec![error(
                "example target requires one parameterless, single-block function",
            )]);
        }
        Ok(())
    }
    fn emit(&self, checked: &CheckedModule<'_>) -> Result<Vec<Artifact>, Vec<Diagnostic>> {
        let mut bytes = Vec::new();
        for instruction in &checked.module().functions[0].blocks[0].instructions {
            match instruction {
                Instruction::Const { dst, value: ConstValue::I(value), .. } => {
                    bytes.push(1);
                    word(&mut bytes, dst.0);
                    bytes.extend((*value as i64).to_le_bytes());
                },
                Instruction::Bin { dst, lhs, rhs, .. } => {
                    bytes.push(2);
                    word(&mut bytes, dst.0);
                    word(&mut bytes, lhs.0);
                    word(&mut bytes, rhs.0);
                },
                _ => return Err(vec![error("unsupported instruction encoding")]),
            }
        }
        if let Some(Terminator::Ret { value: Some(value), .. }) =
            &checked.module().functions[0].blocks[0].terminator
        {
            bytes.push(3);
            word(&mut bytes, value.0);
        }
        Ok(vec![Artifact { name: "program.wexample".into(), bytes }])
    }
}
fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend(value.to_le_bytes());
}
fn error(message: &str) -> Diagnostic {
    Diagnostic::new("example", "emit", "example.unsupported", message)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn module() -> ir::Module {
        let mut builder = ir::ModuleBuilder::new(TARGET, target().storage.data);
        let mut f = builder.begin_function("main", vec![], Type::I64);
        let a = f.const_int(Type::I64, i64::MAX as i128);
        let b = f.const_int(Type::I64, 1);
        let c = f.add(Type::I64, a, b);
        f.ret(Some(c));
        f.finish();
        builder.finish()
    }
    // Independent execution of the artifact proves that the extension emits code.
    fn execute(bytes: &[u8]) -> i64 {
        let mut pc = 0;
        let mut registers = std::collections::HashMap::new();
        fn u32_at(bytes: &[u8], pc: &mut usize) -> u32 {
            let value = u32::from_le_bytes(bytes[*pc..*pc + 4].try_into().unwrap());
            *pc += 4;
            value
        }
        loop {
            let opcode = bytes[pc];
            pc += 1;
            let dst = u32_at(bytes, &mut pc);
            match opcode {
                1 => {
                    let value = i64::from_le_bytes(bytes[pc..pc + 8].try_into().unwrap());
                    pc += 8;
                    registers.insert(dst, value);
                },
                2 => {
                    let a = u32_at(bytes, &mut pc);
                    let b = u32_at(bytes, &mut pc);
                    let v = registers[&a].wrapping_add(registers[&b]);
                    registers.insert(dst, v);
                },
                3 => {
                    assert_eq!(pc, bytes.len());
                    return registers[&dst];
                },
                _ => panic!("unknown opcode"),
            }
        }
    }
    #[test]
    fn external_backend_and_assembler_run_without_builtin_target_changes() {
        let mut registry = toolchain::Registry::new();
        register(&mut registry).unwrap();
        let artifacts = registry.compile(&module(), ir::IrLimits::default()).unwrap();
        assert_eq!(execute(&artifacts[0].bytes), i64::MIN);
        let output = registry
            .assemble(TARGET, "input.example", "value 42", &AssemblyLimits::default())
            .unwrap();
        assert_eq!(execute(&output.sections[0].data), 42);
        assert!(registry
            .assemble(
                TARGET,
                "input",
                "value 42",
                &AssemblyLimits { max_output_bytes: 17, ..Default::default() }
            )
            .is_err());
    }
    #[test]
    fn registration_and_semantic_failures_are_reported() {
        let mut registry = toolchain::Registry::new();
        assert!(registry.register_backend(ExampleBackend).is_err());
        register(&mut registry).unwrap();
        assert!(registry.register_target(target()).is_err());
        assert!(registry.register_assembler(ExampleAssembler).is_err());
        assert!(registry.register_backend(ExampleBackend).is_err());
        assert!(registry.assembler("missing").is_none());
        let mut m = module();
        m.datalayout.ptr_bits = 32;
        assert!(registry.compile(&m, ir::IrLimits::default()).is_err());
        let mut m = module();
        if let Instruction::Bin { op, .. } = &mut m.functions[0].blocks[0].instructions[2] {
            *op = BinOp::Mul;
        }
        let errors = registry.compile(&m, ir::IrLimits::default()).err().unwrap();
        assert_eq!(errors[0].code, "backend.unsupported");
        assert!(errors[0]
            .contexts
            .iter()
            .any(|c| matches!(c, diagnostic::Context::Instruction(2))));
        assert!(registry.target(TARGET).is_some());
    }
}
