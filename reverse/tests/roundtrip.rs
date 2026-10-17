//! The documented canonical subset is tested through the actual Wave decoder.
#[cfg(whale_wave_reverse)]
mod native {
    use amd64::AMD64;
    use assembler::assemble;

    #[test]
    fn canonical_operand_text_reassembles_the_documented_subset_exactly() {
        // These are current Whale encoder outputs with no redundant prefixes,
        // symbolic fields, relative targets or alternative forced encodings.
        // The test does not use the retained raw bytes as assembly input.
        for source in [
            "mov rax,r9",
            "mov ah,bl",
            "mov spl,al",
            "mov r9w,0x1234",
            "mov eax,0x12345678",
            "mov r8,0x123456789abcdef",
            "add rax,r9",
            "sub r8,-1",
            "xor eax,ecx",
            "cmp r10,127",
            "test rax,r9",
            "not r9",
            "neg r8d",
            "imul r9,r10,-7",
            "shl rax,1",
            "shr r9,cl",
            "sar eax,7",
            "mov rax,qword [r12+r9*8-128]",
            "mov dword [rbp+128],r9d",
            "movzx r9d,byte [r12+8]",
            "movss xmm9,xmm10",
            "movsd xmm15,xmm8",
            "movss xmm9,dword [r12+r9*4-128]",
            "movsd qword [rbp+128],xmm15",
            "addss xmm9,xmm10",
            "subsd xmm15,qword [r12+8]",
            "mulss xmm0,dword [rbp-8]",
            "divsd xmm8,xmm9",
            "nop",
            "ret",
        ] {
            let original = assemble(source, &AMD64)
                .unwrap_or_else(|error| panic!("fixture {source:?}: {error}"))
                .sections
                .remove(0)
                .data;
            let decoded = reverse::decode(&original, 0)
                .unwrap_or_else(|error| panic!("decode {source:?}: {error}"));
            assert_eq!(usize::from(decoded.length), original.len(), "{source}");
            let operands = decoded
                .operands
                .iter()
                .map(|operand| reverse::format::operand_text(operand, &decoded, 0))
                .collect::<Vec<_>>()
                .join(", ");
            let rendered = format!("{} {operands}\n", decoded.mnemonic());
            let reassembled = assemble(&rendered, &AMD64)
                .unwrap_or_else(|error| panic!("rendered {source:?} as {rendered:?}: {error}"))
                .sections
                .remove(0)
                .data;
            assert_eq!(reassembled, original, "{source:?} rendered as {rendered:?}");
        }
    }
}
