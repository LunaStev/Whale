use object::{ObjectFile, ObjectFormat, SectionKind};
fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[test]
fn section_types_flags_payloads_and_boundaries_are_independent_expectations() {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    for (name, kind, align, data) in [
        (".text", SectionKind::Text, 16, vec![0xc3]),
        (".rodata", SectionKind::ReadOnlyData, 8, vec![1, 2, 3]),
        (".data", SectionKind::Data, 4, vec![4, 5, 6, 7]),
        (".bss", SectionKind::Bss, 32, vec![0; 33]),
    ] {
        let index = object.add_section(name, kind, align);
        object.sections[index].data = data;
    }
    let elf = object.write().unwrap();
    let table = u64_at(&elf, 40) as usize;
    for (i, (kind, flags, align, size, offset)) in [
        (1, 6, 16, 1, 64),
        (1, 2, 8, 3, 72),
        (1, 3, 4, 4, 76),
        (8, 3, 32, 33, 96),
    ]
    .into_iter()
    .enumerate()
    {
        let section = table + (i + 1) * 64;
        assert_eq!(u32_at(&elf, section + 4), kind);
        assert_eq!(u64_at(&elf, section + 8), flags);
        assert_eq!(u64_at(&elf, section + 48), align);
        assert_eq!(u64_at(&elf, section + 32), size);
        assert_eq!(u64_at(&elf, section + 24), offset);
    }
    assert_eq!(&elf[72..75], &[1, 2, 3]);
    assert_eq!(&elf[76..80], &[4, 5, 6, 7]);
    // BSS does not contribute its memory size to the file cursor.
    let symtab = table + 5 * 64;
    assert_eq!(u64_at(&elf, symtab + 24), 80);
}

#[test]
fn sparse_bss_uses_logical_symbol_ranges_without_growing_the_elf_file() {
    use object::{ObjectSymbol, SymbolBinding, SymbolVisibility};
    let make = |size| {
        let mut object = ObjectFile::new(ObjectFormat::ELF64);
        let index = object.add_section(".bss", SectionKind::Bss, 4096);
        object.sections[index].zero_fill = size;
        object.symbols.push(ObjectSymbol {
            name: "end".into(),
            section_index: Some(index),
            value: size,
            size: 0,
            binding: SymbolBinding::Global,
            visibility: SymbolVisibility::Default,
        });
        object
    };
    let small = make(1).write().unwrap();
    let mut object = make(1 << 40);
    assert!(object.sections[0].data.is_empty());
    let large = object.write().unwrap();
    assert_eq!(large.len(), small.len());
    assert!(large.len() < 1024);
    let table = u64_at(&large, 40) as usize;
    assert_eq!(u64_at(&large, table + 64 + 32), 1 << 40);
    object.symbols[0].size = 1;
    assert!(object.write().unwrap_err().contains("symbol range"));
}

#[test]
fn section_storage_invariants_reject_mixed_data_and_overflow() {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    let index = object.add_section(".data", SectionKind::Data, 1);
    object.sections[index].zero_fill = 1;
    assert!(object.write().unwrap_err().contains("only valid for BSS"));
    object.sections[index].kind = SectionKind::Bss;
    object.sections[index].data = vec![1];
    assert!(object.write().unwrap_err().contains("nonzero initializer"));
    object.sections[index].data = vec![0];
    object.sections[index].zero_fill = u64::MAX;
    assert!(object.write().unwrap_err().contains("memory size overflow"));
}

#[test]
fn writer_rejects_oversized_output_and_reserved_section_counts_before_allocation() {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    assert!(object
        .write_with_limit(63)
        .unwrap_err()
        .contains("size limit"));
    let index = object.add_section(".data", SectionKind::Data, 1 << 63);
    object.sections[index].data = vec![1];
    assert!(object.write().unwrap_err().contains("size limit"));
    assert!(object
        .write_with_limit(u64::MAX)
        .unwrap_err()
        .contains("host capacity"));
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    for _ in 0..(0xff00 - 4) {
        object.add_section("", SectionKind::Bss, 1);
    }
    assert!(object
        .write()
        .unwrap_err()
        .contains("extended section numbering"));
}
