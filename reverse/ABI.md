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

## CFG extension

`whale_wave_reverse_cfg(abi, input, length, base, seeds, seed_count, overlays,
overlay_count, options, options_count, workspace, workspace_words, output,
output_words) -> i32` uses disjoint live byte/u64 buffers. It invokes the same
Wave decoder internally; no predecoded host instruction stream supplies traversal.

`options_count` is 6; options are max instructions I, max blocks B, max edges E,
max stops S, max work W, follow-calls (0/1). Seeds are byte offsets. Overlay rows
are four words: field offset, byte width, target kind, target value; target kinds
0=unresolved,1=absolute address,2=host symbol ID. Rows must be sorted/disjoint and
inside input. At most 1,000,000 rows/seeds are accepted by the raw ABI.

Output reserves `T = 16 + 97I + 5B + 6E + 4S` words. Workspace reserves
`length + B + T` words: one ownership word per input byte, the leader queue and
staged output. The kernel rejects insufficient capacities before access. Hard
record ceilings are 1,000,000 instructions/blocks/stops and 4,000,000 edges; input
is at most 16 MiB. All size formulas fit u64 below these fixed ceilings.

The 16-word header contains ABI, T, used instruction/block/edge/stop counts, work,
zero, and four section offsets (instruction,block,edge,stop), then reserved zeros.
Each instruction row is its byte offset followed by the 96-word decode record.
Block rows are start,end,first instruction index,count,zero. Edge rows are source
instruction offset,kind,target kind,target value,zero,zero. Stop rows are offset,
reason,conflicting instruction offset (overlap only),zero. Only used rows and the
header are copied on success; unused capacity is untouched. No failure publishes
any output word. Scratch contents are unspecified after a call.

Additional status codes: 7 work limit,8 instruction limit,9 leader/block limit,
10 edge limit,11 stop limit. Edge kinds 0..11 are fallthrough,call,jump,conditional
true/false,loop true/false,indirect call,indirect jump,syscall,memory,address.
Stop reasons 1..9 are invalid,truncated,unsupported,overlong,overlap,end of range,
indirect jump,syscall,address overflow. Unresolved graph edges are successful
conservative analysis results, distinct from invalid ABI input or budget failure.
