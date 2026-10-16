# Wave AMD64 reverse core

The decoder is implemented in `wave/amd64.wave`. Rust owns byte buffers and typed
results; it does not decode instructions as a fallback. Ordinary Cargo builds
have no Wave compiler/network dependency and return `DecodeError::CoreUnavailable`.
`backend_available()` and `backend_name()` expose that capability explicitly.

Build the optional Linux x86-64 core from the official pinned Wave source tree:

```sh
LLVM_SYS_211_PREFIX=/usr/lib64/llvm21 python3 tools/build_wave_reverse.py \
  --wave-source /path/to/Wave --out-dir /tmp/whale-wave-reverse \
  --rust-toolchain 1.89.0
WHALE_WAVE_REVERSE_DIR=/tmp/whale-wave-reverse \
  cargo test -p reverse --features wave --locked --jobs 2
```

The helper reads the exact official commit
`e0952ed65d07e14c6ed0aa64f3c9a5532dd7533b` using `git archive`. It never changes the
supplied checkout or trusts its working tree or installed compiler. The temporary
source archive and matching std are used together, with `WAVE_STD_REVISION` set to
the immutable revision. Cargo is locked/offline unless `--allow-network` is
explicitly selected. LLVM 21 and Rust 1.89.0 are bootstrap prerequisites. An
installed toolchain alias can be passed if it reports the exact rustc version.
Wave objects use explicit PIC relocation so optimized jump tables link into Rust
PIE executables. Optimized objects may reference compiler-lowered `memcpy` and
`memset`; these are memory primitives supplied by the Rust host runtime, not
allocator or OS imports. Archive publication is atomic; provenance records source/compiler/archive hashes.
A root `wave-reverse` feature enables this optional capability in the CLI.

This is an AMD64 whitelist, not a complete x86 decoder. Supported families are:

- Integer MOV, ADD/OR/ADC/SBB/AND/SUB/XOR/CMP register/rm forms and the supported
  group immediates; MOVZX/MOVSX/MOVSXD, LEA, TEST register/rm, IMUL two/three operand.
- NOT/NEG/DIV/IDIV; SHL/SHR/SAR with implicit one, CL and imm8; CWD/CDQ/CQO.
- PUSH/POP registers, PUSH immediate; near direct/indirect CALL/JMP; all 16 Jcc,
  SETcc and CMOVcc conditions; LOOP; RET, NOP, SYSCALL and INT3.
- MOVSS/MOVSD register and scalar memory moves; ADDSS/ADDSD, SUBSS/SUBSD,
  MULSS/MULSD and DIVSS/DIVSD. Mandatory F3/F2, XMM0..15 and REX R/X/B are retained.
  Packed/vector instructions, conversions and VEX/EVEX are unsupported.

The catalog includes every integer form emitted by the current encoder and the
selected scalar SSE forms. It also accepts the same supported r/m opcode forms
where the encoder currently exposes only registers. Unknown families, address32,
segment/LOCK prefixes, and noncanonical combinations of 66 with mandatory scalar
SSE F2/F3 are explicit `Unsupported` outcomes. Invalid known constraints such as
register-source LEA are distinct `Invalid` outcomes. An unsupported first opcode
never receives a guessed length.

All reads stay inside both the supplied slice and the 15-byte instruction limit.
Missing bytes in a recognized form are `Truncated`; a known required field that
cannot fit inside 15 bytes is `Overlong`. Fifteen prefix bytes are overlong even
if no sixteenth input byte is available. Legacy prefixes after REX cancel the
earlier REX, and only the effective REX changes AH/CH/DH/BH to SPL/BPL/SIL/DIL.
Stack-register operations default to 64 bits, with 66 selecting 16 bits. SIB and
RIP-relative no-base sentinels do not turn into R13 merely because REX.B is set.

`Instruction` retains the exact original bytes, operand/condition identities,
logical widths, encoded immediate/displacement fields and their byte offsets.
Immediate raw bits and sign extension are explicit. An encoded field width is
in bytes; an operand's `encoded_width` is in bits. RIP-relative and absolute SIB
memory operands remain distinct. `relative_target(base)` uses checked arithmetic;
relocation overlays belong to the host and must precede target interpretation.
No executable loading, source recovery, IR lifting or native Whale backend is
implied by this crate. The archive is generated through Wave's LLVM bootstrap.

## Images and listing

`image::Image` borrows code bytes and distinguishes region-relative offsets,
file offsets and virtual addresses. ET_REL uses the bounded object importer and
retains symbolic RELA annotations. ET_EXEC uses checked ELF header access and
executable PT_LOAD file ranges, including files without section tables. Memory-only
BSS tails are never copied or decoded. Dynamic/PIE images are outside this subset.

`whale reverse inspect file --json` works without the Wave archive. `whale disasm`
requires the linked Wave core, emits typed facts as JSON or a human listing, and
preserves unsupported/truncated bytes as data records. A linear listing does not
establish reachability after an unknown encoding. Executable listing starts at
its entry in that region; `--entry NAME|ADDRESS` selects a particular start.
Raw bytes require `--raw --amd64 --base ADDRESS`. Input/output/record budgets are
explicit; file output publishes atomically after complete success.

Text rendering is canonical presentation, not a promise of identical reassembly
for every valid x86 encoding. Original bytes and exact encoding fields remain
available. Relocations suppress the placeholder numeric branch target and carry
symbol, addend, kind and field location independently.
