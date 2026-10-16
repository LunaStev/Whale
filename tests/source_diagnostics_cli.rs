use std::{fs, process::Command};

#[test]
fn source_mapped_runtime_trap_has_file_and_original_identity() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.wir");
    fs::write(
        &input,
        r#"module {
format_version 7 semantics_version 1 target "x86_64-whale-linux"
datalayout { ptr=64, endian=little }
source_file @s1 "main.wave"
original 77 @s1 9 3 9 8
source_map instruction @f0 %b0 2 77
declare @f0 "main": whale () -> i32, linkage internal
fn @f0 "main"() -> i32, entry %b0 {
%b0 "entry":
%v0: i32 = const i32 1
%v1: i32 = const i32 0
%v2: i32 = sdiv i32 %v0, %v1
ret i32 %v2
}}
"#,
    )
    .unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["ir", "run"])
        .arg(&input)
        .args(["--function", "@f0"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains("division by zero"), "{error}");
    assert!(error.contains("main.wave:9:3"), "{error}");
    assert!(error.contains("@n77"), "{error}");
    assert!(error.contains("@f0"), "{error}");
}

#[cfg(feature = "socket-cli")]
#[test]
fn frontend_error_has_nested_source_and_preserves_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("source.json");
    let output = directory.path().join("out.wir");
    let source = serde_json::json!({"format_version":4,"semantics_version":1,"features":[],"program":{
        "declarations":[],"globals":[],"functions":[{
            "convention":"Whale","linkage":"Internal","link_name":null,"name":"main","parameters":[],
            "return_type":{"Int":{"bits":32,"signed":true}},"body":[{"Return":{
                "Located":{"annotation":{"id":88,"source":{"file":"main.wave","line":12,"column":5,"end_line":12,"end_column":10}},
                "expression":{"Var":"missing"}}
            }}]
        }]
    }});
    fs::write(&input, serde_json::to_vec(&source).unwrap()).unwrap();
    fs::write(&output, b"previous").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["ir", "lower"])
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    let error = String::from_utf8(result.stderr).unwrap();
    assert!(error.contains("main.wave:12:5"), "{error}");
    assert!(error.contains("@n88"), "{error}");
    assert!(error.contains("UnknownVariable"), "{error}");
    assert_eq!(fs::read(output).unwrap(), b"previous");
}
