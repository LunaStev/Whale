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
    let result = whale().args(["reverse", "inspect"]).arg(&input).arg("--json").output().unwrap();
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
    for args in
        [vec!["disasm", "--help"], vec!["reverse", "--help"], vec!["reverse", "cfg", "--help"]]
    {
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
    let result = whale().arg("disasm").arg(&input).arg("--json").output().unwrap();
    if reverse::backend_available() {
        assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
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
        .args(["disasm", "--raw", "--amd64", "--base", "0x1000", "--max-instructions", "1"])
        .arg(input)
        .output()
        .unwrap()
        .status
        .success());
}

fn graph_json(command: &mut Command) -> serde_json::Value {
    let result = command.arg("--json").output().unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stderr));
    serde_json::from_slice(&result.stdout).unwrap()
}

fn graph_offsets(graph: &serde_json::Value) -> Vec<u64> {
    let mut offsets: Vec<_> = graph["instructions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|instruction| instruction["offset"].as_u64().unwrap())
        .collect();
    offsets.sort_unstable();
    offsets
}

fn sectionless_executable(code: &[u8], entry_offset: u64) -> Vec<u8> {
    let mut bytes = vec![0; 120];
    bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    for (at, value) in [(16, 2u16), (18, 62), (52, 64), (54, 56), (56, 1)] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [(20, 1u32), (64, 1), (68, 5)] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (24, 0x400078 + entry_offset),
        (32, 64),
        (80, 0x400000),
        (96, 120 + code.len() as u64),
        (104, 120 + code.len() as u64),
        (112, 4096),
    ] {
        bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(code);
    bytes
}

#[test]
fn ordinary_build_reports_cfg_and_xrefs_core_unavailable_explicitly() {
    if reverse::backend_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("ret");
    fs::write(&input, [0xc3]).unwrap();
    for mode in ["cfg", "xrefs"] {
        let result = whale()
            .args(["reverse", mode, "--raw", "--amd64", "--base", "0"])
            .arg(&input)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains("Wave CFG core unavailable"),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(result.stdout.is_empty());
    }
}

#[test]
fn sectionless_executable_graph_starts_at_elf_entry_and_follows_backward_helpers() {
    if !reverse::backend_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("sectionless");
    // The entry follows an unsupported gap; only the entry, its backward callee,
    // and its continuation are reachable. The mapped ELF headers are not code.
    let bytes =
        sectionless_executable(&[0xc3, 0x0f, 0x0b, 0x90, 0xe8, 0xf7, 0xff, 0xff, 0xff, 0xc3], 4);
    fs::write(&input, &bytes).unwrap();
    for mode in ["cfg", "xrefs"] {
        let graph = graph_json(whale().args(["reverse", mode]).arg(&input));
        assert_eq!(graph["kind"], mode);
        assert_eq!(graph["base"], 0x400000);
        assert_eq!(graph["entry_offset"], 124);
        assert_eq!(graph_offsets(&graph), [120, 124, 129]);
        assert!(graph["stops"].as_array().unwrap().is_empty());
        assert!(graph["edges"].as_array().unwrap().iter().any(|edge| {
            edge["from"] == 124
                && edge["kind"] == "Call"
                && edge["target"]["kind"] == "address"
                && edge["target"]["address"] == 0x400078
        }));
        if mode == "xrefs" {
            assert!(graph["edges"]
                .as_array()
                .unwrap()
                .iter()
                .all(|edge| edge["kind"] != "FallThrough"));
        }
    }
    assert_eq!(fs::read(input).unwrap(), bytes);
}

#[test]
fn raw_graph_accepts_explicit_entry_and_follows_backward_helpers() {
    if !reverse::backend_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw");
    fs::write(&input, [0xc3, 0x0f, 0x0b, 0x90, 0xe8, 0xf7, 0xff, 0xff, 0xff, 0xc3]).unwrap();
    let graph = graph_json(
        whale()
            .args(["reverse", "cfg", "--raw", "--amd64", "--base", "0x1000", "--entry", "0x1004"])
            .arg(&input),
    );
    assert_eq!(graph["entry_offset"], 4);
    assert_eq!(graph_offsets(&graph), [0, 4, 9]);
    assert!(graph["edges"].as_array().unwrap().iter().any(|edge| {
        edge["from"] == 4 && edge["kind"] == "Call" && edge["target"]["address"] == 0x1000
    }));
}

#[test]
fn relocatable_graph_requires_entry_and_preserves_symbolic_external_calls() {
    if !reverse::backend_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("external.o");
    fs::write(&input, object()).unwrap();
    for mode in ["cfg", "xrefs"] {
        let missing = whale().args(["reverse", mode]).arg(&input).output().unwrap();
        assert!(!missing.status.success());
        assert!(String::from_utf8_lossy(&missing.stderr).contains("entry"));
        let graph = graph_json(whale().args(["reverse", mode, "--entry", "0"]).arg(&input));
        assert_eq!(graph_offsets(&graph), [0, 5]);
        let calls: Vec<_> = graph["edges"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|edge| edge["kind"] == "Call")
            .collect();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["target"]["kind"], "symbol");
        assert_eq!(calls[0]["target"]["name"], "external");
        assert!(calls[0]["target"]["address"].is_null());
        assert!(graph["instructions"][0]["target"].is_null());
        assert_eq!(graph["relocations"][0]["symbol"], "external");
        let text = whale().args(["reverse", mode, "--entry", "0"]).arg(&input).output().unwrap();
        assert!(text.status.success());
        assert!(String::from_utf8_lossy(&text.stdout).contains("symbol \"external\""));
        assert!(!String::from_utf8_lossy(&text.stdout).contains("Call -> 0x5"));
    }
}

#[test]
fn relocatable_graph_defaults_to_start_and_keeps_one_code_region_scope() {
    if !reverse::backend_available() {
        return;
    }
    use object::*;
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.add_section(".text", SectionKind::Text, 1);
    object.add_section(".text.other", SectionKind::Text, 1);
    object.sections[0].data = vec![0xc3];
    object.sections[1].data = vec![0xc5];
    object.symbols.push(ObjectSymbol {
        name: "_start".into(),
        section_index: Some(0),
        value: 0,
        size: 1,
        binding: SymbolBinding::Global,
        kind: SymbolKind::Function,
        visibility: SymbolVisibility::Default,
    });
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("regions.o");
    fs::write(&input, object.write().unwrap()).unwrap();
    for mode in ["cfg", "xrefs"] {
        let graph = graph_json(whale().args(["reverse", mode]).arg(&input));
        assert_eq!(graph["region"], 0);
        assert_eq!(graph_offsets(&graph), [0]);
        assert!(graph["stops"].as_array().unwrap().is_empty());
        let ambiguous =
            whale().args(["reverse", mode, "--entry", "0"]).arg(&input).output().unwrap();
        assert!(!ambiguous.status.success());
        assert!(String::from_utf8_lossy(&ambiguous.stderr).contains("ambiguous"));
    }
}

#[test]
fn graph_distinguishes_indirect_targets_overlap_and_unknown_stops() {
    if !reverse::backend_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw");
    fs::write(&input, [0xff, 0xd0, 0xff, 0xe0, 0xc3]).unwrap();
    for mode in ["cfg", "xrefs"] {
        let graph = graph_json(
            whale().args(["reverse", mode, "--raw", "--amd64", "--base", "0x1000"]).arg(&input),
        );
        assert_eq!(graph_offsets(&graph), [0, 2]);
        for kind in ["IndirectCall", "IndirectJump"] {
            assert!(graph["edges"]
                .as_array()
                .unwrap()
                .iter()
                .any(|edge| { edge["kind"] == kind && edge["target"]["kind"] == "unresolved" }));
        }
        assert!(graph["stops"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stop| { stop["kind"] == "IndirectJump" && stop["offset"] == 2 }));
    }
    for (bytes, stop_kind, offset, conflict) in [
        (vec![0xeb, 0xff], "Overlap", 1, Some(0)),
        (vec![0xc5], "Unsupported", 0, None),
        (vec![0x48, 0xb8], "Truncated", 0, None),
    ] {
        fs::write(&input, bytes).unwrap();
        let graph = graph_json(
            whale().args(["reverse", "cfg", "--raw", "--amd64", "--base", "0x1000"]).arg(&input),
        );
        assert!(graph["stops"].as_array().unwrap().iter().any(|stop| {
            stop["kind"] == stop_kind
                && stop["offset"] == offset
                && stop["conflict"] == serde_json::json!(conflict)
        }));
    }
}

#[test]
fn graph_output_limit_is_exact_and_failed_publication_preserves_files() {
    if !reverse::backend_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw");
    let bytes = [0x90, 0xc3];
    fs::write(&input, bytes).unwrap();
    let output = dir.path().join("graph");
    for mode in ["cfg", "xrefs"] {
        for structured in [false, true] {
            let command = || {
                let mut command = whale();
                command.args(["reverse", mode, "--raw", "--amd64", "--base", "0x1000"]).arg(&input);
                if structured {
                    command.arg("--json");
                }
                command
            };
            let complete = command().output().unwrap();
            assert!(complete.status.success());
            let exact = command()
                .args(["--max-output", &complete.stdout.len().to_string()])
                .output()
                .unwrap();
            assert!(exact.status.success());
            assert_eq!(exact.stdout, complete.stdout);
            fs::write(&output, b"old").unwrap();
            let short = command()
                .args(["--max-output", &(complete.stdout.len() - 1).to_string()])
                .arg("-o")
                .arg(&output)
                .output()
                .unwrap();
            assert!(!short.status.success());
            assert!(String::from_utf8_lossy(&short.stderr).contains("output byte limit"));
            assert!(short.stdout.is_empty());
            assert_eq!(fs::read(&output).unwrap(), b"old");
            assert!(!command().arg("-o").arg(&input).output().unwrap().status.success());
            assert_eq!(fs::read(&input).unwrap(), bytes);
        }
    }
}

#[test]
fn graph_budget_options_are_validated_and_enforced_before_output_publication() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("raw");
    fs::write(&input, [0x90, 0xc3]).unwrap();
    let output = dir.path().join("graph");
    for mode in ["cfg", "xrefs"] {
        for (flag, value) in [
            ("--max-work", "0"),
            ("--max-work", "1073741825"),
            ("--max-workspace", "0"),
            ("--max-workspace", "536870913"),
        ] {
            let result = whale()
                .args(["reverse", mode, "--raw", "--amd64", "--base", "0"])
                .arg(&input)
                .args([flag, value])
                .output()
                .unwrap();
            assert!(!result.status.success());
            assert!(String::from_utf8_lossy(&result.stderr).contains(flag));
        }
        if reverse::backend_available() {
            for (flag, resource) in [
                ("--max-instructions", "Instructions"),
                ("--max-work", "Work"),
                ("--max-workspace", "Memory"),
            ] {
                fs::write(&output, b"old").unwrap();
                let result = whale()
                    .args(["reverse", mode, "--raw", "--amd64", "--base", "0"])
                    .arg(&input)
                    .args([flag, "1", "-o"])
                    .arg(&output)
                    .output()
                    .unwrap();
                assert!(!result.status.success());
                assert!(
                    String::from_utf8_lossy(&result.stderr).contains(resource),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert!(result.stdout.is_empty());
                assert_eq!(fs::read(&output).unwrap(), b"old");
            }
        }
    }
}
