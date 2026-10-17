use object::formats::elf_read::ElfView;

use object::*;

use diagnostic::{Context, ToDiagnostic};

fn fixture() -> Vec<u8> {
    let hex = include_str!("fixtures/nasm-rel.hex");
    let bytes: Vec<_> = hex.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    bytes
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn sample() -> ObjectFile {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    for (name, kind, align) in [
        (".text", SectionKind::Text, 16),
        (".data", SectionKind::Data, 8),
        (".rodata", SectionKind::ReadOnlyData, 1),
        (".bss", SectionKind::Bss, 32),
    ] {
        let i = o.add_section(name, kind, align);
        if kind == SectionKind::Bss {
            o.sections[i].zero_fill = 1 << 40;
        } else {
            o.sections[i].data = vec![0; 32];
        }
    }
    for (name, section, kind, binding, visibility) in [
        (
            "function",
            Some(0),
            SymbolKind::Function,
            SymbolBinding::Global,
            SymbolVisibility::Hidden,
        ),
        ("object", Some(1), SymbolKind::Object, SymbolBinding::Local, SymbolVisibility::Default),
        ("external", None, SymbolKind::NoType, SymbolBinding::Weak, SymbolVisibility::Default),
    ] {
        o.symbols.push(ObjectSymbol {
            name: name.into(),
            section_index: section,
            value: 0,
            size: if section.is_some() { 8 } else { 0 },
            kind,
            binding,
            visibility,
        });
    }
    for (i, kind) in [
        RelocKind::Absolute64,
        RelocKind::Absolute32,
        RelocKind::Relative32,
        RelocKind::Relative8,
        RelocKind::GOTPCREL,
        RelocKind::PLT32,
    ]
    .into_iter()
    .enumerate()
    {
        o.relocations.push(ObjectRelocation {
            section_index: i % 2,
            offset: i * 4,
            symbol: if i % 2 == 0 { "external" } else { "object" }.into(),
            addend: -(i as i64) - 4,
            kind,
        });
    }
    o
}

fn assert_semantics(a: &ObjectFile, b: &ObjectFile) {
    assert_eq!(a.target, b.target);
    assert_eq!(a.sections.len(), b.sections.len());
    for (x, y) in a.sections.iter().zip(&b.sections) {
        assert_eq!(
            (&x.name, x.kind, &x.data, x.memory_size().unwrap(), x.align.max(1)),
            (&y.name, y.kind, &y.data, y.memory_size().unwrap(), y.align.max(1))
        );
    }
    assert_eq!(a.symbols.len(), b.symbols.len());
    for x in &a.symbols {
        let y = b.symbols.iter().find(|s| s.name == x.name).unwrap();
        assert_eq!(
            (x.section_index, x.value, x.size, x.kind, x.binding, x.visibility),
            (y.section_index, y.value, y.size, y.kind, y.binding, y.visibility)
        );
    }
    assert_eq!(a.relocations.len(), b.relocations.len());
    for x in &a.relocations {
        let y = b
            .relocations
            .iter()
            .find(|r| r.section_index == x.section_index && r.offset == x.offset)
            .unwrap();
        assert_eq!((&x.symbol, x.addend, x.kind), (&y.symbol, y.addend, y.kind));
    }
}

fn rejected(bytes: &[u8]) -> ObjectError {
    ObjectFile::read(bytes).err().expect("malformed ELF must fail")
}

fn put16(bytes: &mut [u8], at: usize, v: u16) {
    bytes[at..at + 2].copy_from_slice(&v.to_le_bytes())
}

fn put32(bytes: &mut [u8], at: usize, v: u32) {
    bytes[at..at + 4].copy_from_slice(&v.to_le_bytes())
}

fn put64(bytes: &mut [u8], at: usize, v: u64) {
    bytes[at..at + 8].copy_from_slice(&v.to_le_bytes())
}

#[test]
fn writer_reader_roundtrip_preserves_all_supported_semantic_fields_and_sparse_bss() {
    let original = sample();
    let bytes = original.write().unwrap();
    assert!(bytes.len() < 4096);
    let imported = ObjectFile::read(&bytes).unwrap();
    assert_semantics(&original, &imported);
    let again = ObjectFile::read(&imported.write().unwrap()).unwrap();
    assert_semantics(&imported, &again);
    let empty = ObjectFile::read(&ObjectFile::new(ObjectFormat::ELF64).write().unwrap()).unwrap();
    assert!(empty.sections.is_empty() && empty.symbols.is_empty());
    let mut unnamed = sample();
    unnamed.symbols[0].name.clear();
    assert_semantics(&unnamed, &ObjectFile::read(&unnamed.write().unwrap()).unwrap());
}

#[test]
fn retained_nasm_sections_symbols_and_relocation_indices_load_without_nasm() {
    let bytes = fixture();
    let o = ObjectFile::read(&bytes).unwrap();
    assert_eq!(
        o.sections.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        [".text", ".rodata", ".data", ".bss"]
    );
    assert_eq!(o.sections[0].data, [0xe8, 0, 0, 0, 0, 0x48, 0x8d, 5, 0, 0, 0, 0, 0xc3]);
    assert_eq!(o.sections[1].data, [0x41, 0, 0xff]);
    assert_eq!(o.sections[3].zero_fill, 32);
    assert!(o.sections[3].data.is_empty());
    let entry = o.symbols.iter().find(|s| s.name == "entry").unwrap();
    assert_eq!(
        (entry.kind, entry.binding, entry.section_index),
        (SymbolKind::Function, SymbolBinding::Global, Some(0))
    );
    assert!(!o.symbols.iter().any(|s| s.name == "nasm-rel.asm"));
    assert!(o
        .symbols
        .iter()
        .any(|s| s.name == "__whale_elf_section_2$" && s.binding == SymbolBinding::Local));
    assert!(o
        .symbols
        .iter()
        .any(|s| s.name == "__whale_elf_section_2" && s.binding == SymbolBinding::Global));
    let external = &o.relocations[0];
    assert_eq!(
        (external.offset, external.symbol.as_str(), external.kind, external.addend),
        (1, "external", RelocKind::Relative32, -4)
    );
    for r in &o.relocations[1..] {
        let s = o.symbols.iter().find(|s| s.name == r.symbol).unwrap();
        assert_eq!(s.section_index, Some(1));
        assert_eq!(s.binding, SymbolBinding::Local);
    }
    assert_semantics(&o, &ObjectFile::read(&o.write().unwrap()).unwrap());
}

#[test]
fn every_truncated_writer_prefix_is_rejected_without_panicking() {
    let bytes = sample().write().unwrap();
    for end in 0..bytes.len() {
        assert!(ObjectFile::read(&bytes[..end]).is_err(), "prefix {end}");
    }
}

#[test]
fn unsupported_identity_and_checked_table_ranges_have_typed_causes() {
    let good = fixture();
    for (at, value, kind) in [
        (0, 0, ObjectErrorKind::InvalidEncoding),
        (4, 1, ObjectErrorKind::Unsupported),
        (5, 2, ObjectErrorKind::Unsupported),
        (6, 0, ObjectErrorKind::Unsupported),
        (7, 9, ObjectErrorKind::Unsupported),
        (18, 183, ObjectErrorKind::InvalidTarget),
    ] {
        let mut b = good.clone();
        b[at] = value;
        assert_eq!(rejected(&b).kind, kind, "field {at}");
    }
    let mut b = good.clone();
    put16(&mut b, 16, 2);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::Unsupported);
    let mut b = good.clone();
    put64(&mut b, 40, u64::MAX - 2);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::Overflow);
    let mut b = good.clone();
    put16(&mut b, 58, 8);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidSection);
    let mut b = good.clone();
    put16(&mut b, 60, 0);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::Unsupported);
    let mut b = good.clone();
    put16(&mut b, 62, 20);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidSection);
    let mut v = ElfView::parse(&good, ObjectReadLimits::default()).unwrap();
    v.header.section_offset = u64::MAX;
    assert!(v.section(1).is_err());
    v.header.program_count = 1;
    v.header.program_offset = u64::MAX;
    assert!(v.program(0).is_err());
}

#[test]
fn malformed_sections_symbols_relas_and_names_preserve_offset_context() {
    let good = fixture();
    let view = ElfView::parse(&good, ObjectReadLimits::default()).unwrap();
    let table = view.header.section_offset as usize;
    let sym = view.section(6).unwrap();
    let rela = view.section(8).unwrap();
    let strings = view.section(7).unwrap();
    let mut b = good.clone();
    put64(&mut b, table + 64 + 48, 3);
    let e = rejected(&b);
    assert_eq!(e.kind, ObjectErrorKind::InvalidAlignment);
    assert!(e.diagnostic().contexts.contains(&Context::Section { index: 1, name: None }));
    let mut b = good.clone();
    put32(&mut b, table + 64, 99999);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidName);
    let mut b = good.clone();
    put64(&mut b, table + 6 * 64 + 56, 0);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidSymbol);
    let mut b = good.clone();
    put16(&mut b, sym.offset as usize + 10 * 24 + 6, 0xfff1);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::Unsupported);
    let mut b = good.clone();
    put64(&mut b, rela.offset as usize + 8, (999u64 << 32) | 2);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidRelocation);
    let mut b = good.clone();
    put64(&mut b, rela.offset as usize + 8, (1u64 << 32) | 2);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidRelocation);
    let mut b = good.clone();
    put64(&mut b, rela.offset as usize + 8, (9u64 << 32) | 999);
    let e = rejected(&b);
    assert_eq!(e.kind, ObjectErrorKind::Unsupported);
    assert!(e.contexts.iter().any(|c| matches!(c, Context::FileOffset(_))));
    let mut b = good.clone();
    put64(&mut b, rela.offset as usize, 13);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::OutOfBounds);
    let mut b = good.clone();
    b[strings.offset as usize + strings.size as usize - 1] = b'X';
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidName);
    let mut b = good.clone();
    put32(&mut b, table + 8 * 64 + 40, 7);
    assert_eq!(rejected(&b).kind, ObjectErrorKind::InvalidRelocation);
}

#[test]
fn every_reader_budget_has_an_independent_failure_and_valid_neighbor() {
    let good = fixture();
    let defaults = ObjectReadLimits::default();
    for (limits, resource) in [
        (
            ObjectReadLimits { max_input_bytes: good.len() - 1, ..defaults },
            ObjectResource::InputBytes,
        ),
        (ObjectReadLimits { max_sections: 9, ..defaults }, ObjectResource::Sections),
        (ObjectReadLimits { max_symbols: 11, ..defaults }, ObjectResource::Symbols),
        (ObjectReadLimits { max_relocations: 2, ..defaults }, ObjectResource::Relocations),
        (ObjectReadLimits { max_payload_bytes: 23, ..defaults }, ObjectResource::PayloadBytes),
        (ObjectReadLimits { max_string_bytes: 0, ..defaults }, ObjectResource::StringBytes),
        (ObjectReadLimits { max_work: 0, ..defaults }, ObjectResource::Work),
    ] {
        let e = ObjectFile::read_with_limits(&good, limits).err().unwrap();
        assert_eq!(e.kind, ObjectErrorKind::ResourceLimit);
        assert_eq!(e.resource, Some(resource));
    }
    assert!(ObjectFile::read_with_limits(
        &good,
        ObjectReadLimits {
            max_input_bytes: good.len(),
            max_sections: 10,
            max_symbols: 12,
            max_relocations: 3,
            max_payload_bytes: 24,
            ..defaults
        }
    )
    .is_ok());
    let mut with_program = good.clone();
    put16(&mut with_program, 54, 56);
    put16(&mut with_program, 56, 1);
    put64(&mut with_program, 32, 64);
    let e = ElfView::parse(&with_program, ObjectReadLimits { max_program_headers: 0, ..defaults })
        .unwrap_err();
    assert_eq!(e.resource, Some(ObjectResource::ProgramHeaders));
}

#[test]
fn aliased_payloads_and_repeated_names_cannot_multiply_decoded_storage_unboundedly() {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    for i in 0..32 {
        let section = o.add_section(&format!("s{i}"), SectionKind::Data, 1);
        o.sections[section].data = vec![1; 16];
    }
    let mut b = o.write().unwrap();
    let v = ElfView::parse(&b, ObjectReadLimits::default()).unwrap();
    let first = v.section(1).unwrap().offset;
    let table = v.header.section_offset as usize;
    for i in 1..=32 {
        put64(&mut b, table + i * 64 + 24, first);
    }
    let e = ObjectFile::read_with_limits(
        &b,
        ObjectReadLimits { max_payload_bytes: 16, ..Default::default() },
    )
    .err()
    .unwrap();
    assert_eq!(e.resource, Some(ObjectResource::PayloadBytes));
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section(&"long".repeat(1024), SectionKind::Data, 1);
    let e = ObjectFile::read_with_limits(
        &o.write().unwrap(),
        ObjectReadLimits { max_work: 128, ..Default::default() },
    )
    .err()
    .unwrap();
    assert_eq!(e.resource, Some(ObjectResource::Work));
}

#[test]
fn typed_validation_and_legacy_rendering_share_the_same_component_cause() {
    let mut o = sample();
    o.sections[1].align = 3;
    let e = o.validate_diagnostic().unwrap_err();
    assert_eq!(e.kind, ObjectErrorKind::InvalidAlignment);
    assert_eq!(e.diagnostic().phase, "validate");
    assert_eq!(e.diagnostic().code, "invalid-alignment");
    assert_eq!(o.validate().unwrap_err(), e.to_string());
    let e = e.context(Context::Object { name: "archive.a".into(), member: Some("input.o".into()) });
    assert!(e.to_string().contains("archive.a(input.o)"));
}

#[test]
fn typed_writer_distinguishes_output_budget_layout_and_invalid_input_causes() {
    let o = sample();
    let exact = o.write_diagnostic().unwrap().len() as u64;
    assert_eq!(o.write_diagnostic_with_limit(exact).unwrap(), o.write().unwrap());
    let e = o.write_diagnostic_with_limit(exact - 1).unwrap_err();
    assert_eq!(
        (e.kind, e.resource),
        (ObjectErrorKind::ResourceLimit, Some(ObjectResource::OutputBytes))
    );
    assert_eq!(e.diagnostic().phase, "write");
    let mut o = sample();
    o.sections[1].align = 3;
    let e = o.write_diagnostic().unwrap_err();
    assert_eq!(e.kind, ObjectErrorKind::InvalidAlignment);
    assert!(e.contexts.contains(&Context::Section { index: 1, name: None }));
    let mut o = sample();
    o.symbols[0].section_index = Some(99);
    let e = o.write_diagnostic().unwrap_err();
    assert_eq!(e.kind, ObjectErrorKind::InvalidSection);
    assert!(e.contexts.contains(&Context::Symbol("function".into())));
}

#[test]
fn sparse_nobits_alignment_may_put_conceptual_file_offset_beyond_eof() {
    for alignment in [32768, 1u64 << 63] {
        let mut object = ObjectFile::new(ObjectFormat::ELF64);
        let section = object.add_section("sparse", SectionKind::Bss, alignment);
        object.sections[section].zero_fill = 1 << 60;
        let bytes = object.write().unwrap();
        assert!(bytes.len() < 1024);
        let view = ElfView::parse(&bytes, ObjectReadLimits::default()).unwrap();
        assert!(view.section(1).unwrap().offset > bytes.len() as u64);
        let imported = ObjectFile::read_with_limits(
            &bytes,
            ObjectReadLimits { max_payload_bytes: 0, ..Default::default() },
        )
        .unwrap();
        assert_eq!(
            (imported.sections[0].align, imported.sections[0].zero_fill),
            (alignment, 1 << 60)
        );
        assert!(imported.sections[0].data.is_empty());
    }
}
