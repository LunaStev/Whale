use reverse::*;

#[test]
fn absent_core_has_an_explicit_capability_error() {
    if !backend_available() {
        assert_eq!(decode(&[0x90], 0), Err(DecodeError::CoreUnavailable));
        assert_eq!(backend_name(), "unavailable");
    }
}

#[cfg(whale_wave_reverse)]
mod native {
    use super::*;

    use assembler::{assemble, isa::AMD64};

    fn one(bytes: &[u8]) -> Instruction {
        let instruction = decode(bytes, 0).unwrap_or_else(|e| panic!("{bytes:02x?}: {e}"));
        assert_eq!(instruction.length as usize, bytes.len());
        assert_eq!(instruction.original_bytes(), bytes);
        instruction
    }

    fn reg(operand: Operand, name: &str) {
        let Operand::Register(r) = operand else { panic!("expected {name}: {operand:?}") };
        assert_eq!(r.name(), name);
    }

    fn encoded(source: &str) -> Vec<u8> {
        assemble(source, &AMD64).unwrap_or_else(|e| panic!("{source}: {e}")).sections.remove(0).data
    }

    #[test]
    fn manual_integer_fields_preserve_widths_signedness_and_high_byte_rules() {
        let x = one(&[0x48, 0xb8, 0xef, 0xcd, 0xab, 0x89, 0x67, 0x45, 0x23, 0x01]);
        assert_eq!(x.opcode, Opcode::Mov);
        reg(x.operands[0], "rax");
        assert_eq!(
            x.encoding.immediate,
            Some(EncodingField { offset: 2, width: 8, bits: 0x0123456789abcdef })
        );
        let x = one(&[0x66, 0x41, 0xb9, 0x34, 0x12]);
        reg(x.operands[0], "r9w");
        assert_eq!(x.encoding.immediate.unwrap().width, 2);
        let x = one(&[0x48, 0xc7, 0xc0, 0xff, 0xff, 0xff, 0xff]);
        let Operand::Immediate(i) = x.operands[1] else { panic!() };
        assert_eq!(
            (i.width, i.encoded_width, i.sign_extended, i.signed_value()),
            (64, 32, true, -1)
        );
        for (bytes, dst, src) in [
            (vec![0x88, 0xe0], "al", "ah"),
            (vec![0x40, 0x88, 0xe0], "al", "spl"),
            (vec![0x41, 0x88, 0xe0], "r8b", "spl"),
            (vec![0x0f, 0xb6, 0xc4], "eax", "ah"),
            (vec![0x44, 0x0f, 0xb6, 0xc4], "r8d", "spl"),
            // An intervening legacy prefix cancels the earlier REX.
            (vec![0x40, 0x66, 0x88, 0xe0], "al", "ah"),
        ] {
            let x = one(&bytes);
            reg(x.operands[0], dst);
            reg(x.operands[1], src);
        }
        for (bytes, name, width) in [
            (vec![0x50], "rax", 64),
            (vec![0x41, 0x50], "r8", 64),
            (vec![0x66, 0x41, 0x50], "r8w", 16),
            (vec![0x48, 0x58], "rax", 64),
        ] {
            let x = one(&bytes);
            reg(x.operands[0], name);
            let Operand::Register(r) = x.operands[0] else { panic!() };
            assert_eq!(r.width, width);
        }
        let x = one(&[0x66, 0x68, 0, 0x80]);
        assert!(matches!(
            x.operands[0],
            Operand::Immediate(Immediate {
                width: 16,
                encoded_width: 16,
                sign_extended: false,
                ..
            })
        ));
        let x = one(&[0x6a, 0xff]);
        let Operand::Immediate(i) = x.operands[0] else { panic!() };
        assert_eq!((i.width, i.encoded_width, i.signed_value()), (64, 8, -1));
        let x = one(&[0x48, 0xd1, 0xe0]);
        assert!(x.encoding.immediate.is_none());
        assert!(matches!(
            x.operands[1],
            Operand::Immediate(Immediate { value: 1, encoded_width: 0, .. })
        ));
    }

    #[test]
    fn manual_sib_rip_absolute_and_rex_sentinels_are_distinct() {
        let x = one(&[0x4b, 0x8b, 0x44, 0xcc, 0x80]);
        reg(x.operands[0], "rax");
        let Operand::Memory(m) = x.operands[1] else { panic!() };
        assert_eq!(m.base.unwrap().name(), "r12");
        assert_eq!(m.index.unwrap().name(), "r9");
        assert_eq!((m.scale, m.displacement, m.rip_relative), (8, -128, false));
        assert_eq!(x.encoding.modrm, Some(EncodingByte { offset: 2, value: 0x44 }));
        assert_eq!(x.encoding.sib, Some(EncodingByte { offset: 3, value: 0xcc }));
        assert_eq!(
            x.encoding.displacement,
            Some(EncodingField { offset: 4, width: 1, bits: 0x80 })
        );
        for rex in [0x48, 0x49] {
            let x = one(&[rex, 0x8b, 0x05, 0xfc, 0xff, 0xff, 0xff]);
            let Operand::Memory(m) = x.operands[1] else { panic!() };
            assert!(m.rip_relative);
            assert!(m.base.is_none());
            assert_eq!(m.displacement, -4);
            let x = one(&[rex, 0x8b, 0x04, 0x25, 0xfc, 0xff, 0xff, 0xff]);
            let Operand::Memory(m) = x.operands[1] else { panic!() };
            assert!(!m.rip_relative);
            assert!(m.base.is_none());
            assert!(m.index.is_none());
            assert_eq!(m.displacement, -4);
        }
        let x = one(&[0x4a, 0x8b, 0x04, 0x25, 0, 0, 0, 0]);
        let Operand::Memory(m) = x.operands[1] else { panic!() };
        assert_eq!(m.index.unwrap().name(), "r12");
    }

    #[test]
    fn direct_indirect_conditional_and_loop_flow_keep_the_encoding_target_contract() {
        for (bytes, flow, displacement, width) in [
            (vec![0xe8, 0xfb, 0xff, 0xff, 0xff], "call", -5, 32),
            (vec![0xeb, 0xfe], "jmp", -2, 8),
            (vec![0x0f, 0x85, 1, 0, 0, 0], "jne", 1, 32),
            (vec![0xe2, 0xfc], "loop", -4, 8),
        ] {
            let x = one(&bytes);
            assert_eq!(x.mnemonic(), flow);
            assert_eq!(x.flow.relative(), Some(displacement));
            assert!(
                matches!(x.operands[0],Operand::Relative(r) if r.displacement==displacement && r.encoded_width==width)
            );
        }
        let x = one(&[0x41, 0xff, 0xd0]);
        assert_eq!(x.flow, Flow::Call { relative: None });
        reg(x.operands[0], "r8");
        let x = one(&[0xff, 0x25, 0, 0, 0, 0]);
        assert_eq!(x.flow, Flow::Jump { relative: None });
        assert_eq!(one(&[0xc3]).flow, Flow::Return);
        assert_eq!(one(&[0xcc]).flow, Flow::Trap);
        assert_eq!(one(&[0x0f, 5]).flow, Flow::Syscall);
        assert_eq!(one(&[0xeb, 0xfe]).relative_target(0x1000), Some(0x1000));
        assert_eq!(one(&[0xeb, 0xfc]).relative_target(0), None);
    }

    #[test]
    fn scalar_sse_uses_mandatory_prefixes_xmm_registers_and_scalar_memory_width() {
        for (prefix, precision) in [(0xf3, 32), (0xf2, 64)] {
            for (opcode, mnemonic) in
                [(0x10, "mov"), (0x58, "add"), (0x5c, "sub"), (0x59, "mul"), (0x5e, "div")]
            {
                let suffix = if precision == 32 { "ss" } else { "sd" };
                let x = one(&[prefix, 0x45, 0x0f, opcode, 0xca]);
                assert_eq!(x.mnemonic(), format!("{mnemonic}{suffix}"));
                reg(x.operands[0], "xmm9");
                reg(x.operands[1], "xmm10");
                let x = one(&[prefix, 0x47, 0x0f, opcode, 0x4c, 0xcc, 0x80]);
                reg(x.operands[0], "xmm9");
                let Operand::Memory(m) = x.operands[1] else { panic!() };
                assert_eq!(m.width, precision);
                assert_eq!(m.base.unwrap().name(), "r12");
                assert_eq!(m.index.unwrap().name(), "r9");
            }
            let x = one(&[prefix, 0x45, 0x0f, 0x11, 0x7d, 8]);
            let Operand::Memory(m) = x.operands[0] else { panic!() };
            assert_eq!(m.width, precision);
            reg(x.operands[1], "xmm15");
        }
        for bytes in [
            &[0x0f, 0x10, 0xc0][..],
            &[0x66, 0x0f, 0x58, 0xc0][..],
            &[0xf3, 0x90][..],
            &[0xc5, 0xfa, 0x10, 0xc0][..],
        ] {
            assert!(matches!(decode(bytes, 0), Err(DecodeError::Unsupported { .. })));
        }
    }

    #[test]
    fn every_integer_encoder_family_and_width_decodes_without_a_rust_fallback() {
        let mut cases = vec![
            "ret".into(),
            "nop".into(),
            "syscall".into(),
            "int3".into(),
            "cwd".into(),
            "cdq".into(),
            "cqo".into(),
            "push -1".into(),
            "push 128".into(),
            "push r8".into(),
            "pop r15w".into(),
            "call rax".into(),
            "jmp qword [r12+r9*8+128]".into(),
            "lea r9,[r12+r13*4-129]".into(),
        ];
        for (a, b, width) in [
            ("al", "cl", 8),
            ("ax", "cx", 16),
            ("eax", "ecx", 32),
            ("rax", "rcx", 64),
            ("r8b", "r9b", 8),
            ("r8w", "r9w", 16),
            ("r8d", "r9d", 32),
            ("r8", "r9", 64),
        ] {
            let size = match width {
                8 => "byte",
                16 => "word",
                32 => "dword",
                _ => "qword",
            };
            for mnemonic in ["mov", "add", "sub", "and", "or", "xor", "cmp"] {
                cases.push(format!("{mnemonic} {a},{b}"));
                cases.push(format!("{mnemonic} {a},127"));
                cases.push(format!("{mnemonic} {a},128"));
                cases.push(format!("{mnemonic} {a},{size} [r12+r9*8-128]"));
                cases.push(format!("{mnemonic} {size} [r13+128],{a}"));
                cases.push(format!("{mnemonic} {size} [r13],1"));
            }
            for mnemonic in ["adc", "sbb", "test"] {
                cases.push(format!("{mnemonic} {a},{b}"));
            }
            for mnemonic in ["not", "neg", "div", "idiv"] {
                cases.push(format!("{mnemonic} {a}"));
            }
            for mnemonic in ["shl", "sal", "shr", "sar"] {
                for count in ["1", "cl", "255"] {
                    cases.push(format!("{mnemonic} {a},{count}"));
                }
            }
            if width != 8 {
                cases.push(format!("imul {a},{b}"));
                cases.push(format!("imul {a},{b},-1"));
                cases.push(format!("imul {a},{b},128"));
                cases.push(format!("imul {a},{size} [r12+8]"));
                for condition in [
                    "o", "no", "b", "ae", "e", "ne", "be", "a", "s", "ns", "p", "np", "l", "ge",
                    "le", "g",
                ] {
                    cases.push(format!("cmov{condition} {a},{b}"));
                }
                for mnemonic in ["movzx", "movsx"] {
                    cases.push(format!("{mnemonic} {a},byte [r12]"));
                }
            }
        }
        for condition in
            ["o", "no", "b", "ae", "e", "ne", "be", "a", "s", "ns", "p", "np", "l", "ge", "le", "g"]
        {
            cases.push(format!("set{condition} r9b"));
            cases.push(format!("set{condition} byte [r12+127]"));
        }
        cases.extend([
            "movsxd r8,r9d".into(),
            "movsx r8d,word [r9]".into(),
            "movzx r8,word [r12]".into(),
            "mov ah,cl".into(),
            "mov spl,al".into(),
        ]);
        for source in cases {
            let bytes = encoded(&source);
            let instruction = one(&bytes);
            let first = source.split_whitespace().next().unwrap();
            let expected = if first == "sal" { "shl" } else { first };
            assert_eq!(instruction.mnemonic(), expected, "{source}");
            for n in 0..bytes.len() {
                assert!(
                    matches!(decode(&bytes[..n], 0), Err(DecodeError::Truncated { .. })),
                    "{source} truncated{n}: {:?}",
                    decode(&bytes[..n], 0)
                );
            }
        }
        for source in ["jmp done\ndone:", "call done\ndone:", "je done\ndone:", "loop done\ndone:"]
        {
            one(&encoded(source));
        }
    }

    #[test]
    fn every_sib_base_index_scale_and_displacement_boundary_is_bounded() {
        let names = [
            "rax", "rcx", "rdx", "rbx", "rsp", "rbp", "rsi", "rdi", "r8", "r9", "r10", "r11",
            "r12", "r13", "r14", "r15",
        ];
        for base in names {
            for index in names {
                if index == "rsp" {
                    continue;
                }
                for scale in [1, 2, 4, 8] {
                    for displacement in [-129, -128, 0, 127, 128] {
                        let source = format!("mov rax,[{base}+{index}*{scale}{displacement:+}]");
                        let instruction = one(&encoded(&source));
                        let Operand::Memory(m) = instruction.operands[1] else { panic!() };
                        assert_eq!(m.base.unwrap().name(), base, "{source}");
                        assert_eq!(m.index.unwrap().name(), index, "{source}");
                        assert_eq!((m.scale, m.displacement), (scale, displacement));
                    }
                }
            }
        }
    }

    #[test]
    fn statuses_prefix_order_offset_and_fifteen_byte_limit_are_explicit() {
        assert!(matches!(decode(&[0x8d, 0xc0], 0), Err(DecodeError::Invalid { .. })));
        assert!(matches!(decode(&[0x67, 0x90], 0), Err(DecodeError::Unsupported { .. })));
        assert!(matches!(decode(&[0x41, 0x90], 0), Err(DecodeError::Unsupported { .. })));
        assert!(matches!(decode(&[0xf3, 0x90], 0), Err(DecodeError::Unsupported { .. })));
        assert!(matches!(decode(&[0x90], 2), Err(DecodeError::Invalid { .. })));
        assert!(matches!(decode(&[0x90], 1), Err(DecodeError::Truncated { .. })));
        let mut longest = vec![0x66; 14];
        longest.push(0x90);
        one(&longest);
        longest.insert(0, 0x66);
        assert!(matches!(decode(&longest, 0), Err(DecodeError::Overlong { .. })));
        assert!(matches!(decode(&[0x66; 15], 0), Err(DecodeError::Overlong { .. })));
        let x = decode(&[0xcc, 0x48, 0x89, 0xd8, 0xc3], 1).unwrap();
        assert_eq!(x.offset, 1);
        assert_eq!(x.original_bytes(), [0x48, 0x89, 0xd8]);
    }

    #[test]
    fn raw_abi_capacity_version_and_every_failed_decode_leave_output_unchanged() {
        extern "C" {
            fn whale_wave_reverse_decode(
                abi: u64,
                data: *const u8,
                len: u64,
                offset: u64,
                output: *mut u64,
                capacity: u64,
            ) -> i32;
        }
        for (bytes, abi, offset, cap, expected) in [
            (vec![0x90], 0, 0, 96, 6),
            (vec![0x90], 1, 0, 95, 5),
            (vec![0x90], 1, 2, 96, 1),
            (vec![0x0f], 1, 0, 96, 2),
            (vec![0xc5], 1, 0, 96, 3),
            (vec![0x66; 15], 1, 0, 96, 4),
            (vec![0x8d, 0xc0], 1, 0, 96, 1),
        ] {
            let mut output = [u64::MAX; 96];
            // SAFETY: live, aligned caller-owned buffers; invalid scalar arguments
            // exercise the ABI's documented rejection-before-publication path.
            let status = unsafe {
                whale_wave_reverse_decode(
                    abi,
                    bytes.as_ptr(),
                    bytes.len() as u64,
                    offset,
                    output.as_mut_ptr(),
                    cap,
                )
            };
            assert_eq!(status, expected);
            assert_eq!(output, [u64::MAX; 96]);
        }
        let input = [0x90u8];
        let mut output = [u64::MAX; 96];
        // SAFETY: null and oversized scalar arguments must be rejected before
        // any pointer arithmetic/access; the sentinel output is valid.
        unsafe {
            assert_eq!(
                whale_wave_reverse_decode(1, std::ptr::null(), 0, 0, output.as_mut_ptr(), 96),
                1
            );
            assert_eq!(
                whale_wave_reverse_decode(1, input.as_ptr(), 1, 0, std::ptr::null_mut(), 96),
                1
            );
            assert_eq!(
                whale_wave_reverse_decode(1, input.as_ptr(), u64::MAX, 0, output.as_mut_ptr(), 96),
                1
            );
        }
        assert_eq!(output, [u64::MAX; 96]);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn actual_kernel_reads_never_cross_a_protected_input_boundary() {
        use std::ffi::c_void;

        extern "C" {
            fn mmap(
                address: *mut c_void,
                length: usize,
                protection: i32,
                flags: i32,
                fd: i32,
                offset: i64,
            ) -> *mut c_void;

            fn mprotect(address: *mut c_void, length: usize, protection: i32) -> i32;

            fn munmap(address: *mut c_void, length: usize) -> i32;

            fn sysconf(name: i32) -> i64;

            fn whale_wave_reverse_decode(
                abi: u64,
                data: *const u8,
                len: u64,
                offset: u64,
                output: *mut u64,
                capacity: u64,
            ) -> i32;
        }
        // SAFETY: one private mapping is retained until all calls finish. Every
        // input ends at the protected page, including zero-length input.
        unsafe {
            let page = usize::try_from(sysconf(30)).unwrap();
            assert!(page > 0);
            let mapping = mmap(std::ptr::null_mut(), page * 2, 3, 0x22, -1, 0);
            assert_ne!(mapping as isize, -1);
            assert_eq!(mprotect(mapping.cast::<u8>().add(page).cast(), page, 0), 0);
            let mut random = 0x12345678u32;
            for length in 0..=31 {
                for _ in 0..100 {
                    let data = mapping.cast::<u8>().add(page - length);
                    for i in 0..length {
                        random = random.wrapping_mul(1664525).wrapping_add(1013904223);
                        data.add(i).write((random >> 24) as u8);
                    }
                    let mut output = [u64::MAX; 96];
                    let status = whale_wave_reverse_decode(
                        1,
                        data,
                        length as u64,
                        0,
                        output.as_mut_ptr(),
                        96,
                    );
                    assert!((0..=4).contains(&status));
                    if status == 0 {
                        assert!(output[2] > 0 && output[2] <= 15 && output[2] <= length as u64);
                    } else {
                        assert_eq!(output, [u64::MAX; 96]);
                    }
                }
            }
            assert_eq!(munmap(mapping, page * 2), 0);
        }
    }
}
