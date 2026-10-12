use std::env;

use crate::commands;

pub fn run() {
    let mut args = env::args().skip(1);

    let Some(cmd) = args.next() else {
        print_help();
        return;
    };

    match cmd.as_str() {
        "--help" | "-h" => print_help(),
        "asm" => commands::asm::run(args.collect()),
        "object" => commands::object::run(args.collect()),
        "link" => commands::linker::run(args.collect()),
        "ir" => commands::ir::run(args.collect()),
        _ => {
            eprintln!("Unknown command: {}", cmd);
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_help() {
    println!("Usage:");
    println!("  whale asm --amd64 <input> -o <output.o>");
    println!("  whale object <input> -o <output>");
    println!("  whale link <...>");
    println!("  whale ir <subcommand> [options]");
    println!();
    println!("Commands:");
    println!("  asm     Assemble AMD64 source into an ELF64 object");
    println!("  object  Wrap raw bytes in an AMD64 ELF64 object");
    println!("  link    Executable linking (not implemented)");
    println!("  ir      IR tools (verify/print/run; socket-cli enables AST lower)");
}
