use linker::core::symbol_table::{SymbolKey, SymbolTable};
use object::{
    ObjectFile, ObjectFormat, ObjectSymbol, SectionKind, SymbolBinding, SymbolVisibility,
};

fn object(name: &str, binding: SymbolBinding) -> ObjectFile {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.add_section(".text", SectionKind::Text, 1);
    object.sections[0].data = vec![0xc3];
    object.symbols.push(ObjectSymbol {
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
        .resolve(&[
            object("helper", SymbolBinding::Local),
            object("helper", SymbolBinding::Local),
        ])
        .unwrap();
    assert_eq!(table.symbols.len(), 2);
    for object_index in 0..2 {
        assert_eq!(
            table.symbols[&SymbolKey::Local {
                object_index,
                name: "helper".into()
            }]
                .object_index,
            Some(object_index)
        );
    }
    let mut owners = table
        .symbols
        .values()
        .map(|symbol| symbol.object_index.unwrap())
        .collect::<Vec<_>>();
    owners.sort();
    assert_eq!(owners, vec![0, 1]);
    for global_first in [true, false] {
        let mut objects = vec![
            object("helper", SymbolBinding::Global),
            object("helper", SymbolBinding::Local),
        ];
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
    input
        .symbols
        .extend(object("helper", SymbolBinding::Local).symbols);
    let mut table = SymbolTable::new();
    assert!(table.resolve(&[input]).is_err());
    assert!(table.symbols.is_empty());
}

#[test]
fn independent_resolution_does_not_reuse_previous_state() {
    let mut table = SymbolTable::new();
    for _ in 0..2 {
        table
            .resolve(&[object("entry", SymbolBinding::Global)])
            .unwrap();
    }
    table.resolve(&[]).unwrap();
    assert!(table.symbols.is_empty());
    table
        .resolve(&[object("before", SymbolBinding::Global)])
        .unwrap();
    assert!(table
        .resolve(&[
            object("duplicate", SymbolBinding::Global),
            object("duplicate", SymbolBinding::Global)
        ])
        .is_err());
    assert!(
        table.symbols.is_empty(),
        "failed resolution must not expose stale or partial symbols"
    );
    table
        .resolve(&[object("duplicate", SymbolBinding::Global)])
        .unwrap();
    assert_eq!(table.symbols.len(), 1);
}
