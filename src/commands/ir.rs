use std::{fs, io::Read, process};

use ir::lower_ast::{interchange, lower_o0};
use ir::{printer, verifier, Target};

pub fn run(args: Vec<String>) {
    if args.is_empty() {
        print_help();
        return;
    }

    let mut sub: Option<String> = None;
    let mut input: Option<String> = None;
    let mut output: Option<String> = None;

    let mut target: String = "x86_64-whale-linux".to_string();
    let mut do_verify = true;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" => {
                print_help();
                return;
            }

            "lower" => {
                sub = Some("lower".to_string());
            }

            "--target" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --target requires a value");
                    process::exit(1);
                }
                target = args[i + 1].clone();
                i += 1;
            }

            "-o" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: -o requires a value");
                    process::exit(1);
                }
                output = Some(args[i + 1].clone());
                i += 1;
            }

            "--no-verify" => do_verify = false,

            s if !s.starts_with('-') && input.is_none() => {
                input = Some(s.to_string());
            }

            _ => {}
        }
        i += 1;
    }

    let sub = sub.unwrap_or_else(|| {
        eprintln!("Error: missing subcommand");
        print_help();
        process::exit(1);
    });

    if sub != "lower" {
        eprintln!("Error: unsupported subcommand: {}", sub);
        print_help();
        process::exit(1);
    }

    let input = input.unwrap_or_else(|| {
        eprintln!("Error: missing input file");
        print_help();
        process::exit(1);
    });

    let target = Target::lookup(&target).unwrap_or_else(|e| {
        eprintln!("Error: {e}");
        process::exit(1);
    });

    // 1) Read socket json
    let src = (|| -> std::io::Result<String> {
        let mut source = String::new();
        fs::File::open(&input)?
            .take((interchange::DEFAULT_MAX_INPUT_BYTES + 1) as u64)
            .read_to_string(&mut source)?;
        Ok(source)
    })()
    .unwrap_or_else(|e| {
        eprintln!("Failed to read {}: {}", input, e);
        process::exit(1);
    });

    // 2) JSON -> frontend::Program
    let program = interchange::decode(&src).unwrap_or_else(|e| {
        eprintln!("Failed to parse socket JSON: {}", e);
        process::exit(1);
    });

    // 3) lower
    let module = lower_o0(&program, target.name(), target.data_layout()).unwrap_or_else(|e| {
        eprintln!("lower_o0 failed: {:?}", e);
        process::exit(1);
    });

    // 4) verify(optional)
    if do_verify {
        verifier::verify_module(&module).unwrap_or_else(|e| {
            eprintln!("verify failed: {:?}", e);
            process::exit(1);
        });
    }

    // 5) print
    let txt = printer::print_module(&module);

    // 6) output
    if let Some(out) = output {
        super::output::publish(input.as_ref(), out.as_ref(), txt.as_bytes()).unwrap_or_else(|e| {
            eprintln!("Failed to write {}: {}", out, e);
            process::exit(1);
        });
        println!("Wrote IR to {}", out);
    } else {
        print!("{}", txt);
    }
}

fn print_help() {
    println!("Usage:");
    println!("  whale ir <command> [options]");
    println!();
    println!("Commands:");
    println!("  lower <socket.json>   Lower socket AST JSON into Whale IR text");
    println!();
    println!("Options (ir lower):");
    println!("  -o <path>        Write printed IR text to file (default: stdout)");
    println!("  --target <t>     Output target (default and supported: x86_64-whale-linux)");
    println!("  --no-verify      Skip verifier");
    println!("  --no-print       Do not print IR text to stdout (useful with -o)");
    println!("  --trace          Print trace logs (parse/lower/verify/write steps)");
    println!("  --help           Show this help");
    println!();
    println!("Input format:");
    println!(
        "  Required envelope: format_version: 1, semantics_version: 1, features: [], program: AST"
    );
    println!("  Integer values are decimal strings; float values are exact-width 0x bit strings.");
    println!("  Unknown fields/features, duplicate keys and unversioned input are rejected.");
    println!();
    println!("Examples:");
    println!("  whale ir lower program.json");
    println!("  whale ir lower program.json -o out.wir");
    println!("  whale ir lower program.json --target x86_64-whale-linux");
    println!("  whale ir lower program.json --no-verify");
    println!("  whale ir lower program.json -o out.wir --no-print");
    println!("  whale ir lower program.json --trace");
}
