use std::{fs, process::Command};
fn whale() -> Command {
    Command::new(env!("CARGO_BIN_EXE_whale"))
}
fn object() -> Vec<u8> {
    use object::*;
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section(".text", SectionKind::Text, 1);
    o.sections[0].data = vec![0xe8, 0, 0, 0, 0, 0xc3];
    o.symbols.push(ObjectSymbol {
        name: "external".into(),
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
        symbol: "external".into(),
        kind: RelocKind::PLT32,
        addend: -4,
    });
    o.write().unwrap()
}
#[test]
fn inspection_is_available_in_ordinary_builds_and_arguments_are_strict() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("sample.o");
    fs::write(&input, object()).unwrap();
    let result = whale()
        .args(["reverse", "inspect"])
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    assert!(result.status.success());
    let v: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(v["kind"], "elf64-amd64-relocatable");
    assert_eq!(v["relocations"][0]["symbol"], "external");
    for args in [
        vec!["disasm", "--raw", "missing"],
        vec!["reverse", "inspect", "--wat"],
        vec!["reverse", "missing"],
        vec!["disasm", "--amd64", "--amd64", "missing"],
        vec!["disasm", "--max-instructions", "0", "missing"],
    ] {
        assert!(!whale().args(args).output().unwrap().status.success());
    }
    for args in [
        vec!["disasm", "--help"],
        vec!["reverse", "--help"],
        vec!["reverse", "cfg", "--help"],
    ] {
        assert!(whale().args(args).output().unwrap().status.success());
    }
}
#[test]
fn disassembly_is_wave_backed_and_preserves_input_and_failed_output() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("sample.o");
    let bytes = object();
    fs::write(&input, &bytes).unwrap();
    let output = dir.path().join("listing");
    fs::write(&output, b"old").unwrap();
    let result = whale()
        .arg("disasm")
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    if reverse::backend_available() {
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let v: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        let inst = &v["records"][0]["instruction"];
        assert_eq!(inst["mnemonic"], "call");
        assert!(inst["target"].is_null());
        assert_eq!(inst["relocations"][0]["symbol"], "external");
        assert_eq!(inst["bytes"], serde_json::json!([232, 0, 0, 0, 0]));
    } else {
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("Wave reverse core unavailable"));
    }
    assert!(!whale()
        .arg("disasm")
        .arg(&input)
        .arg("-o")
        .arg(&input)
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(fs::read(&input).unwrap(), bytes);
    assert!(!whale()
        .arg("disasm")
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .args(["--max-output", "1"])
        .output()
        .unwrap()
        .status
        .success());
    assert_eq!(fs::read(&output).unwrap(), b"old");
}
#[test]
fn raw_listing_keeps_unknown_bytes_and_enforces_decode_budget() {
    if !reverse::backend_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("bytes");
    fs::write(&input, [0x90, 0x0f]).unwrap();
    let result = whale()
        .args(["disasm", "--raw", "--amd64", "--base", "0x1000"])
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    assert!(result.status.success());
    let v: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(v["records"][0]["instruction"]["mnemonic"], "nop");
    assert_eq!(v["records"][1]["byte"], 15);
    assert_eq!(v["records"][1]["code"], false);
    assert!(!whale()
        .args([
            "disasm",
            "--raw",
            "--amd64",
            "--base",
            "0x1000",
            "--max-instructions",
            "1"
        ])
        .arg(input)
        .output()
        .unwrap()
        .status
        .success());
}
