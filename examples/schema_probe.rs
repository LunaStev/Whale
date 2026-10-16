#[cfg(feature = "socket-cli")]
fn main() {
    let path = std::env::args_os().nth(1).expect("AST fixture path");
    let bytes = std::fs::read(path).unwrap();
    assert!(bytes.len() <= 65536);
    let Ok(source) = std::str::from_utf8(&bytes) else {
        println!("decode=reject lower=skip verify=skip");
        return;
    };
    let Ok(program) = ir::lower_ast::interchange::decode_with_limit(source, 65536) else {
        println!("decode=reject lower=skip verify=skip");
        return;
    };
    let target = ir::Target::X86_64WhaleLinux;
    match ir::lower_ast::lower_o0(&program, target.name(), target.data_layout()) {
        Ok(module) => {
            ir::verify_module(&module).expect("successful lowering must verify");
            println!("decode=accept lower=accept verify=accept");
        },
        Err(_) => println!("decode=accept lower=reject verify=skip"),
    }
}

#[cfg(not(feature = "socket-cli"))]
fn main() {
    eprintln!("schema_probe requires --features socket-cli");
    std::process::exit(2);
}
