mod common;

use common::*;

use object::*;

#[test]
fn symbol_records_preserve_kind_size_visibility_values_and_local_order() {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    for (name, kind) in [(".text", SectionKind::Text), (".data", SectionKind::Data)] {
        let i = o.add_section(name, kind, 8);
        o.sections[i].data = vec![0; 32];
    }
    for (name, kind, section, value, size, binding, visibility) in [
        (
            "function",
            SymbolKind::Function,
            Some(0),
            4,
            7,
            SymbolBinding::Global,
            SymbolVisibility::Hidden,
        ),
        (
            "local_data",
            SymbolKind::Object,
            Some(1),
            8,
            3,
            SymbolBinding::Local,
            SymbolVisibility::Default,
        ),
        (
            "weak",
            SymbolKind::Function,
            Some(0),
            16,
            4,
            SymbolBinding::Weak,
            SymbolVisibility::Default,
        ),
        (
            "external",
            SymbolKind::Function,
            None,
            0,
            0,
            SymbolBinding::Global,
            SymbolVisibility::Default,
        ),
        (
            "local_label",
            SymbolKind::NoType,
            Some(0),
            31,
            0,
            SymbolBinding::Local,
            SymbolVisibility::Hidden,
        ),
    ] {
        o.symbols.push(ObjectSymbol {
            name: name.into(),
            kind,
            section_index: section,
            value,
            size,
            binding,
            visibility,
        });
    }
    let bytes = o.write().unwrap();
    let sections = sections(&bytes);
    let syms = symbols(&bytes, &sections);
    assert_eq!(
        syms,
        vec![
            Symbol {
                index: 0,
                name: "".into(),
                value: 0,
                size: 0,
                binding: 0,
                kind: 0,
                visibility: 0,
                section: 0
            },
            Symbol {
                index: 1,
                name: "local_data".into(),
                value: 8,
                size: 3,
                binding: 0,
                kind: 1,
                visibility: 0,
                section: 2
            },
            Symbol {
                index: 2,
                name: "local_label".into(),
                value: 31,
                size: 0,
                binding: 0,
                kind: 0,
                visibility: 2,
                section: 1
            },
            Symbol {
                index: 3,
                name: "function".into(),
                value: 4,
                size: 7,
                binding: 1,
                kind: 2,
                visibility: 2,
                section: 1
            },
            Symbol {
                index: 4,
                name: "weak".into(),
                value: 16,
                size: 4,
                binding: 2,
                kind: 2,
                visibility: 0,
                section: 1
            },
            Symbol {
                index: 5,
                name: "external".into(),
                value: 0,
                size: 0,
                binding: 1,
                kind: 2,
                visibility: 0,
                section: 0
            },
        ]
    );
    assert_eq!(sections.iter().find(|s| s.kind == 2).unwrap().info, 3);
    assert_eq!(o.write().unwrap(), bytes);
}

#[test]
fn all_relocation_kinds_link_to_exact_symbol_and_section_with_signed_addends() {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    for name in [".text", ".data"] {
        let i = o.add_section(
            name,
            if name == ".text" { SectionKind::Text } else { SectionKind::Data },
            8,
        );
        o.sections[i].data = vec![0; 48];
    }
    o.symbols.push(ObjectSymbol {
        name: "local".into(),
        kind: SymbolKind::NoType,
        section_index: Some(1),
        value: 7,
        size: 1,
        binding: SymbolBinding::Local,
        visibility: SymbolVisibility::Default,
    });
    o.symbols.push(ObjectSymbol {
        name: "defined".into(),
        kind: SymbolKind::Function,
        section_index: Some(0),
        value: 0,
        size: 48,
        binding: SymbolBinding::Global,
        visibility: SymbolVisibility::Default,
    });
    let kinds = [
        RelocKind::Absolute64,
        RelocKind::Absolute32,
        RelocKind::Relative32,
        RelocKind::Relative8,
        RelocKind::GOTPCREL,
        RelocKind::PLT32,
    ];
    for section in [1, 0] {
        for (n, kind) in kinds.iter().enumerate() {
            o.relocations.push(ObjectRelocation {
                section_index: section,
                offset: n * 8,
                kind: *kind,
                symbol: ["local", "defined", "undefined"][n % 3].into(),
                addend: if n % 2 == 0 { 0x0b } else { -0x0a },
            });
        }
    }
    let bytes = o.write().unwrap();
    let secs = sections(&bytes);
    let syms = symbols(&bytes, &secs);
    let symtab = secs.iter().find(|s| s.kind == 2).unwrap();
    let relas: Vec<_> = secs.iter().filter(|s| s.kind == 4).collect();
    assert_eq!(relas.len(), 2);
    for (n, s) in relas.iter().enumerate() {
        assert_eq!(s.name, [".rela.text", ".rela.data"][n]);
        assert_eq!(s.info, n + 1);
        assert_eq!(s.link, symtab.index);
        assert_eq!(s.entsize, 24);
        assert_eq!(s.size, 6 * 24);
        for (i, kind) in [1u32, 10, 2, 15, 9, 4].into_iter().enumerate() {
            let p = s.offset + i * 24;
            let info = u64_at(&bytes, p + 8);
            assert_eq!(u64_at(&bytes, p), i as u64 * 8);
            assert_eq!(info as u32, kind);
            let sym = &syms[(info >> 32) as usize];
            assert_eq!(sym.name, ["local", "defined", "undefined"][i % 3]);
            assert_eq!(u64_at(&bytes, p + 16) as i64, if i % 2 == 0 { 0x0b } else { -0x0a });
        }
    }
    let undefined = syms.iter().find(|s| s.name == "undefined").unwrap();
    assert_eq!(
        (undefined.section, undefined.binding, undefined.kind, undefined.size),
        (0, 1, 0, 0)
    );
}
