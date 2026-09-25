# Whale

**A general-purpose compiler toolchain written in Rust.**

[![Rust CI](https://github.com/wavefnd/Whale/actions/workflows/rust.yml/badge.svg?branch=master)](https://github.com/wavefnd/Whale/actions/workflows/rust.yml)
[![Code quality](https://github.com/wavefnd/Whale/actions/workflows/quality.yml/badge.svg?branch=master)](https://github.com/wavefnd/Whale/actions/workflows/quality.yml)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue.svg)](LICENSE)

Whale brings together an intermediate representation, assembler, object-file
library, and linker infrastructure. Developed within the
[Wave ecosystem](https://github.com/wavefnd/Wave), its components are intended to
serve language implementations and compiler tooling as reusable Rust libraries.

Whale is in active, early development. APIs and IR formats are evolving; the
current capabilities are listed below.

[Getting started](#getting-started) · [Usage](#usage) · [Development](#development) · [Contributing](#contributing) · [Support](#support)

## Design goals

- **Defined behavior:** specify program behavior explicitly, including invalid
  operations, with the goal of an IR without undefined behavior.
- **Explicit O0 IR:** keep operations and required safety behavior visible in IR,
  preserving unused operations and unreachable blocks for debugging. O0 is the
  current development priority; O1 and higher optimizations are future work.
  Verification diagnoses invalid IR without removing or simplifying it.
  Explicit constant declarations retain their typed initializer expressions and
  evaluated values, including unused local declarations. Their compile-time
  evaluation does not introduce runtime arithmetic instructions.
- **Reusable components:** expose the IR, assembler, object model, and linker as
  separate crates.

These are project goals. Complete memory-safety semantics and an end-to-end
native compilation pipeline are still being developed.

## Current capabilities

| Component | Available today | Status |
| --- | --- | --- |
| Assembler | AMD64 assembly, sections, symbols, and relocations emitted as ELF64 object files | Available |
| IR | Typed IR construction, printing, verification, and scalar AST JSON lowering | Experimental |
| Object library | Object model and ELF64 relocatable object serialization | Available |
| Object CLI | Wrap raw input bytes in an ELF64 object with a `.text` section | Limited |
| Linker | Initial library infrastructure; `whale link` remains a placeholder | In development |

The IR target selector accepts only `x86_64-whale-linux`; unknown targets fail
with the supported choice, including with `--no-verify`. Its output data layout
is 64-bit little endian on every build host. Library lowering and verification
reject target/layout mismatches. The IR layout API computes checked sizes,
field offsets, array strides, and natural/allocation alignments for this target.
It does not implement aggregate ABI passing or native code generation.

The current emitted object target is AMD64 ELF64. Object metadata records the machine,
format, byte order, and address width; writers and linker inputs reject unsupported
combinations. `ObjectFile::new(ObjectFormat::ELF64)` remains an AMD64 convenience
constructor; explicit identities use `ObjectFile::with_target`, and format access
is now `object.target.format`. An object file is not a linked
executable. CI runs host checks on Linux, Windows, and macOS; running Whale on a
host does not imply support for that host's native object format or instruction
set as an output target.

## Getting started

You need Git and **Rust 1.86.0 or newer**, including Cargo. Stable Rust is
recommended for development.

```sh
git clone https://github.com/wavefnd/Whale.git
cd Whale
cargo build --release --locked
```

The executable is written to `target/release/whale`, or
`target/release/whale.exe` on Windows. The examples below use `cargo run` so that
installing Whale on your `PATH` is optional.

Enable the experimental IR command when building with:

```sh
cargo build --release --locked --features socket-cli
```

## Usage

### Assemble an AMD64 object

Save the following as `example.asm`:

```asm
section .text
global answer

answer:
    mov eax, 42
    ret
```

```sh
cargo run --release --locked -- asm --amd64 example.asm -o example.o
```

This produces a relocatable ELF64 object. Assembly is implemented within Whale;
no external assembler is needed.

### Lower an AST to IR

Save this minimal typed AST as `program.json`:

```json
{
  "globals": [],
  "functions": [{
    "name": "answer",
    "parameters": [],
    "return_type": {"Int": {"bits": 32, "signed": true}},
    "body": [{"Return": {"Lit": {"Int": {
      "bits": 32, "signed": true, "value": 42
    }}}}]
  }]
}
```

```sh
cargo run --release --locked --features socket-cli -- ir lower program.json -o program.wir
```

The command lowers and verifies the module, then writes textual IR to
`program.wir`. Omit `-o program.wir` to print it to standard output. The input
schema is defined in [the frontend AST types](ir/src/lower_ast/frontend.rs).

### Wrap raw bytes in an object

For an existing raw binary file:

```sh
cargo run --release --locked -- object code.bin -o code.o
```

This places the input bytes in an ELF64 `.text` section and defines a global
`start` symbol at offset zero. It does not compile textual IR or disassemble
existing object files.

## Development

| Path | Responsibility |
| --- | --- |
| [assembler/](assembler/) | Assembly parsing and instruction encoding |
| [ir/](ir/) | IR types, builders, lowering, verification, and printing |
| [object/](object/) | Sections, symbols, relocations, and ELF serialization |
| [linker/](linker/) | Symbol resolution and layout infrastructure |
| [src/](src/) | Command-line interface |
| [tests/](tests/) | CLI integration tests |
| [tools/](tools/) | CI helpers and standalone CLI smoke checks |

Run the workspace checks from the repository root:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
```

[GitHub Actions](https://github.com/wavefnd/Whale/actions) also covers native host
configurations, the minimum supported Rust version, optimized tests, rustdoc,
coverage, and scheduled maintenance. The checked-in
[workflows](.github/workflows/) contain the commands used by CI.

## Contributing

Contributions to correctness, diagnostics, tests, and toolchain capabilities are
welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) for setup, validation, and
signed-off commits.

- Find a bounded task in [good first issues](https://github.com/wavefnd/Whale/issues?q=is%3Aissue%20is%3Aopen%20label%3A%22good%20first%20issue%22).
- Follow planned work in the [toolchain backlog](https://github.com/wavefnd/Whale/issues/12).
- [Report a bug or propose a feature](https://github.com/wavefnd/Whale/issues/new/choose).
- See [MAINTAINERS](MAINTAINERS) for review contacts and the
  [Code of Conduct](CODE_OF_CONDUCT.md) for community expectations.

The repository's AI-use policy is recorded in [ai.txt](ai.txt).

## Support

Support development through [Open Collective](https://opencollective.com/wave-lang)
or [GitHub Sponsors](https://github.com/sponsors/LunaStev).

## License

Whale is licensed under the [Mozilla Public License 2.0](LICENSE).
See [COPYRIGHT](COPYRIGHT) and [NOTICE](NOTICE) for attribution and notices.
