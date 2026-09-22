# Continuous integration

Whale's minimum supported Rust version (MSRV) is **1.86.0**. Every workspace
package inherits this requirement from `workspace.package.rust-version`.
Raise the MSRV only in a dedicated change that updates this declaration, the CI
job, and this guide together, and validates both feature configurations. Versions
below 1.86.0 are not claimed as supported.
Normal development and quality checks use stable Rust. CI records `rustc -vV`
so failures can be tied to the actual compiler version.

## Pull request checks

| Workflow | Checks |
| --- | --- |
| Rust | Native Linux x64/ARM64, Windows x64/ARM64 MSVC, macOS ARM64/Intel; independent default/all-features builds, tests, and standalone CLI smoke checks |
| Rust | Rust 1.86.0 builds and tests with the committed lockfile, independently for default/all features |
| Rust | Release-mode workspace regression tests with all features |
| Rust | Linux RISC-V cross-build and bounded QEMU CLI smoke checks |
| Quality | Separate rustfmt, strict default/all-features Clippy, warning-free public rustdoc, workflow syntax, and CI helper checks |
| Coverage | All-features workspace tests, LCOV, HTML, per-file details, and a per-crate summary |

These workflows run on pull requests to `master`, pushes to `master`, and manual
dispatch. Feature and platform matrix jobs do not cancel their siblings on
failure. The native jobs verify the compiler's host triple to detect accidental
emulation. RISC-V checks the **host running Whale**; the smoke input still emits
AMD64 ELF objects, and does not imply RISC-V code generation support.

The Python smoke script copies the CLI into a temporary directory whose path
contains spaces and runs it with an empty directory as `PATH`. It checks explicit
AMD64 instruction bytes and ELF metadata, fresh-process object/IR determinism,
socket JSON lowering to stdout and a file, rejected input without output
creation/overwrite, and the feature-disabled CLI diagnostic. It uses no external
assembler, linker, or object-inspection tool. QEMU is used only to execute the
RISC-V host binary. This is a built-binary check, not release-archive validation.

Coverage currently has no minimum percentage gate. The report includes workspace
source files with no coverage; missing tests (including in the linker) remain
visible rather than being hidden by exclusions. It covers the Rust test suite;
the separate Python CLI smoke runs are not included in its percentage.

## Scheduled checks

`Maintenance` runs every Monday at 04:23 UTC and can be started manually. Its two
independent jobs check the locked dependencies against RustSec using pinned
`cargo-audit`, and build/test all features on Rust beta. A failed advisory database
fetch is an infrastructure failure, not evidence that dependencies are safe or
vulnerable. There are no blanket advisory ignores. Any temporary ignore must name the
advisory, explain why it applies, and record an expiry date and tracking issue.
To reproduce locally, install `cargo-audit` 0.22.2 with `--locked`, then run
`cargo audit --file Cargo.lock` and `cargo metadata --locked --format-version 1`. Beta failures remain visible
without making beta a required stable pull-request check.

## Reproduce locally

Run from the repository root with Python 3.13 and Rust installed:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --all-features --locked
cargo test --workspace --all-features --release --locked
cargo +1.86.0 test --workspace --locked
cargo +1.86.0 test --workspace --all-features --locked
python -m unittest discover -s tools -p 'test_*.py' -v
cargo build --workspace --locked
python tools/ci_smoke.py --binary target/debug/whale --socket disabled
cargo build --workspace --all-features --locked
python tools/ci_smoke.py --binary target/debug/whale --socket enabled
```

Use `target/debug/whale.exe` on Windows. Set `RUSTDOCFLAGS=-D warnings` in your
shell and run `cargo doc --workspace --all-features --no-deps --locked` for docs.
For RISC-V, install the `riscv64gc-unknown-linux-gnu` Rust target, the GNU cross
compiler, its libc development sysroot, and QEMU user emulation, then run:

```sh
CARGO_TARGET_RISCV64GC_UNKNOWN_LINUX_GNU_LINKER=riscv64-linux-gnu-gcc \
  cargo build --workspace --all-features --locked --target riscv64gc-unknown-linux-gnu
python tools/ci_smoke.py --binary target/riscv64gc-unknown-linux-gnu/debug/whale \
  --socket enabled --emulator /usr/bin/qemu-riscv64 --sysroot /usr/riscv64-linux-gnu
```

For coverage, install `llvm-tools-preview` with rustup and
`cargo install cargo-llvm-cov --version 0.9.1 --locked`, then copy the commands
from `.github/workflows/coverage.yml`. Workflow validation uses actionlint
1.7.12; its download is verified against the committed SHA-256 checksum.
GitHub actions are pinned to commit hashes. Update these pins explicitly and
rerun workflow validation when upgrading tools.

## Failure evidence and permissions

Every workflow declares `contents: read`. Checkouts do not persist credentials,
and pull-request jobs require no repository secrets or write tokens. All jobs
have deadlines and each workflow cancels superseded runs for the same PR/ref.

`tools/ci.py run --name NAME -- COMMAND ...` preserves a command's failing exit
status, bounds its runtime (15 minutes by default), and records command, host,
timing, and exit metadata. Each log retains at most 8 MiB; truncation is recorded
explicitly. A timeout exits with status 124. Test its timeout and failure paths
with `tools/test_ci.py`.

Jobs attempt artifact upload with `if: always()` and retain evidence for seven
days. Artifacts include logs, metadata, CLI transcripts, and failing smoke inputs
and outputs. Coverage adds HTML and LCOV. A failed command is never converted to
success for upload. Hard runner loss or forced cancellation may prevent upload.

Scheduled fuzzing remains tracked in #186 and depends on the harness work in
#91/#101. Versioned release archives, extracted-package validation, checksums,
and gated release publication remain tracked in #187–#190. These need their own
implementation before CI can validate them; this change adds no release or
fuzzing placeholders.
