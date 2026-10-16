use linker::core::executable::{executable_layout, write_executable};

use object::*;

fn program() -> ObjectFile {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section(".text", SectionKind::Text, 16);
    // mov eax,60; mov edi,37; syscall
    o.sections[0].data = vec![0xb8, 60, 0, 0, 0, 0xbf, 37, 0, 0, 0, 0x0f, 0x05];
    o.symbols.push(ObjectSymbol {
        name: "_start".into(),
        section_index: Some(0),
        value: 0,
        size: 12,
        kind: SymbolKind::Function,
        binding: SymbolBinding::Global,
        visibility: SymbolVisibility::Default,
    });
    o
}

fn u64at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().unwrap())
}

#[test]
fn headers_segments_permissions_and_sparse_bss_agree() {
    let mut o = program();
    o.add_section(".constants", SectionKind::ReadOnlyData, 8192);
    o.sections[1].data = vec![1, 2, 3];
    o.add_section(".state", SectionKind::Data, 32);
    o.sections[2].data = vec![4, 5];
    o.add_section(".bss", SectionKind::Bss, 8192);
    o.sections[3].zero_fill = 1 << 30;
    let bytes = write_executable(&[o], "_start", 65536).unwrap();
    assert_eq!(&bytes[..7], b"\x7fELF\x02\x01\x01");
    assert_eq!(u64at(&bytes, 40), 0);
    assert_eq!(u64at(&bytes, 24) % 16, 0);
    assert_eq!(u16::from_le_bytes(bytes[56..58].try_into().unwrap()), 4);
    for (i, flags) in [5u32, 4, 6].into_iter().enumerate() {
        let at = 64 + i * 56;
        assert_eq!(u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()), flags);
        assert_eq!(u64at(&bytes, at + 8) % 4096, u64at(&bytes, at + 16) % 4096);
        assert!(u64at(&bytes, at + 32) <= u64at(&bytes, at + 40));
        assert!(u64at(&bytes, at + 8) + u64at(&bytes, at + 32) <= bytes.len() as u64);
    }
    assert!(u64at(&bytes, 64 + 2 * 56 + 40) > 1 << 30);
    assert!(bytes.len() < 65536);
}

#[test]
fn invalid_entry_and_limits_fail_before_output() {
    assert!(write_executable(&[program()], "missing", 65536).unwrap_err().contains("entry"));
    assert!(write_executable(&[program()], "_start", 1).is_err());
    let mut o = program();
    o.symbols[0].value = 12;
    o.symbols[0].size = 0;
    assert!(write_executable(&[o], "_start", 65536).is_err());
    let mut o = program();
    o.sections[0].kind = SectionKind::Data;
    assert!(write_executable(&[o], "_start", 65536).is_err());
    let mut o = program();
    o.sections[0].align = 3;
    assert!(executable_layout(&[o], 65536).is_err());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn syscall_only_executable_runs_with_the_asserted_exit_status() {
    use std::{fs, os::unix::fs::PermissionsExt, process::Command};
    let path = std::env::temp_dir().join(format!("whale-static-executable-{}", std::process::id()));
    let bytes = write_executable(&[program()], "_start", 65536).unwrap();
    fs::write(&path, bytes).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    let status = Command::new(&path).status();
    fs::remove_file(path).unwrap();
    assert_eq!(status.unwrap().code(), Some(37));
}

#[test]
fn sparse_bss_must_fit_portable_lower_user_address_space() {
    let mut o = program();
    o.add_section(".bss", SectionKind::Bss, 1);
    o.sections[1].zero_fill = 1 << 63;
    assert!(write_executable(&[o.clone()], "_start", 8192).unwrap_err().contains("user address"));
    o.sections[1].zero_fill = 1 << 40;
    let bytes = write_executable(&[o.clone()], "_start", 8192).unwrap();
    assert_eq!(bytes.len(), 4096);
    assert_eq!(u64at(&bytes, 64 + 56 + 40), 1 << 40);
    // The final user page is reserved; an exclusive end at its start is legal.
    let end = (1u64 << 47) - 4096;
    o.sections[1].zero_fill = end - 0x401000;
    assert!(write_executable(&[o.clone()], "_start", 8192).is_ok());
    o.sections[1].zero_fill += 1;
    assert!(write_executable(&[o.clone()], "_start", 8192).is_err());
    o.sections[1].zero_fill = 1;
    o.sections[1].align = 1 << 47;
    assert!(write_executable(&[o], "_start", 8192).is_err());
}
