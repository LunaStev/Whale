# Reverse kernel ABI 1

Every pointer refers to live caller-owned storage for the duration of a call.
Byte lengths describe real readable memory; u64 buffers are naturally aligned.
The kernel allocates nothing and calls no OS/provider functions. It does not
retain pointers. Scalar quantities and u64 records use the host C ABI.

`whale_wave_reverse_abi() -> u64` returns 1.

`whale_wave_reverse_decode(abi, input, length, offset, output, capacity) -> i32`
accepts input bytes and output u64 words. Offset and length must fit the supplied
allocation; lengths above i64::MAX, null pointers, and offset > length are invalid.
Capacity must be at least 96 words. Only successful decoding commits 96 output
words. Every failure leaves the entire output unchanged, including capacity and
ABI failures. Rust checks the capability version before calling the kernel and
validates all returned ranges/identities before constructing typed results.

Status codes: 0 success; 1 invalid argument/known encoding; 2 truncated;
3 unsupported; 4 overlong; 5 output capacity; 6 ABI mismatch.

The 96-word result is zero-initialized, with these occupied fields:

| Words | Meaning |
|---|---|
| 0..1 | ABI version=1, record words=96 |
| 2..6 | byte length, opcode ID, condition ID (16=none), operand count, flow ID |
| 7..9 | prefix count, opcode byte offset, opcode length |
| 10..13 | ModRM offset+1/value, SIB offset+1/value (zero offset means absent) |
| 14..16 | displacement offset+1, encoded byte width, raw bits |
| 17..19 | immediate offset+1, encoded byte width, raw bits |
| 20 | effective REX byte (zero=absent) |
| 24..38 | original prefix bytes in order |
| 40..55, 56..71, 72..87 | three operand records |

Each operand begins with kind and logical bit width. Kind1=GPR,2=XMM: word2
register index, word3 high-byte flag. High-byte GPR indices0..3 mean AH..BH.
Kind3=memory: word2 base (0 absent,1..16 RAX..R15,17 RIP), word3 index (0 absent,
1..16 register+1), word4 scale, word5 signed displacement bits. Kind4=immediate:
word2 raw encoded bits, word3 sign-extension flag, word6 encoded bit width (zero
for implicit one). Kind5=relative: word2 signed displacement bits, word6 encoded
bit width. XMM registers have physical width128; scalar memory width is32/64.

Opcode IDs are the exhaustive ordered mapping in `model.rs::Opcode::from_code`.
Condition IDs0..15 follow the AMD64 condition encoding. Flow IDs0..9 are next,
direct call, indirect call, direct jump, indirect jump, conditional, loop, return,
trap and syscall. Relative flow values are encoded displacements, never resolved
symbols. Reserved words must not acquire new meanings without an ABI revision.
