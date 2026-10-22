use object::*;

fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(b[at..at + 2].try_into().unwrap())
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn fixture(machine: Machine) -> ObjectFile {
    let mut o = ObjectFile::with_target(ObjectTarget::coff(machine));
    o.add_section(".text", SectionKind::Text, 16);
    o.sections[0].data = if machine == Machine::X86_64 {
        vec![0xe8, 0, 0, 0, 0, 0xc3]
    } else {
        0x94000000u32.to_le_bytes().to_vec()
    };
    o.symbols.push(ObjectSymbol {
        name: "entry".into(),
        section_index: Some(0),
        value: 0,
        size: o.sections[0].data.len() as u64,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });
    o.relocations.push(ObjectRelocation {
        section_index: 0,
        offset: usize::from(machine == Machine::X86_64),
        symbol: "external_helper".into(),
        addend: if machine == Machine::X86_64 { -4 } else { 4 },
        kind: if machine == Machine::X86_64 {
            RelocKind::PLT32
        } else {
            RelocKind::CoffSpecific { machine, code: 3 }
        },
    });
    o
}

#[test]
fn coff_headers_symbols_auxiliary_indices_and_in_place_addends_are_explicit() {
    for (machine, code) in [(Machine::X86_64, 0x8664), (Machine::AArch64, 0xaa64)] {
        let o = fixture(machine);
        let b = o.write().unwrap();
        assert_eq!(u16_at(&b, 0), code);
        assert_eq!(u16_at(&b, 2), 1);
        assert_eq!(&b[4..8], &[0; 4]);
        assert_eq!(u16_at(&b, 16), 0);
        assert_eq!(u32_at(&b, 12), 5); // section+aux, function+aux, undefined helper
        let data = u32_at(&b, 40) as usize;
        let reloc = u32_at(&b, 44) as usize;
        assert_eq!(u32_at(&b, reloc), if machine == Machine::X86_64 { 1 } else { 0 });
        assert_eq!(u32_at(&b, reloc + 4), 4);
        assert_eq!(u16_at(&b, reloc + 8), if machine == Machine::X86_64 { 4 } else { 3 });
        assert_eq!(
            u32_at(&b, data + usize::from(machine == Machine::X86_64)),
            if machine == Machine::X86_64 { 0 } else { 0x94000001 }
        );
        let symbols = u32_at(&b, 8) as usize;
        assert_eq!(u32_at(&b, symbols + 18), o.sections[0].data.len() as u32);
        assert_eq!(u32_at(&b, symbols + 36 + 22), o.symbols[0].size as u32);
        let helper = symbols + 72;
        assert_eq!(u16_at(&b, helper + 12), 0);
        assert_eq!(b[helper + 16], 2);
        let strings = symbols + 5 * 18;
        let offset = u32_at(&b, helper + 4) as usize;
        assert_eq!(&b[strings + offset..b.len() - 1], b"external_helper");
        assert_eq!(o.sections[0].data, fixture(machine).sections[0].data);
    }
}

#[test]
fn coff_sparse_bss_long_names_and_exact_output_limits_do_not_materialize_memory() {
    let mut o = ObjectFile::with_target(ObjectTarget::coff(Machine::X86_64));
    o.add_section("/long_section_name", SectionKind::Bss, 8192);
    o.sections[0].zero_fill = u32::MAX.into();
    let bytes = o.write().unwrap();
    assert!(bytes.len() < 256);
    assert_eq!(u32_at(&bytes, 36), u32::MAX);
    assert_eq!(u32_at(&bytes, 40), 0);
    assert_eq!(u32_at(&bytes, 56) & 0xf00000, 0xe00000);
    assert_eq!(bytes, o.write_with_limit(bytes.len() as u64).unwrap());
    let e = o.write_diagnostic_with_limit(bytes.len() as u64 - 1).unwrap_err();
    assert_eq!(e.resource, Some(ObjectResource::OutputBytes));
    o.sections[0].zero_fill += 1;
    assert!(o.write().unwrap_err().contains("section size"));
}

#[test]
fn coff_rejects_target_namespace_overlap_and_unsupported_metadata() {
    let mut o = fixture(Machine::AArch64);
    o.relocations[0].kind = RelocKind::TargetSpecific { machine: Machine::AArch64, code: 283 };
    assert!(o.write().is_err());
    o = fixture(Machine::X86_64);
    o.relocations.push(o.relocations[0].clone());
    assert!(o.write().unwrap_err().contains("overlapping"));
    o = fixture(Machine::X86_64);
    o.symbols[0].binding = SymbolBinding::Weak;
    assert!(o.write().is_err());
    o = fixture(Machine::X86_64);
    o.sections[0].align = 16384;
    assert!(o.write().is_err());
    o = fixture(Machine::AArch64);
    o.relocations[0].addend = 2;
    assert!(o.write().is_err());
    o = fixture(Machine::AArch64);
    o.sections[0].data = [0u8; 4].to_vec();
    assert!(o.write().is_err());
    assert!(ObjectTarget::coff(Machine::RiscV64).validate().is_err());
    assert!(ObjectTarget::coff(Machine::LoongArch64).validate().is_err());
    assert!(ObjectTarget::coff(Machine::X86_64).validate_amd64().is_err());
    assert!(ObjectTarget::coff(Machine::X86_64).elf_machine().is_err());
    assert!(object::formats::elf::write_elf(&fixture(Machine::X86_64)).is_err());
}

#[test]
fn coff_arm64_conditional_and_adr_addends_use_their_own_bitfields() {
    for (code, word, addend, expected) in
        [(15, 0x54000000u32, -4, 0x54ffffe0), (5, 0x10000000, -1, 0x70ffffe0)]
    {
        let mut o = fixture(Machine::AArch64);
        o.sections[0].data = word.to_le_bytes().to_vec();
        o.relocations[0].kind = RelocKind::CoffSpecific { machine: Machine::AArch64, code };
        o.relocations[0].addend = addend;
        let b = o.write().unwrap();
        assert_eq!(u32_at(&b, u32_at(&b, 40) as usize), expected);
    }
}
