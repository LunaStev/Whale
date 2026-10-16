use linker::{
    core::{archive::read_archive, linker::OutputFormat},
    Linker,
};

use object::*;

fn object(name: &str, code: &[u8]) -> ObjectFile {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section(".text", SectionKind::Text, 1);
    o.sections[0].data = code.into();
    o.symbols.push(ObjectSymbol {
        name: name.into(),
        section_index: Some(0),
        value: 0,
        size: code.len() as u64,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });
    o
}

fn entry() -> ObjectFile {
    let mut o = object("_start", &[0xe8, 0, 0, 0, 0, 0x89, 0xc7, 0xb8, 60, 0, 0, 0, 0x0f, 5]);

    o.symbols.push(ObjectSymbol {
        name: "helper".into(),
        section_index: None,
        value: 0,
        size: 0,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });

    o.relocations.push(ObjectRelocation {
        section_index: 0,
        offset: 1,
        symbol: "helper".into(),
        kind: RelocKind::PLT32,
        addend: -4,
    });

    o
}

fn member(out: &mut Vec<u8>, name: &str, payload: &[u8]) {
    assert!(name.len() <= 16);
    let header =
        format!("{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", 0, 0, 0, "100644", payload.len());

    assert_eq!(header.len(), 60);
    out.extend(header.bytes());
    out.extend(payload);

    if payload.len() % 2 != 0 {
        out.push(b'\n');
    }
}

fn archive(objects: &[(&str, ObjectFile)]) -> Vec<u8> {
    let mut out = b"!<arch>\n".to_vec();

    for (name, object) in objects {
        member(&mut out, name, &object.write().unwrap());
    }

    out
}

#[test]
fn ordinary_long_names_and_malformed_boundaries() {
    let mut out = b"!<arch>\n".to_vec();
    member(&mut out, "//", b"a-very-long-object-name.o/\n");
    member(&mut out, "/0", b"abc");
    member(&mut out, "#1/19", b"bsd-long-name.o\0\0\0\0xyz");

    let parsed = read_archive(&out).unwrap();
    assert_eq!(parsed[0].name, "a-very-long-object-name.o");
    assert_eq!(parsed[0].bytes, b"abc");
    assert_eq!(parsed[1].name, "bsd-long-name.o");
    assert_eq!(parsed[1].bytes, b"xyz");

    for end in 1..out.len() {
        if end != 8 {
            let _ = read_archive(&out[..end]);
        }
    }

    assert!(read_archive(b"!<thin>\n").is_err());

    let mut bad = out.clone();
    bad[8 + 48] = b'-';
    assert!(read_archive(&bad).is_err());
    out.pop();
    assert!(read_archive(&out).is_err());
}

#[test]
fn unused_duplicate_definitions_do_not_participate_and_entry_seeds_extraction() {
    let bytes = archive(&[
        ("helper.o/", object("helper", &[0xb8, 42, 0, 0, 0, 0xc3])),
        ("entry.o/", entry()),
        ("unused.o/", object("helper", &[0xc3])),
    ]);

    let mut linker = Linker::new(OutputFormat::ELF64Executable);
    linker.add_archive(&bytes).unwrap();

    let result = linker.link().unwrap();
    assert!(result.len() < 4096);
    assert_eq!(linker.link().unwrap(), result);

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        use std::{fs, os::unix::fs::PermissionsExt, process::Command};

        let path = std::env::temp_dir().join(format!("whale-archive-run-{}", std::process::id()));

        fs::write(&path, result).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        let status = Command::new(&path).status();
        fs::remove_file(&path).unwrap();

        assert_eq!(status.unwrap().code(), Some(42));
    }
}

#[test]
fn archive_order_is_left_to_right_and_undefined_weak_does_not_extract() {
    let bytes = archive(&[("helper.o/", object("helper", &[0xc3]))]);
    let mut bad = Linker::new(OutputFormat::ELF64Executable);
    bad.add_archive(&bytes).unwrap();
    bad.add_object(entry());
    assert!(bad.link().unwrap_err().contains("Unresolved"));
    let mut good = Linker::new(OutputFormat::ELF64Executable);
    good.add_object(entry());
    good.add_archive(&bytes).unwrap();
    assert!(good.link().is_ok());
    let mut start = object("_start", &[0xb8, 60, 0, 0, 0, 0x31, 0xff, 0x0f, 5]);
    start.add_section(".data", SectionKind::Data, 1);
    start.sections[1].data = vec![0; 8];
    start.symbols.push(ObjectSymbol {
        name: "optional".into(),
        section_index: None,
        value: 0,
        size: 0,
        binding: SymbolBinding::Weak,
        kind: SymbolKind::NoType,
        visibility: SymbolVisibility::Default,
    });
    start.relocations.push(ObjectRelocation {
        section_index: 1,
        offset: 0,
        symbol: "optional".into(),
        kind: RelocKind::Absolute64,
        addend: 0,
    });
    // Selecting this member would introduce a duplicate _start and fail.
    let mut unused = object("optional", &[0xc3]);
    let mut other = object("_start", &[0xc3]);
    unused.symbols.push(other.symbols.remove(0));
    let bytes = archive(&[("unused.o/", unused)]);
    let mut link = Linker::new(OutputFormat::ELF64Executable);
    link.add_object(start);
    link.add_archive(&bytes).unwrap();
    assert!(link.link().is_ok());
}

fn reader_resource(error: linker::LinkError) -> ObjectResource {
    error.input_cause.expect("typed reader cause").resource.expect("resource cause")
}

fn aliased_object(section_count: usize, size: usize) -> Vec<u8> {
    // Independent alias fixture: only one payload is stored in the file, but
    // every raw PROGBITS header describes that same range.
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    for _ in 0..section_count {
        o.add_section(".data", SectionKind::Data, 1);
    }
    o.sections[0].data = vec![0x42; size];
    let mut bytes = o.write().unwrap();
    let table = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
    let first = table + 64;
    let offset = bytes[first + 24..first + 32].to_vec();
    for index in 1..=section_count {
        let at = table + index * 64;
        bytes[at + 24..at + 32].copy_from_slice(&offset);
        bytes[at + 32..at + 40].copy_from_slice(&(size as u64).to_le_bytes());
    }
    bytes
}

#[test]
fn archive_alias_payloads_share_one_budget_even_when_members_are_unused() {
    use linker::core::archive::read_objects_with_limits;
    let object = aliased_object(16, 1024);
    let mut bytes = b"!<arch>\n".to_vec();
    for name in ["a.o/", "b.o/", "c.o/"] {
        member(&mut bytes, name, &object);
    }
    let (_, usage) = ObjectFile::read_with_usage(&object, ObjectReadLimits::default()).unwrap();
    assert_eq!(usage.payload_bytes, 16 * 1024);
    assert!(object.len() < usage.payload_bytes);
    let error = read_objects_with_limits(
        &bytes,
        ObjectReadLimits { max_payload_bytes: 24 * 1024, ..ObjectReadLimits::default() },
    )
    .err()
    .expect("unused decoded members must still share payload budget");
    assert_eq!(reader_resource(error), ObjectResource::PayloadBytes);
    assert_eq!(
        read_objects_with_limits(
            &bytes,
            ObjectReadLimits { max_payload_bytes: 48 * 1024, ..ObjectReadLimits::default() }
        )
        .unwrap()
        .len(),
        3
    );
}

#[test]
fn every_reader_counter_is_cumulative_across_objects_including_auxiliary_records() {
    let bytes = entry().write().unwrap();
    let (object, u) = ObjectFile::read_with_usage(&bytes, ObjectReadLimits::default()).unwrap();
    assert_eq!(u.input_bytes, bytes.len());
    assert!(u.sections > object.sections.len());
    assert!(u.symbols > object.symbols.len()); // null symbol is real reader work
    assert_eq!(u.relocations, 1);
    let owned_names: usize = object.sections.iter().map(|s| s.name.len()).sum::<usize>()
        + object.symbols.iter().map(|s| s.name.len()).sum::<usize>()
        + object.relocations.iter().map(|s| s.symbol.len()).sum::<usize>();
    assert!(u.string_bytes > owned_names); // auxiliary/repeated name inspections
    for resource in [
        ObjectResource::InputBytes,
        ObjectResource::Sections,
        ObjectResource::Symbols,
        ObjectResource::Relocations,
        ObjectResource::PayloadBytes,
        ObjectResource::StringBytes,
        ObjectResource::Work,
    ] {
        let mut limits = ObjectReadLimits::default();
        let (slot, amount) = match resource {
            ObjectResource::InputBytes => (&mut limits.max_input_bytes, u.input_bytes),
            ObjectResource::Sections => (&mut limits.max_sections, u.sections),
            ObjectResource::Symbols => (&mut limits.max_symbols, u.symbols),
            ObjectResource::Relocations => (&mut limits.max_relocations, u.relocations),
            ObjectResource::PayloadBytes => (&mut limits.max_payload_bytes, u.payload_bytes),
            ObjectResource::StringBytes => (&mut limits.max_string_bytes, u.string_bytes),
            ObjectResource::Work => (&mut limits.max_work, u.work),
            _ => unreachable!(),
        };
        assert!(amount > 0);
        *slot = amount * 2 - 1;
        let mut linker = Linker::with_read_limits(OutputFormat::ELF64Executable, limits);
        linker.add_object_bytes(&bytes).unwrap();
        assert_eq!(reader_resource(linker.add_object_bytes(&bytes).unwrap_err()), resource);
    }
}

#[test]
fn shared_budget_spans_objects_and_archives_and_failed_archives_are_transactional() {
    let start = entry().write().unwrap();
    let helper = object("helper", &[0xc3]);
    let bytes = archive(&[("a.o/", helper.clone()), ("b.o/", object("unused", &[0x90; 32]))]);
    let mut linker = Linker::with_read_limits(
        OutputFormat::ELF64Executable,
        ObjectReadLimits { max_payload_bytes: 15, ..ObjectReadLimits::default() },
    );
    linker.add_object_bytes(&start).unwrap(); // 14 bytes
    for _ in 0..2 {
        assert_eq!(
            reader_resource(linker.add_archive(&bytes).unwrap_err()),
            ObjectResource::PayloadBytes
        );
    }
    // The first member of either rejected archive was neither retained nor charged.
    linker.add_object_bytes(&helper.write().unwrap()).unwrap();
    assert!(linker.link().is_ok());
    // Multiple archives share the same counters, rather than restarting quotas.
    let one = archive(&[("a.o/", helper)]);
    let mut linker = Linker::with_read_limits(
        OutputFormat::ELF64Executable,
        ObjectReadLimits { max_payload_bytes: 1, ..ObjectReadLimits::default() },
    );
    linker.add_archive(&one).unwrap();
    assert_eq!(
        reader_resource(linker.add_archive(&one).unwrap_err()),
        ObjectResource::PayloadBytes
    );
}

#[test]
fn archive_names_and_parser_work_are_preflighted_with_shared_reader_limits() {
    use linker::core::archive::read_objects_with_limits;
    let mut bsd = b"!<arch>\n".to_vec();
    member(&mut bsd, "#1/9", b"long-name");
    let error = read_objects_with_limits(
        &bsd,
        ObjectReadLimits { max_string_bytes: 8, ..ObjectReadLimits::default() },
    )
    .err()
    .unwrap();
    assert_eq!(reader_resource(error), ObjectResource::StringBytes);
    let error = read_objects_with_limits(
        &bsd,
        ObjectReadLimits { max_work: 1, ..ObjectReadLimits::default() },
    )
    .err()
    .unwrap();
    assert_eq!(reader_resource(error), ObjectResource::Work);
    for gnu in [false, true] {
        let long = vec![b'a'; 1024 * 1024 + 1];
        let mut bytes = b"!<arch>\n".to_vec();
        if gnu {
            let mut names = long;
            names.extend_from_slice(b"/\n");
            member(&mut bytes, "//", &names);
            member(&mut bytes, "/0", b"");
        } else {
            member(&mut bytes, "#1/1048577", &long);
        }
        assert!(read_archive(&bytes).unwrap_err().contains("name byte budget"));
    }
}
