use linker::{core::linker::OutputFormat, Linker};

use std::{path::Path, process};

pub fn run(args: Vec<String>) {
    if args.is_empty() || args == ["--help"] {
        help();
        return;
    }
    if let Err(error) = execute(args) {
        eprintln!("Error: {error}");
        process::exit(1);
    }
}

fn execute(args: Vec<String>) -> Result<(), String> {
    let mut inputs = Vec::new();
    let mut output = None;
    let mut entry = None;
    let mut input_limit = None;
    let mut output_limit = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            flag @ ("-o" | "--entry" | "--max-input" | "--max-output") => {
                let value = args
                    .get(i + 1)
                    .filter(|v| !v.starts_with('-'))
                    .ok_or_else(|| format!("{flag} requires a value"))?
                    .clone();
                match flag {
                    "-o" => {
                        if output.replace(value).is_some() {
                            return Err("duplicate -o".into());
                        }
                    },
                    "--entry" => {
                        if value.is_empty() || entry.replace(value).is_some() {
                            return Err("duplicate or empty --entry".into());
                        }
                    },
                    "--max-input" => {
                        if input_limit
                            .replace(super::input::byte_limit(&value).map_err(str::to_owned)?)
                            .is_some()
                        {
                            return Err("duplicate --max-input".into());
                        }
                    },
                    _ => {
                        if output_limit
                            .replace(super::input::byte_limit(&value).map_err(str::to_owned)?)
                            .is_some()
                        {
                            return Err("duplicate --max-output".into());
                        }
                    },
                }
                i += 1;
            },
            value if !value.starts_with('-') => inputs.push(value.to_owned()),
            value => return Err(format!("unknown option {value}")),
        }
        i += 1;
    }
    if inputs.is_empty() || inputs.len() > 4096 {
        return Err("link requires 1..4096 input objects/archives".into());
    }
    let output = output.ok_or("missing output (-o)")?;
    let mut remaining = input_limit.unwrap_or(super::input::OBJECT_INPUT_BYTES);
    let mut linker = Linker::with_read_limits(
        OutputFormat::ELF64Executable,
        object::ObjectReadLimits {
            max_input_bytes: usize::try_from(remaining).unwrap_or(usize::MAX),
            ..object::ObjectReadLimits::default()
        },
    );
    linker.set_entry(entry.unwrap_or_else(|| "_start".into()));
    for input in &inputs {
        let bytes =
            super::input::read(Path::new(input), remaining).map_err(|e| format!("{input}: {e}"))?;
        remaining = remaining
            .checked_sub(bytes.len() as u64)
            .ok_or("total link input byte limit exceeded")?;
        if bytes.starts_with(b"!<arch>\n") || bytes.starts_with(b"!<thin>\n") {
            linker.add_archive(&bytes).map_err(|e| format!("{input}: {e}"))?;
        } else {
            linker.add_object_bytes(&bytes).map_err(|e| format!("{input}: {e}"))?;
        }
    }
    let bytes = linker
        .link_with_limit(output_limit.unwrap_or(256 * 1024 * 1024))
        .map_err(|e| e.to_string())?;
    let paths: Vec<_> = inputs.iter().map(Path::new).collect();
    super::output::publish_many(&paths, Path::new(&output), &bytes, true)
        .map_err(|e| format!("{output}: {e}"))?;
    println!("Wrote {} bytes to {}", bytes.len(), output);
    Ok(())
}

fn help() {
    println!("Usage: whale link <input.o|input.a>... -o <executable> [--entry NAME]\n\nStatic Linux AMD64 ELF64 only; entry defaults to _start.\n--max-input N    cumulative input byte limit (default 268435456)\n--max-output N   executable byte limit (default 268435456)\nArchives use left-to-right order with a fixed-point scan within each archive.\nThin archives, shared libraries, GOT and dynamic PLT linking are unsupported.");
}
