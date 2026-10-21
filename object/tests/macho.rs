use object::*;

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(b[at..at + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(b[at..at + 8].try_into().unwrap())
}

fn fixture(machine: Machine) -> ObjectFile {
    let mut o = ObjectFile::with_target(ObjectTarget::macho64(machine));
    o.add_section(".text", SectionKind::Text, 16);
    o.sections[0].data = if machine == Machine::X86_64 {
        vec![0xe8, 0, 0, 0, 0, 0xc3]
    } else {
        0x94000000u32.to_le_bytes().to_vec()
    };
    o.symbols.push(ObjectSymbol {
        name: "_entry".into(),
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
        symbol: "_helper".into(),
        addend: if machine == Machine::X86_64 { -4 } else { 0 },
        kind: RelocKind::MachSpecific { machine, code: 2 },
    });
    o
}

#[test]
fn amd64_macho_header_load_commands_symbols_and_cross_object_branch_are_explicit() {
    let o = fixture(Machine::X86_64);
    let b = o.write().unwrap();
    assert_eq!(u32_at(&b, 0), 0xfeedfacf);
    assert_eq!(u32_at(&b, 4), 0x01000007);
    assert_eq!(u32_at(&b, 8), 3);
    assert_eq!(u32_at(&b, 12), 1);
    assert_eq!(u32_at(&b, 16), 4);
    assert_eq!(u32_at(&b, 20), 280);
    assert_eq!(u32_at(&b, 32), 0x19);
    assert_eq!(u32_at(&b, 36), 152);
    assert!(u64_at(&b, 80) <= u64_at(&b, 64));
    assert_eq!(&b[104..110], b"__text");
    assert_eq!(&b[120..126], b"__TEXT");
    assert_eq!(u64_at(&b, 144), 6);
    let data = u32_at(&b, 152) as usize;
    assert_eq!(&b[data..data + 6], &[0xe8, 0, 0, 0, 0, 0xc3]);
    let reloc = u32_at(&b, 160) as usize;
    assert_eq!(u32_at(&b, 164), 1);
    assert_eq!(u32_at(&b, reloc), 1);
    assert_eq!(u32_at(&b, reloc + 4), 0x2d000001);
    let sym = 184;
    assert_eq!(u32_at(&b, sym), 2);
    assert_eq!(u32_at(&b, sym + 12), 2);
    let table = u32_at(&b, sym + 8) as usize;
    assert_eq!(b[table + 4], 0x0f);
    assert_eq!(b[table + 5], 1);
    assert_eq!(b[table + 20], 1);
    assert_eq!(b[table + 21], 0);
    assert_eq!(u64_at(&b, table + 24), 0);
    assert_eq!(&o.sections[0].data, &fixture(Machine::X86_64).sections[0].data);
    assert_eq!(b, o.write_with_limit(b.len() as u64).unwrap());
    assert_eq!(
        o.write_diagnostic_with_limit(b.len() as u64 - 1).unwrap_err().resource,
        Some(ObjectResource::OutputBytes)
    );
}

#[test]
fn arm64_cpu_branch_namespace_and_absolute_pointer_addends_are_independent() {
    let mut o = fixture(Machine::AArch64);
    let data = o.add_section(".data", SectionKind::Data, 8);
    o.sections[data].data = vec![0; 8];
    o.relocations.push(ObjectRelocation {
        section_index: data,
        offset: 0,
        symbol: "_payload".into(),
        addend: 4,
        kind: RelocKind::Absolute64,
    });
    let b = o.write().unwrap();
    assert_eq!(u32_at(&b, 4), 0x0100000c);
    assert_eq!(u32_at(&b, 8), 0);
    let text = u32_at(&b, 152) as usize;
    assert_eq!(u32_at(&b, text), 0x94000000);
    let reloc = u32_at(&b, 160) as usize;
    assert_eq!(u32_at(&b, reloc + 4), 0x2d000001);
    let data = u32_at(&b, 232) as usize;
    assert_eq!(u64_at(&b, data), 4);
    let reloc = u32_at(&b, 240) as usize;
    assert_eq!(u32_at(&b, reloc + 4), 0x0e000002);
    for kind in [
        RelocKind::TargetSpecific { machine: Machine::AArch64, code: 283 },
        RelocKind::CoffSpecific { machine: Machine::AArch64, code: 3 },
        RelocKind::MachSpecific { machine: Machine::X86_64, code: 2 },
        RelocKind::MachSpecific { machine: Machine::AArch64, code: 3 },
        RelocKind::Absolute32,
    ] {
        o.relocations[0].kind = kind;
        assert!(o.write().is_err(), "{kind:?}");
    }
    o = fixture(Machine::AArch64);
    o.relocations[0].addend = 4;
    assert!(o.write().is_err());
    o.relocations[0].addend = 0;
    o.sections[0].data = vec![0; 4];
    assert!(o.write().is_err());
}

#[test]
fn sparse_bss_and_symbol_partitions_use_virtual_addresses_without_payloads() {
    let mut o = fixture(Machine::X86_64);
    let bss = o.add_section(".bss", SectionKind::Bss, 4096);
    o.sections[bss].zero_fill = 1 << 40;
    o.symbols.push(ObjectSymbol {
        name: "local_bss".into(),
        section_index: Some(bss),
        value: 8,
        size: 8,
        binding: SymbolBinding::Local,
        kind: SymbolKind::Object,
        visibility: SymbolVisibility::Default,
    });
    let b = o.write().unwrap();
    assert!(b.len() < 1024);
    assert_eq!(u64_at(&b, 224), 1 << 40);
    assert_eq!(u32_at(&b, 232), 0);
    assert_eq!(u32_at(&b, 248), 1);
    let syms = 264;
    assert_eq!(u32_at(&b, syms), 2);
    let at = u32_at(&b, syms + 8) as usize;
    assert_eq!(b[at + 4], 0x0e);
    assert_eq!(b[at + 5], 2);
    assert_eq!(u64_at(&b, at + 8), 4104);
    let dy = syms + 24;
    assert_eq!((u32_at(&b, dy + 12), u32_at(&b, dy + 20), u32_at(&b, dy + 28)), (1, 1, 1));
    let rel = u32_at(&b, 160) as usize;
    assert_eq!(u32_at(&b, rel + 4) & 0x00ffffff, 2);
}

#[test]
fn macho_rejects_overlaps_unsupported_metadata_names_and_other_machines() {
    let mut o = fixture(Machine::X86_64);
    o.relocations.push(o.relocations[0].clone());
    assert!(o.write().unwrap_err().contains("overlapping"));
    o = fixture(Machine::X86_64);
    o.symbols[0].binding = SymbolBinding::Weak;
    assert!(o.write().is_err());
    o = fixture(Machine::X86_64);
    o.symbols[0].visibility = SymbolVisibility::Hidden;
    assert!(o.write().is_err());
    o = fixture(Machine::X86_64);
    o.sections[0].name = "x".repeat(17);
    assert!(o.write().is_err());
    o = fixture(Machine::X86_64);
    o.add_section("__text", SectionKind::Text, 1);
    assert!(o.write().is_err());
    o = fixture(Machine::X86_64);
    o.relocations[0].kind = RelocKind::GOTPCREL;
    assert!(o.write().is_err());
    for machine in [Machine::RiscV64, Machine::LoongArch64] {
        assert!(ObjectTarget::macho64(machine).validate().is_err());
    }
    assert!(ObjectTarget::macho64(Machine::X86_64).validate_amd64().is_err());
    assert!(ObjectTarget::macho64(Machine::X86_64).elf_machine().is_err());
}
