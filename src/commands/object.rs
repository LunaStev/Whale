use object::{ObjectFile, ObjectFormat, ObjectSymbol, SectionKind, SymbolBinding};

use std::process;

use std::time::Instant;

use super::debug::dump_bytes;

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut input = None;
    let mut output = None;
    let mut max_input = None;

    let mut debug_mode = false;
    let mut show_bytes = false;
    let mut dump_hex = false;
    let mut dump_bin = false;
    let mut dump_json = false;
    let mut show_stats = false;
    let mut trace_enable = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => {
                print_help();
                return;
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
            "--debug-whale" => debug_mode = true,
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
            "--bytes" => show_bytes = true,
            "--dump-hex" => dump_hex = true,
            "--dump-bin" => dump_bin = true,
            "--dump-json" => dump_json = true,
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

    let input = input.unwrap_or_else(|| {
        eprintln!("Error: No input file");
        process::exit(1);
    });
    let output = output.unwrap_or_else(|| {
        eprintln!("Error: No output file (-o)");
        process::exit(1);
    });

    if trace_enable {
        println!("[trace] reading input file: {}", input);
    }
    let start_time = Instant::now();

    let bytes =
        super::input::read(input.as_ref(), max_input.unwrap_or(super::input::OBJECT_INPUT_BYTES))
            .unwrap_or_else(|e| {
                eprintln!("Failed to read input: {}", e);
                process::exit(1);
            });

    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    let sec_idx = obj.add_section(".text", SectionKind::Text, 16);
    obj.sections[sec_idx].data = bytes;

    obj.symbols.push(ObjectSymbol {
        kind: object::SymbolKind::NoType,
        name: "start".to_string(),
        section_index: Some(sec_idx),
        value: 0,
        size: 0,
        binding: SymbolBinding::Global,
        visibility: object::SymbolVisibility::Default,
    });

    let out_bytes = obj.write().unwrap_or_else(|e| {
        eprintln!("ELF build error: {e}");
        process::exit(1);
    });
    let elapsed = start_time.elapsed();

    super::output::publish(input.as_ref(), output.as_ref(), &out_bytes).unwrap_or_else(|e| {
        eprintln!("Failed to write {output}: {e}");
        process::exit(1);
    });

    if debug_mode && (show_bytes || dump_hex || dump_bin || dump_json) {
        dump_bytes("object", &out_bytes, show_bytes, dump_hex, dump_bin, dump_json);
    }

    if debug_mode && show_stats {
        println!(
            "== STATS ==\nInput bytes: {}\nOutput bytes: {}\nTime: {} ms",
            obj.sections[sec_idx].data.len(),
            out_bytes.len(),
            elapsed.as_millis()
        );
    }
    println!("Created object file {} ({} bytes)", output, out_bytes.len());
}

pub fn print_help() {
    println!("Usage:");
    println!("  whale object <input> -o <output>");
    println!();
    println!("Wrap raw bytes in an AMD64 ELF64 object with a .text section.");
    println!();
    println!("Options:");
    println!("  --max-input N   input byte limit (default 268435456 bytes)");
    println!("  --help          show this help");
    println!("  --debug-whale   enable debug features");
    println!("  --bytes         print object bytes (debug, first 256 bytes)");
    println!("  --dump-hex      print object bytes as hex (debug, first 256 bytes)");
    println!("  --dump-bin      print object bytes as binary (debug, first 256 bytes)");
    println!("  --dump-json     print object size and byte head as JSON (debug)");
    println!("  --stats         show stats (debug)");
    println!("  --trace         trace logs");
}
