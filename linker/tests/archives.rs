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
    let mut o = object(
        "_start",
        &[0xe8, 0, 0, 0, 0, 0x89, 0xc7, 0xb8, 60, 0, 0, 0, 0x0f, 5],
    );
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
    let header = format!(
        "{name:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        0,
        0,
        0,
        "100644",
        payload.len()
    );
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
