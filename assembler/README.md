# AMD64 scalar assembler forms

These forms run in AMD64 long mode. `assemble` and `assemble_with_limits` use
Whale's parser and encoder directly; neither uses an external assembler.
Instructions and directives end at a newline or EOF. A label may precede its
instruction on the same line. CR, LF, and CRLF are supported. Quoted multiline
strings preserve their source bytes. Token positions index original UTF-8 bytes;
display columns count Unicode scalars, with a tab counting as one column.

```asm
section .text
global example
example:
    mov spl, al
    lea r8, [r9 + rcx*4 + 32]
    test r8, r9
    jne .different
    shl r8, 1
    shr r8, cl
    sar r8, 255
    not r8
    neg r8
    imul r8, r9, -128
.different:
    cqo
    idiv r9
    ret
```

`SPL`, `BPL`, `SIL`, and `DIL` require a REX prefix even with no extension bits.
`AH`, `CH`, `DH`, and `BH` cannot coexist with any REX prefix, including extended
address registers. These checks apply to existing moves and ALU operations, and
the new scalar forms.

| Family | Supported forms and boundaries |
| --- | --- |
| LEA | `lea r32/r64, [address]`, using 64-bit base/index registers and signed 32-bit displacement. Symbolic addresses and 8/16-bit destinations are rejected. Operand size does not select 32-bit address size. |
| TEST | `test reg, reg`, matching 8/16/32/64-bit widths; flags are updated without changing the operands. |
| NOT / NEG | A single register at any scalar width; memory forms are rejected. |
| SHL / SAL / SHR / SAR | Register destination at any scalar width, with count `1`, unsigned immediate `0..255`, or `CL`; memory forms and other count registers are rejected. |
| DIV / IDIV | One register divisor at any scalar width; memory forms are rejected. |
| CWD / CDQ / CQO | No operands; sign extend AX into DX:AX, EAX into EDX:EAX, and RAX into RDX:RAX respectively. |
| IMUL | Existing `imul r16/r32/r64, reg/mem` and new `imul reg, reg, immediate`, with matching widths. The immediate uses signed imm8 when possible, otherwise imm16 for r16 or imm32 for r32/r64. The r64 immediate must fit signed 32 bits because hardware sign extends it. r16/r32 also accept the full unsigned bit pattern for their immediate width. Three-operand memory forms remain unsupported. |

DIV/IDIV consume AX for an 8-bit divisor (quotient AL, remainder AH); DX:AX,
EDX:EAX, or RDX:RAX for wider divisors (quotient AX/EAX/RAX, remainder
DX/EDX/RDX). Divide by zero and quotient overflow raise architectural divide
errors. CWD/CDQ/CQO prepare the signed dividend; unsigned division normally needs
a zero high half. Raw machine instructions do not enforce Whale IR's separate
wrap, trap, or pointer contracts.

Raw shifts use the AMD64 hardware count masks (five bits for 8/16/32-bit operands,
six for 64-bit operands). The assembler preserves the encoded immediate and does
not substitute this machine policy for Whale's IR count-modulo-width semantics.

All integer condition codes have short and near forms. Same-section branches
relax at the inclusive signed-byte displacement limits; cross-section or external
branches carry `Branch32` relocations with a `-4` addend.

| Condition | Mnemonics and aliases |
| --- | --- |
| Overflow / no overflow | JO / JNO |
| Unsigned below / above-or-equal | JB, JC, JNAE / JAE, JNB, JNC |
| Equal / not equal | JE, JZ / JNE, JNZ |
| Unsigned below-or-equal / above | JBE, JNA / JA, JNBE |
| Sign / no sign | JS / JNS |
| Parity / no parity | JP, JPE / JNP, JPO |
| Signed less / greater-or-equal | JL, JNGE / JGE, JNL |
| Signed less-or-equal / greater | JLE, JNG / JG, JNLE |

The exact-byte fixtures in `tests/amd64_scalar_forms.rs` are independent opcode
and addressing expectations. The 13 accepted LEA fixtures were additionally
checked with `objdump -D -b binary -m i386:x86-64 -M intel`; all disassembled to
the intended destination and effective address. Neither objdump nor NASM is a
normal test dependency.
