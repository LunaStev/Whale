//! Dependency-free bounded mutation targets; every accepted input exercises real library APIs.
#![allow(dead_code)]
use std::path::{Path, PathBuf};
pub const MAX_INPUT: usize = 8192;
#[derive(Clone, Copy, Debug)]
pub enum Target {
    Assembler,
    Ir,
    Ast,
}
impl Target {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "assembler" => Some(Self::Assembler),
            "ir" => Some(Self::Ir),
            "ast" => Some(Self::Ast),
            _ => None,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Assembler => "assembler",
            Self::Ir => "ir",
            Self::Ast => "ast",
        }
    }
}
pub fn ir_limits() -> ir::IrLimits {
    ir::IrLimits {
        max_input_bytes: MAX_INPUT,
        max_tokens: 4096,
        max_nodes: 8192,
        max_type_depth: 32,
        max_const_depth: 32,
        max_analysis_work: 131072,
    }
}
/// Rejection is normal. A panic means a target violated its bounded-input contract.
pub fn check(target: Target, bytes: &[u8]) -> bool {
    if bytes.len() > MAX_INPUT {
        return false;
    }
    let Ok(source) = std::str::from_utf8(bytes) else {
        return false;
    };
    match target {
        Target::Assembler => {
            if let Ok(tokens) = assembler::tokens::tokenize(source) {
                let _ = assembler::isa::amd64::parser::parse(&tokens);
            }
            let limits = assembler::AssemblyLimits {
                max_output_bytes: 65536,
                max_relax_iterations: 32,
            };
            let Ok(output) =
                assembler::assemble_with_limits(source, &assembler::isa::AMD64, &limits)
            else {
                return false;
            };
            assert!(
                output.sections.iter().map(|s| s.data.len()).sum::<usize>()
                    <= limits.max_output_bytes
            );
            for symbol in &output.symbols {
                if let Some(index) = symbol.section_index {
                    let section = output
                        .sections
                        .get(index)
                        .expect("assembled symbol section must exist");
                    assert!(
                        symbol.offset + symbol.size
                            <= section.data.len() as u64 + section.zero_fill
                    );
                }
            }
            true
        }
        Target::Ir => {
            let Ok(module) = ir::parse_module_with_limits(source, ir_limits()) else {
                return false;
            };
            canonical(&module);
            true
        }
        Target::Ast => {
            #[cfg(feature = "socket-cli")]
            {
                let Ok(program) = ir::lower_ast::interchange::decode_with_limit(source, MAX_INPUT)
                else {
                    return false;
                };
                let limits = ir::lower_ast::AstLimits {
                    max_depth: 16,
                    max_nodes: 2048,
                    max_string_bytes: MAX_INPUT,
                    max_output_bytes: 65536,
                    max_source_work: 65_536,
                };
                let target = ir::Target::X86_64WhaleLinux;
                let Ok(module) = ir::lower_ast::lower_o0_with_limits(
                    &program,
                    target.name(),
                    target.data_layout(),
                    limits,
                ) else {
                    return false;
                };
                match ir::verify_module_with_limits(&module, ir_limits()) {
                    Ok(()) => {}
                    Err(ir::VerifyError::ResourceLimit(_)) => return false,
                    Err(error) => panic!("successful O0 lowering produced invalid IR: {error:?}"),
                }
                canonical(&module);
                // Check both the direct owned encoder and strict JSON decoder.
                if let Ok(encoded) = ir::lower_ast::interchange::encode_with_limits(program, limits)
                {
                    ir::lower_ast::interchange::decode_with_limit(&encoded, 65536)
                        .expect("encoded AST must decode");
                }
                true
            }
            #[cfg(not(feature = "socket-cli"))]
            {
                false
            }
        }
    }
}
fn canonical(module: &ir::Module) {
    let text = ir::print_module(module);
    assert!(text.len() <= 262144, "bounded source expanded unexpectedly");
    let mut limits = ir_limits();
    limits.max_input_bytes = 262144;
    limits.max_tokens = 16384;
    let decoded =
        ir::parse_module_with_limits(&text, limits).expect("printed verified module must parse");
    assert_eq!(
        ir::print_module(&decoded),
        text,
        "canonical text/identities changed"
    );
}
pub struct Rng(u64);
impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn index(&mut self, n: usize) -> usize {
        (self.next() as usize) % n.max(1)
    }
}
pub fn mutate(seed: &[u8], rng: &mut Rng) -> Vec<u8> {
    let mut input = seed[..seed.len().min(MAX_INPUT)].to_vec();
    for _ in 0..1 + rng.index(4) {
        let at = rng.index(input.len() + 1);
        match rng.index(5) {
            0 if at < input.len() => {
                input[at] ^= 1 << rng.index(8);
            }
            1 if input.len() < MAX_INPUT => {
                input.insert(at, rng.next() as u8);
            }
            2 if at < input.len() => {
                let end = (at + 1 + rng.index(32)).min(input.len());
                input.drain(at..end);
            }
            3 if !input.is_empty() => {
                let end = (at + 1 + rng.index(64)).min(input.len());
                let copy = input[at..end].to_vec();
                let room = MAX_INPUT - input.len();
                input.splice(at..at, copy.into_iter().take(room));
            }
            _ => {
                let token = [
                    b"9223372036854775808".as_slice(),
                    b"resq 9223372036854775807",
                    b"{{[[",
                    b"\"format_version\":99",
                    b"\r\n",
                    b"\xff",
                ][rng.index(6)];
                let room = MAX_INPUT - input.len();
                input.splice(at..at, token.iter().copied().take(room));
            }
        }
    }
    input
}
pub fn minimize(mut input: Vec<u8>, fails: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let mut width = input.len() / 2;
    while width > 0 {
        let mut at = 0;
        while at + width <= input.len() {
            let mut candidate = input.clone();
            candidate.drain(at..at + width);
            if fails(&candidate) {
                input = candidate;
            } else {
                at += width;
            }
        }
        width /= 2;
    }
    input
}
pub fn read_bounded(path: &Path) -> Vec<u8> {
    use std::io::Read;
    let file = std::fs::File::open(path).expect("replay input");
    let mut bytes = Vec::new();
    file.take((MAX_INPUT + 1) as u64)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= MAX_INPUT, "seed/replay exceeds input budget");
    bytes
}
pub fn seeds(root: &Path, target: Target) -> Vec<(PathBuf, Vec<u8>)> {
    let mut entries: Vec<_> = std::fs::read_dir(root.join(target.name()))
        .expect("seed directory")
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .collect();
    entries.sort();
    assert!(!entries.is_empty(), "empty seed corpus");
    entries
        .into_iter()
        .map(|path| {
            let bytes = read_bounded(&path);
            assert!(bytes.len() <= MAX_INPUT);
            (path, bytes)
        })
        .collect()
}
