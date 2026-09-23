use std::process::Command;

#[test]
fn unimplemented_linking_reports_failure() {
    let output = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["link", "missing.o"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("not implemented"));
}

#[test]
fn unknown_commands_fail_but_existing_help_still_succeeds() {
    let output = Command::new(env!("CARGO_BIN_EXE_whale"))
        .arg("nonsense")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Unknown command"));
    assert!(Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--help"])
        .output()
        .unwrap()
        .status
        .success());
}
