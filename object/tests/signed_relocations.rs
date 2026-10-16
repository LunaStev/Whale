use object::*;

#[test]
fn signed_absolute32_roundtrips_as_distinct_elf_type_eleven() {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section(".data", SectionKind::Data, 1);
    o.sections[0].data = vec![0; 8];
    o.symbols.push(ObjectSymbol {
        name: "x".into(),
        section_index: Some(0),
        value: 0,
        size: 0,
        binding: SymbolBinding::Local,
        kind: SymbolKind::Object,
        visibility: SymbolVisibility::Default,
    });
    for (offset, kind) in [(0, RelocKind::SignedAbsolute32), (4, RelocKind::Absolute32)] {
        o.relocations.push(ObjectRelocation {
            section_index: 0,
            offset,
            symbol: "x".into(),
            addend: -1,
            kind,
        });
    }
    let bytes = o.write().unwrap();
    let read = ObjectFile::read(&bytes).unwrap();
    assert_eq!(read.relocations[0].kind, RelocKind::SignedAbsolute32);
    assert_eq!(read.relocations[1].kind, RelocKind::Absolute32);
    assert_eq!(read.relocations[0].addend, -1);
}
