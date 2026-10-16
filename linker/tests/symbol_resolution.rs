use linker::core::symbol_table::{SymbolKey, SymbolTable};

use object::{
    ObjectFile, ObjectFormat, ObjectSymbol, SectionKind, SymbolBinding, SymbolVisibility,
};

fn object(name: &str, binding: SymbolBinding) -> ObjectFile {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.add_section(".text", SectionKind::Text, 1);
    object.sections[0].data = vec![0xc3];
    object.symbols.push(ObjectSymbol {
        kind: object::SymbolKind::NoType,
        name: name.into(),
        section_index: Some(0),
        value: 0,
        size: 1,
        binding,
        visibility: SymbolVisibility::Default,
    });
    object
}

#[test]
fn local_symbols_remain_scoped_to_their_own_objects() {
    let mut table = SymbolTable::new();
    table
        .resolve(&[object("helper", SymbolBinding::Local), object("helper", SymbolBinding::Local)])
        .unwrap();
    assert_eq!(table.symbols.len(), 2);
    for object_index in 0..2 {
        assert_eq!(
            table.symbols[&SymbolKey::Local { object_index, name: "helper".into() }].object_index,
            Some(object_index)
        );
    }
    let mut owners =
        table.symbols.values().map(|symbol| symbol.object_index.unwrap()).collect::<Vec<_>>();
    owners.sort();
    assert_eq!(owners, vec![0, 1]);
    for global_first in [true, false] {
        let mut objects =
            vec![object("helper", SymbolBinding::Global), object("helper", SymbolBinding::Local)];
        if !global_first {
            objects.reverse();
        }
        table.resolve(&objects).unwrap();
        assert_eq!(table.symbols.len(), 2);
    }
}

#[test]
fn duplicate_local_definitions_in_one_object_are_rejected() {
    let mut input = object("helper", SymbolBinding::Local);
    input.symbols.extend(object("helper", SymbolBinding::Local).symbols);
    let mut table = SymbolTable::new();
    assert!(table.resolve(&[input]).is_err());
    assert!(table.symbols.is_empty());
}

#[test]
fn independent_resolution_does_not_reuse_previous_state() {
    let mut table = SymbolTable::new();
    for _ in 0..2 {
        table.resolve(&[object("entry", SymbolBinding::Global)]).unwrap();
    }
    table.resolve(&[]).unwrap();
    assert!(table.symbols.is_empty());
    table.resolve(&[object("before", SymbolBinding::Global)]).unwrap();
    assert!(table
        .resolve(&[
            object("duplicate", SymbolBinding::Global),
            object("duplicate", SymbolBinding::Global)
        ])
        .is_err());
    assert!(table.symbols.is_empty(), "failed resolution must not expose stale or partial symbols");
    table.resolve(&[object("duplicate", SymbolBinding::Global)]).unwrap();
    assert_eq!(table.symbols.len(), 1);
}

#[test]
fn consuming_objects_rejects_bad_metadata_and_resets_resolution() {
    let mut table = SymbolTable::new();
    for bad in ["section", "range", "overflow", "symbol_name", "section_name", "relocation_name"] {
        table.resolve(&[object("previous", SymbolBinding::Global)]).unwrap();
        let mut input = object("entry", SymbolBinding::Global);
        match bad {
            "section" => input.symbols[0].section_index = Some(99),
            "range" => input.symbols[0].size = 2,
            "overflow" => {
                input.symbols[0].value = u64::MAX;
                input.symbols[0].size = 1;
            },
            "symbol_name" => input.symbols[0].name = "bad\0name".into(),
            "section_name" => input.sections[0].name = "bad\0section".into(),
            "relocation_name" => {
                input.relocations.push(object::ObjectRelocation {
                    section_index: 0,
                    offset: 0,
                    kind: object::RelocKind::Relative8,
                    symbol: "bad\0reloc".into(),
                    addend: 0,
                });
            },
            _ => unreachable!(),
        }
        assert!(input.validate().is_err(), "{bad}");
        assert!(table.resolve(&[input]).unwrap_err().contains("link input 0"), "{bad}");
        assert!(table.symbols.is_empty(), "{bad}");
    }
    let mut end = object("end", SymbolBinding::Global);
    end.symbols[0].value = 1;
    end.symbols[0].size = 0;
    end.validate().unwrap();
    table.resolve(&[end, object("neighbor", SymbolBinding::Local)]).unwrap();
    assert_eq!(table.symbols.len(), 2);
    assert_eq!(table.symbols[&SymbolKey::Global("end".into())].value, 1);
}

#[test]
fn strong_wins_weak_in_both_orders_first_weak_wins_and_conflicts_identify_inputs() {
    let mut table = SymbolTable::new();
    for strong_first in [true, false] {
        let mut weak = object("x", SymbolBinding::Weak);
        weak.symbols[0].size = 0;
        let strong = object("x", SymbolBinding::Global);
        let objects = if strong_first { vec![strong, weak] } else { vec![weak, strong] };
        table.resolve(&objects).unwrap();
        let x = &table.symbols[&SymbolKey::Global("x".into())];
        assert_eq!(x.binding, SymbolBinding::Global);
        assert_eq!(x.object_index, Some(usize::from(!strong_first)));
        assert_eq!(x.size, 1);
    }
    let mut second = object("x", SymbolBinding::Weak);
    second.symbols[0].size = 0;
    table.resolve(&[object("x", SymbolBinding::Weak), second]).unwrap();
    assert_eq!(table.symbols[&SymbolKey::Global("x".into())].object_index, Some(0));
    let error = table
        .resolve(&[object("x", SymbolBinding::Global), object("x", SymbolBinding::Global)])
        .unwrap_err();
    assert!(error.contains("objects 0 and 1"), "{error}");
    assert!(table.symbols.is_empty());
    assert!(table.references.is_empty());
}

fn reference(name: &str, binding: Option<SymbolBinding>) -> ObjectFile {
    let mut o = object("unrelated", SymbolBinding::Local);
    if let Some(binding) = binding {
        o.symbols.push(ObjectSymbol {
            name: name.into(),
            kind: object::SymbolKind::NoType,
            section_index: None,
            value: 0,
            size: 0,
            binding,
            visibility: SymbolVisibility::Default,
        });
    }
    o.relocations.push(object::ObjectRelocation {
        section_index: 0,
        offset: 0,
        symbol: name.into(),
        addend: 0,
        kind: object::RelocKind::Relative8,
    });
    o
}

#[test]
fn actual_references_resolve_locally_or_globally_and_unreferenced_declarations_are_ignored() {
    let mut table = SymbolTable::new();
    let error = table.resolve(&[reference("missing", None)]).unwrap_err();
    assert!(
        error.contains("missing")
            && error.contains("object 0")
            && error.contains(".text")
            && error.contains("offset 0"),
        "{error}"
    );
    let mut input = object("unused", SymbolBinding::Global);
    input.symbols[0].section_index = None;
    table.resolve(&[input]).unwrap();
    assert!(table.symbols.is_empty());
    table
        .resolve(&[
            reference("target", Some(SymbolBinding::Global)),
            object("target", SymbolBinding::Global),
        ])
        .unwrap();
    assert_eq!(table.references.len(), 1);
    assert_eq!(table.references[0].target, SymbolKey::Global("target".into()));
    assert!(!table.references[0].weak_undefined);
    let local = reference("unrelated", None);
    table.resolve(&[local, object("unrelated", SymbolBinding::Global)]).unwrap();
    assert_eq!(
        table.references[0].target,
        SymbolKey::Local { object_index: 0, name: "unrelated".into() }
    );
}

#[test]
fn referenced_undefined_weak_is_zero_and_never_satisfies_a_strong_reference() {
    let mut table = SymbolTable::new();
    table.resolve(&[reference("optional", Some(SymbolBinding::Weak))]).unwrap();
    let value = &table.symbols[&SymbolKey::Global("optional".into())];
    assert_eq!(
        (value.object_index, value.section_index, value.value, value.binding),
        (None, None, 0, SymbolBinding::Weak)
    );
    assert!(table.references[0].weak_undefined);
    for weak_first in [true, false] {
        let weak = reference("optional", Some(SymbolBinding::Weak));
        let strong = reference("optional", Some(SymbolBinding::Global));
        let objects = if weak_first { vec![weak, strong] } else { vec![strong, weak] };
        assert!(table.resolve(&objects).unwrap_err().contains("Unresolved strong"));
        assert!(table.symbols.is_empty());
        assert!(table.references.is_empty());
    }
    table
        .resolve(&[
            reference("optional", Some(SymbolBinding::Weak)),
            object("optional", SymbolBinding::Global),
        ])
        .unwrap();
    assert!(!table.references[0].weak_undefined);
    assert_eq!(table.symbols[&SymbolKey::Global("optional".into())].object_index, Some(1));
}

#[test]
fn public_linker_reports_unresolved_references_before_its_layout_boundary() {
    use linker::core::linker::{Linker, OutputFormat};
    let mut linker = Linker::new(OutputFormat::ELF64Executable);
    linker.add_object(reference("missing", None));
    assert!(linker.link().unwrap_err().contains("Unresolved strong symbol"));
}
