pub mod asm;
pub mod linker;
pub mod object;

#[cfg(feature = "socket-cli")]
pub mod ir;

#[cfg(not(feature = "socket-cli"))]
pub mod ir {
    pub fn run(_: Vec<String>) {
        eprintln!("Error: 'whale ir' requires feature 'socket-cli'.");
        eprintln!("Build/run with: cargo run -p whale --features socket-cli -- ir ...");
        std::process::exit(2);
    }
}
