use object::{
    ObjectFile, ObjectFormat, ObjectSymbol, SectionKind, SymbolBinding, SymbolVisibility,
};

fn object() -> ObjectFile {
    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    obj.add_section(".text", SectionKind::Text, 1);
    obj.sections[0].data = vec![0xc3];
    obj
}

fn symbol(section: Option<usize>, value: u64, size: u64) -> ObjectSymbol {
    ObjectSymbol {
        name: "entry".into(),
        section_index: section,
        value,
        size,
        binding: SymbolBinding::Global,
        visibility: SymbolVisibility::Default,
    }
}

#[test]
fn invalid_section_is_not_converted_into_an_external_symbol() {
    let mut obj = object();
    obj.symbols.push(symbol(Some(999), 0, 0));
    assert!(obj.write().unwrap_err().contains("invalid section"));
    obj.symbols[0].section_index = None;
    assert!(obj.write().is_ok());
}

#[test]
fn ambiguous_symbol_names_fail_in_either_insertion_order() {
    for reverse in [false, true] {
        let mut obj = object();
        obj.symbols.push(symbol(Some(0), 0, 1));
        obj.symbols.push(symbol(None, 0, 0));
        if reverse {
            obj.symbols.reverse();
        }
        assert!(obj.write().unwrap_err().contains("duplicate symbol"));
    }
}

#[test]
fn symbol_extents_fit_the_section_without_overflow() {
    for (value, size) in [(2, 0), (0, 2), (1, 1), (u64::MAX, 2)] {
        let mut obj = object();
        obj.symbols.push(symbol(Some(0), value, size));
        assert!(obj.write().unwrap_err().contains("symbol range"));
    }
    for (value, size) in [(0, 1), (1, 0)] {
        let mut obj = object();
        obj.symbols.push(symbol(Some(0), value, size));
        assert!(obj.write().is_ok());
    }
}

#[test]
fn bss_rejects_initial_data_but_preserves_zero_storage_and_end_labels() {
    let mut obj = object();
    obj.sections[0].kind = SectionKind::Bss;
    obj.sections[0].name = ".bss".into();
    assert!(obj.write().unwrap_err().contains("nonzero initializer"));
    obj.sections[0].data = vec![0; 8];
    obj.symbols.push(symbol(Some(0), 8, 0));
    let elf = obj.write().unwrap();
    let shoff = u64::from_le_bytes(elf[40..48].try_into().unwrap()) as usize;
    let bss = &elf[shoff + 64..shoff + 128];
    assert_eq!(u32::from_le_bytes(bss[4..8].try_into().unwrap()), 8);
    assert_eq!(u64::from_le_bytes(bss[32..40].try_into().unwrap()), 8);
    obj.symbols[0].value = 9;
    assert!(obj.write().is_err());
}
