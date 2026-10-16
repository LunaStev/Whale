use linker::core::{
    layout::Layout,
    relocation::{apply_relocations, RelocationErrorKind},
};

use object::*;

fn input(kind: RelocKind, addend: i64) -> ObjectFile {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.add_section(".data", SectionKind::Data, 1);
    object.sections[0].data = vec![0xa5; 16];
    object.symbols.push(ObjectSymbol {
        name: "value".into(),
        section_index: Some(0),
        value: 0,
        size: 0,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Object,
        visibility: SymbolVisibility::Default,
    });
    object.relocations.push(ObjectRelocation {
        section_index: 0,
        offset: 0,
        symbol: "value".into(),
        kind,
        addend,
    });
    object
}

fn apply(kind: RelocKind, addend: i64, base: u64) -> Result<Vec<u8>, RelocationErrorKind> {
    let objects = [input(kind, addend)];
    let layout = Layout::compute(&objects, base).unwrap();
    apply_relocations(&objects, &layout, 32).map(|x| x[0][0].clone()).map_err(|e| e.kind)
}

#[test]
fn absolute_signed_and_unsigned_widths_are_distinct() {
    assert_eq!(&apply(RelocKind::SignedAbsolute32, -1, 0).unwrap()[..4], &[255; 4]);
    assert_eq!(apply(RelocKind::Absolute32, -1, 0), Err(RelocationErrorKind::Range));
    assert_eq!(&apply(RelocKind::Absolute32, 0, u32::MAX as u64).unwrap()[..4], &[255; 4]);
    assert_eq!(
        apply(RelocKind::SignedAbsolute32, 0, u32::MAX as u64),
        Err(RelocationErrorKind::Range)
    );
    assert_eq!(
        &apply(RelocKind::Absolute64, -4, 0x123456789).unwrap()[..8],
        &0x123456785u64.to_le_bytes()
    );
    assert!(apply(RelocKind::Absolute64, -1, 0).is_err());
    for v in [i32::MIN as i64, i32::MAX as i64] {
        assert!(apply(RelocKind::SignedAbsolute32, v, 0).is_ok());
    }
    for v in [i32::MIN as i64 - 1, i32::MAX as i64 + 1] {
        assert!(apply(RelocKind::SignedAbsolute32, v, 0).is_err());
    }
}

#[test]
fn pc_relative_uses_place_and_signed_ranges() {
    for kind in [RelocKind::Relative32, RelocKind::PLT32] {
        assert_eq!(&apply(kind, -4, 0x400000).unwrap()[..4], &(-4i32).to_le_bytes());
        for v in [i32::MIN as i64, i32::MAX as i64] {
            assert!(apply(kind, v, 0x400000).is_ok());
        }
        assert_eq!(apply(kind, i32::MAX as i64 + 1, 0x400000), Err(RelocationErrorKind::Range));
    }
    assert_eq!(apply(RelocKind::Relative8, -128, 1000).unwrap()[0], 128);
    assert_eq!(apply(RelocKind::Relative8, 127, 1000).unwrap()[0], 127);
    for v in [-129, 128] {
        assert_eq!(apply(RelocKind::Relative8, v, 1000), Err(RelocationErrorKind::Range));
    }
    assert_eq!(apply(RelocKind::GOTPCREL, 0, 0), Err(RelocationErrorKind::Unsupported));
}

#[test]
fn owner_qualified_cross_object_references_and_failures_are_atomic() {
    let mut source = input(RelocKind::Absolute64, -2);
    source.symbols[0].section_index = None;
    let mut target = input(RelocKind::Absolute64, 0);
    target.relocations.clear();
    let mut objects = [source, target];
    let layout = Layout::compute(&objects, 0x1000).unwrap();
    let bytes = apply_relocations(&objects, &layout, 32).unwrap();
    assert_eq!(&bytes[0][0][..8], &0x100eu64.to_le_bytes());
    assert_eq!(objects[0].sections[0].data, vec![0xa5; 16]);
    objects[0].relocations.push(ObjectRelocation {
        section_index: 0,
        offset: 4,
        symbol: "value".into(),
        addend: 0,
        kind: RelocKind::Absolute32,
    });
    assert_eq!(
        apply_relocations(&objects, &layout, 32).unwrap_err().kind,
        RelocationErrorKind::Overlap
    );
    assert_eq!(objects[0].sections[0].data, vec![0xa5; 16]);
    assert_eq!(
        apply_relocations(&objects, &layout, 31).unwrap_err().kind,
        RelocationErrorKind::Limit
    );
    objects[0].relocations[0].offset = usize::MAX;
    assert!(apply_relocations(&objects, &layout, 32).is_err());
}
