use std::process;

use std::time::Instant;

use super::debug::dump_bytes;

use assembler::isa::amd64::parser::parse;

use assembler::tokens::tokenize;

use assembler::{isa::AMD64, AssemblerOutput};

use object::{
    ObjectFile, ObjectFormat, ObjectRelocation, ObjectSymbol, RelocKind as ObjectRelocKind,
    SectionKind, SymbolBinding, SymbolVisibility,
};

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut arch = None;
    let mut input = None;
    let mut output = None;
    let mut max_input = None;

    let mut debug_mode = false;
    let mut show_ast = false;
    let mut show_token = false;
    let mut show_bytes = false;
    let mut dump_hex = false;
    let mut dump_bin = false;
    let mut dump_json = false;
    let mut _no_color = false;
    let mut _no_warn_ext = false;
    let mut show_stats = false;
    let mut trace_enable = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => {
                print_help();
                return;
            },

            flag @ ("--amd64" | "--aarch64") => {
                if arch.is_some() {
                    eprintln!("Error: architecture must be specified only once");
                    process::exit(1);
                }
                arch = Some(if flag == "--amd64" { "amd64" } else { "aarch64" });
            },

            "-o" => {
                if output.is_some() {
                    eprintln!("Error: duplicate -o option");
                    process::exit(1);
                }
                let path = args.get(i + 1).filter(|s| !s.starts_with('-')).unwrap_or_else(|| {
                    eprintln!("Error: -o requires a path (use ./ for paths starting with '-')");
                    process::exit(1);
                });
                output = Some(path.clone());
                i += 1;
            },
            "--max-input" => {
                if max_input.is_some() {
                    eprintln!("Error: duplicate --max-input option");
                    process::exit(1);
                }
                let limit = args.get(i + 1).unwrap_or_else(|| {
                    eprintln!("Error: --max-input requires a decimal byte count");
                    process::exit(1);
                });
                max_input = Some(super::input::byte_limit(limit).unwrap_or_else(|e| {
                    eprintln!("Error: {e}");
                    process::exit(1);
                }));
                i += 1;
            },

            "--debug-whale" => debug_mode = true,
            "--ast" => show_ast = true,
            "--token" => show_token = true,
            "--bytes" => show_bytes = true,
            "--dump-hex" => dump_hex = true,
            "--dump-bin" => dump_bin = true,
            "--dump-json" => dump_json = true,
            "--no-color" => _no_color = true,
            "--no-warn-extension" => _no_warn_ext = true,
            "--stats" => show_stats = true,
            "--trace" => trace_enable = true,

            s if input.is_none() && !s.starts_with('-') => input = Some(s.to_string()),
            s => {
                let kind =
                    if s.starts_with('-') { "unknown option" } else { "unexpected argument" };
                eprintln!("Error: {kind}: {s}");
                process::exit(1);
            },
        }
        i += 1;
    }

    let arch = arch.unwrap_or_else(|| {
        eprintln!("Error: architecture must be specified (--amd64)");
        process::exit(1);
    });

    if arch != "amd64" {
        eprintln!("Error: only --amd64 is supported right now");
        process::exit(1);
    }

    let input = input.unwrap_or_else(|| {
        eprintln!("Error: missing input file.");
        process::exit(1);
    });

    let output = output.unwrap_or_else(|| {
        eprintln!("Error: missing output (-o)");
        process::exit(1);
    });

    if !output.ends_with(".o") {
        eprintln!("Error: for now, asm output must be .o (object).");
        process::exit(1);
    }

    if trace_enable {
        println!("[trace] reading input file: {}", input);
    }
    let bytes =
        super::input::read(input.as_ref(), max_input.unwrap_or(super::input::ASM_INPUT_BYTES))
            .unwrap_or_else(|e| {
                eprintln!("Failed to read {}: {}", input, e);
                process::exit(1);
            });
    let src = String::from_utf8(bytes).unwrap_or_else(|e| {
        eprintln!("Failed to read {input}: assembly must be UTF-8: {e}");
        process::exit(1);
    });

    let mut token_len: Option<usize> = None;
    let mut ast_items_len: Option<usize> = None;

    let need_tokens = debug_mode && (show_token || show_ast || show_stats);
    if need_tokens {
        if trace_enable {
            println!("[trace] tokenize start");
        }
        let tokens = tokenize(&src).unwrap_or_else(|e| {
            eprintln!("Tokenize error: {}", e.with_source_name(&input));
            process::exit(1);
        });
        token_len = Some(tokens.len());

        if show_token {
            dbg!(&tokens);
        }

        if show_ast || show_stats {
            if trace_enable {
                println!("[trace] parse start");
            }
            let ast = parse(&tokens).unwrap_or_else(|e| {
                eprintln!("Parse error: {}", e.with_source_name(&input));
                process::exit(1);
            });
            ast_items_len = Some(ast.items.len());

            if show_ast {
                println!("{:#?}", ast);
            }
        }
    }

    if trace_enable {
        println!("[trace] assemble start");
    }
    let start_time = Instant::now();
    let out = assembler::assemble_named(&input, &src, &AMD64).unwrap_or_else(|e| {
        eprintln!("Assemble error: {}", e);
        process::exit(1);
    });
    let elapsed = start_time.elapsed();

    if trace_enable {
        println!("[trace] creating object file");
    }
    let final_bytes = build_elf_from_asm_output(&out).unwrap_or_else(|e| {
        eprintln!("ELF build error: {}", e);
        process::exit(1);
    });

    super::output::publish(input.as_ref(), output.as_ref(), &final_bytes).unwrap_or_else(|e| {
        eprintln!("Failed to write {}: {}", output, e);
        process::exit(1);
    });

    if debug_mode && (show_bytes || dump_hex || dump_bin || dump_json) {
        dump_bytes("object", &final_bytes, show_bytes, dump_hex, dump_bin, dump_json);
    }

    if debug_mode && show_stats {
        println!(
            "== STATS ==\nTokens: {}\nAST nodes: {}\nObject bytes: {}\nTime: {} ms",
            token_len.unwrap_or(0),
            ast_items_len.unwrap_or(0),
            final_bytes.len(),
            elapsed.as_millis()
        );
    }

    println!("Wrote {} bytes to {}", final_bytes.len(), output);
}

fn build_elf_from_asm_output(out: &AssemblerOutput) -> Result<Vec<u8>, String> {
    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    let mut section_map = Vec::with_capacity(out.sections.len());

    for sec in &out.sections {
        let kind = match sec.kind {
            assembler::AsmSectionKind::Text => SectionKind::Text,
            assembler::AsmSectionKind::Data => SectionKind::Data,
            assembler::AsmSectionKind::ReadOnlyData => SectionKind::ReadOnlyData,
            assembler::AsmSectionKind::Bss => SectionKind::Bss,
        };
        let align = sec.align;
        let idx = obj.add_section(&sec.name, kind, align);
        obj.sections[idx].data = sec.data.clone();
        obj.sections[idx].zero_fill = sec.zero_fill;
        section_map.push(idx);
    }

    for sym in &out.symbols {
        let section_index = sym.section_index.and_then(|idx| section_map.get(idx).copied());

        obj.symbols.push(ObjectSymbol {
            kind: match sym.kind {
                assembler::SymbolKind::NoType => object::SymbolKind::NoType,
                assembler::SymbolKind::Function => object::SymbolKind::Function,
                assembler::SymbolKind::Object => object::SymbolKind::Object,
            },
            name: sym.name.clone(),
            section_index,
            value: sym.offset,
            size: sym.size,
            binding: if sym.is_global { SymbolBinding::Global } else { SymbolBinding::Local },
            visibility: match sym.visibility {
                assembler::SymbolVisibility::Default => SymbolVisibility::Default,
                assembler::SymbolVisibility::Hidden => SymbolVisibility::Hidden,
            },
        });
    }

    for (asm_sec_idx, sec) in out.sections.iter().enumerate() {
        let Some(&obj_sec_idx) = section_map.get(asm_sec_idx) else {
            continue;
        };

        for reloc in &sec.relocs {
            let is_undefined_symbol = out
                .symbols
                .iter()
                .find(|s| s.name == reloc.symbol)
                .map(|s| s.section_index.is_none())
                .unwrap_or(true);

            let kind = match reloc.kind {
                assembler::assembler::RelocKind::Absolute64 => ObjectRelocKind::Absolute64,
                assembler::assembler::RelocKind::Absolute32 => ObjectRelocKind::Absolute32,
                assembler::assembler::RelocKind::SignedAbsolute32 => {
                    ObjectRelocKind::SignedAbsolute32
                },
                assembler::assembler::RelocKind::Relative32 => ObjectRelocKind::Relative32,
                assembler::assembler::RelocKind::Branch32 => {
                    if is_undefined_symbol {
                        ObjectRelocKind::PLT32
                    } else {
                        ObjectRelocKind::Relative32
                    }
                },
                assembler::assembler::RelocKind::Relative8 => ObjectRelocKind::Relative8,
            };

            obj.relocations.push(ObjectRelocation {
                section_index: obj_sec_idx,
                offset: reloc.offset,
                symbol: reloc.symbol.clone(),
                addend: reloc.addend,
                kind,
            });

            if obj.symbols.iter().all(|s| s.name != reloc.symbol) {
                obj.symbols.push(ObjectSymbol {
                    kind: object::SymbolKind::NoType,
                    name: reloc.symbol.clone(),
                    section_index: None,
                    value: 0,
                    size: 0,
                    binding: SymbolBinding::Global,
                    visibility: SymbolVisibility::Default,
                });
            }
        }
    }

    obj.write()
}

fn print_help() {
    println!("Usage:");
    println!("  whale asm --amd64 <input> -o <output.o>");
    println!();
    println!("Options:");
    println!("  --max-input N   input byte limit (default 8388608 bytes)");
    println!("  --debug-whale   enable debug features");
    println!("  --ast           print parser AST (debug)");
    println!("  --token         print tokens (debug)");
    println!("  --bytes         print object bytes (debug)");
    println!("  --dump-hex      print object bytes as hex (debug)");
    println!("  --dump-bin      print object bytes as binary (debug)");
    println!("  --dump-json     print object bytes as json (debug)");
    println!("  --stats         show stats (debug)");
    println!("  --trace         trace logs");
}
