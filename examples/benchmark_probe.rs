// SPDX-License-Identifier: MPL-2.0
//! Process-isolated public API measurements; see tools/BENCHMARKS.md.
use amd64::AMD64;
use assembler::{ast::AST, tokens::tokenize, traits::ISA, AssemblerOutput};

use ir::{ConstValue, DataLayout, Module, ModuleBuilder, Type};

use object::{
    ObjectFile, ObjectFormat, ObjectSymbol, SectionKind, SymbolBinding, SymbolKind,
    SymbolVisibility,
};

use std::{hint::black_box, path::Path, time::Instant};

const MAX_COUNT: usize = 100_000;

fn assembly(count: usize) -> String {
    let mut source = String::from("section .text\nglobal bench\nbench:\nmov eax,42\n");
    for _ in 0..count {
        source.push_str("add eax,0\n");
    }
    source.push_str("ret\n");
    source
}

fn module(count: usize) -> Module {
    let mut builder = ModuleBuilder::new("x86_64-whale-linux", DataLayout::default_64bit_le());
    let mut f = builder.begin_function("bench", vec![], Type::I32);
    let mut value = f.const_i32(42);
    let zero = f.const_i32(0);
    for _ in 0..count {
        value = f.add(Type::I32, value, zero);
    }
    f.ret(Some(value));
    f.finish();
    builder.finish()
}

fn validate_ast(ast: &AST, count: usize) {
    assert_eq!(ast.items.len(), count + 5);
}

fn validate_assembly(output: &AssemblerOutput, count: usize) {
    assert_eq!(output.sections.len(), 1);
    assert!(output.sections[0].relocs.is_empty());
    assert_eq!(output.symbols.len(), 1);
    let symbol = &output.symbols[0];
    assert_eq!(
        (symbol.name.as_str(), symbol.section_index, symbol.offset, symbol.size, symbol.is_global),
        ("bench", Some(0), 0, 0, true)
    );
    assert!(matches!(symbol.kind, assembler::SymbolKind::NoType));
    assert!(matches!(symbol.visibility, assembler::SymbolVisibility::Default));
    let code = &output.sections[0].data;
    assert_eq!(code.len(), 3 * count + 6);
    assert_eq!(&code[..5], &[0xb8, 42, 0, 0, 0]);
    assert_eq!(code.last(), Some(&0xc3));
    assert!(code[5..code.len() - 1].chunks_exact(3).all(|b| b == [0x83, 0xc0, 0]));
}

// This fixture has exactly one code section and symbol and no relocations.
// Include metadata construction and its data clone in object-emission timing.
fn object(output: &AssemblerOutput) -> Vec<u8> {
    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    let section = &output.sections[0];
    let index = obj.add_section(&section.name, SectionKind::Text, section.align);
    obj.sections[index].data = section.data.clone();
    let symbol = &output.symbols[0];
    obj.symbols.push(ObjectSymbol {
        name: symbol.name.clone(),
        kind: SymbolKind::NoType,
        section_index: Some(index),
        value: symbol.offset,
        size: 0,
        binding: SymbolBinding::Global,
        visibility: SymbolVisibility::Default,
    });
    obj.write().unwrap()
}

fn word16(bytes: &[u8], at: usize) -> usize {
    u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize
}

fn word32(bytes: &[u8], at: usize) -> usize {
    u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize
}

fn word64(bytes: &[u8], at: usize) -> usize {
    usize::try_from(u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())).unwrap()
}

fn validate_object(bytes: &[u8], expected: &[u8]) {
    // Independent ELF64 header/section inspection, with no external tools.
    assert_eq!(&bytes[..7], b"\x7fELF\x02\x01\x01");
    assert_eq!(word16(bytes, 16), 1);
    assert_eq!(word16(bytes, 18), 62);
    let (table, stride, count, names) =
        (word64(bytes, 40), word16(bytes, 58), word16(bytes, 60), word16(bytes, 62));
    assert_eq!(stride, 64);
    assert!(names < count && table + count * stride <= bytes.len());
    let string_header = table + names * stride;
    let strings = &bytes[word64(bytes, string_header + 24)..][..word64(bytes, string_header + 32)];
    let mut found = false;
    for index in 1..count {
        let header = table + index * stride;
        let start = word32(bytes, header);
        let name = &strings[start..];
        let end = name.iter().position(|b| *b == 0).unwrap();
        if &name[..end] == b".text" {
            assert_eq!(word64(bytes, header + 8), 6);
            let payload = &bytes[word64(bytes, header + 24)..][..word64(bytes, header + 32)];
            assert_eq!(payload, expected);
            found = true;
        }
    }
    assert!(found);
}

fn digest(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325, |state, byte| {
        (state ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

fn measure<T>(f: impl FnOnce() -> T) -> (u128, T) {
    let started = Instant::now();
    let result = black_box(f());
    (started.elapsed().as_nanos(), result)
}

fn record(phase: &str, count: usize, input: usize, output: usize, elapsed: u128, result: u64) {
    println!("{{\"phase\":\"{phase}\",\"count\":{count},\"input_bytes\":{input},\"output_bytes\":{output},\"elapsed_ns\":{elapsed},\"result_digest\":\"{result:016x}\",\"correct\":true}}");
}

fn run(phase: &str, count: usize, directory: Option<&Path>) -> Result<(), String> {
    if ![
        "prepare",
        "asm-tokenize",
        "asm-parse",
        "asm-encode",
        "asm-object",
        "asm-library-e2e",
        "ir-parse-verify",
        "ir-verify",
        "ir-print",
        "ir-library-e2e",
    ]
    .contains(&phase)
    {
        return Err("unknown benchmark phase".into());
    }
    if count > MAX_COUNT {
        return Err(format!("count exceeds {MAX_COUNT}"));
    }
    if phase == "prepare" {
        let dir = directory.ok_or("prepare requires an output directory")?;
        let source = assembly(count);
        let output = assembler::assemble(&source, &AMD64).map_err(|e| e.to_string())?;
        validate_assembly(&output, count);
        let bytes = object(&output);
        validate_object(&bytes, &output.sections[0].data);
        let m = module(count);
        ir::verify_module(&m).map_err(|e| format!("{e:?}"))?;
        let text = ir::print_module(&m);
        let parsed = ir::parse_module(&text).map_err(|e| e.to_string())?;
        assert_eq!(ir::print_module(&parsed), text);
        assert_eq!(
            ir::interpret(&parsed, ir::FunctionId(0), &[]).unwrap().value,
            Some(ConstValue::I(42))
        );
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        for (name, data) in [
            ("input.asm", source.as_bytes()),
            ("input.wir", text.as_bytes()),
            ("expected.o", bytes.as_slice()),
        ] {
            std::fs::write(dir.join(name), data).map_err(|e| e.to_string())?;
        }
        record(phase, count, source.len() + text.len(), bytes.len(), 0, digest(&bytes));
        return Ok(());
    }
    if phase.starts_with("asm-") {
        let source = assembly(count);
        match phase {
            "asm-tokenize" => {
                let (ns, tokens) = measure(|| tokenize(black_box(&source)).unwrap());
                assert!(!tokens.is_empty());
                record(phase, count, source.len(), 0, ns, tokens.len() as u64);
            },
            "asm-parse" => {
                let tokens = tokenize(&source).unwrap();
                let (ns, ast) = measure(|| AMD64.parse(black_box(&tokens)).unwrap());
                validate_ast(&ast, count);
                record(phase, count, source.len(), 0, ns, ast.items.len() as u64);
            },
            "asm-encode" => {
                let ast = AMD64.parse(&tokenize(&source).unwrap()).unwrap();
                let (ns, out) = measure(|| AMD64.encode(black_box(&ast)).unwrap());
                validate_assembly(&out, count);
                record(
                    phase,
                    count,
                    source.len(),
                    out.sections[0].data.len(),
                    ns,
                    digest(&out.sections[0].data),
                );
            },
            "asm-object" => {
                let out = assembler::assemble(&source, &AMD64).unwrap();
                let (ns, bytes) = measure(|| object(black_box(&out)));
                validate_object(&bytes, &out.sections[0].data);
                record(phase, count, source.len(), bytes.len(), ns, digest(&bytes));
            },
            "asm-library-e2e" => {
                let (ns, (out, bytes)) = measure(|| {
                    let out = assembler::assemble(black_box(&source), &AMD64).unwrap();
                    let bytes = object(&out);
                    (out, bytes)
                });
                validate_assembly(&out, count);
                validate_object(&bytes, &out.sections[0].data);
                record(phase, count, source.len(), bytes.len(), ns, digest(&bytes));
            },
            _ => return Err("unknown assembly phase".into()),
        }
    } else {
        let m = module(count);
        let text = ir::print_module(&m);
        match phase {
            // Public parse_module includes verification; do not label this parse-only.
            "ir-parse-verify" => {
                let (ns, parsed) = measure(|| ir::parse_module(black_box(&text)).unwrap());
                assert_eq!(ir::print_module(&parsed), text);
                record(phase, count, text.len(), 0, ns, digest(text.as_bytes()));
            },
            "ir-verify" => {
                let (ns, ()) = measure(|| ir::verify_module(black_box(&m)).unwrap());
                record(phase, count, text.len(), 0, ns, digest(text.as_bytes()));
            },
            "ir-print" => {
                let (ns, printed) = measure(|| ir::print_module(black_box(&m)));
                assert_eq!(printed, text);
                record(phase, count, text.len(), printed.len(), ns, digest(printed.as_bytes()));
            },
            "ir-library-e2e" => {
                let (ns, printed) =
                    measure(|| ir::print_module(&ir::parse_module(black_box(&text)).unwrap()));
                assert_eq!(printed, text);
                record(phase, count, text.len(), printed.len(), ns, digest(printed.as_bytes()));
            },
            _ => return Err("unknown IR phase".into()),
        }
    }
    Ok(())
}

fn main() -> Result<(), String> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if !(2..=3).contains(&args.len()) {
        return Err("usage: benchmark_probe PHASE COUNT [PREPARE_DIRECTORY]".into());
    }
    let phase = args[0].to_str().ok_or("phase is not UTF-8")?;
    let count =
        args[1].to_str().ok_or("count is not UTF-8")?.parse().map_err(|_| "invalid count")?;
    run(phase, count, args.get(2).map(Path::new))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_bytes_and_ir_execution_are_independent_correctness_checks() {
        for n in [0, 1, 64] {
            let source = assembly(n);
            let out = assembler::assemble(&source, &AMD64).unwrap();
            validate_assembly(&out, n);
            validate_object(&object(&out), &out.sections[0].data);
            let m = module(n);
            ir::verify_module(&m).unwrap();
            assert_eq!(
                ir::interpret(&m, ir::FunctionId(0), &[]).unwrap().value,
                Some(ConstValue::I(42))
            );
        }
    }

    #[test]
    fn counts_and_phase_names_are_bounded_before_workload_allocation() {
        assert!(run("asm-encode", MAX_COUNT + 1, None).is_err());
        assert!(run("unknown", 1, None).is_err());
    }
}
