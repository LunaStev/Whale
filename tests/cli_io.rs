use std::{
    fs,
    process::{Command, Stdio},
};

#[test]
fn version_forms_use_package_metadata() {
    for option in ["--version", "-V"] {
        let result = Command::new(env!("CARGO_BIN_EXE_whale")).arg(option).output().unwrap();
        assert!(result.status.success(), "{result:?}");
        assert!(result.stderr.is_empty());
        assert_eq!(result.stdout, format!("whale {}\n", env!("CARGO_PKG_VERSION")).as_bytes());
    }
}

#[test]
fn artifact_input_budgets_check_boundaries_and_preserve_output() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input");
    let output = directory.path().join("output.o");
    for mode in ["asm", "object"] {
        fs::write(&source, b"nop\n").unwrap();
        for (limit, success) in [("4", true), ("3", false), ("0", false)] {
            fs::write(&output, b"previous").unwrap();
            let mut command = Command::new(env!("CARGO_BIN_EXE_whale"));
            command.arg(mode);
            if mode == "asm" {
                command.arg("--amd64");
            }
            let result = command
                .arg(&source)
                .arg("-o")
                .arg(&output)
                .args(["--max-input", limit])
                .output()
                .unwrap();
            assert_eq!(result.status.success(), success, "{result:?}");
            if success {
                assert!(fs::read(&output).unwrap().starts_with(b"\x7fELF"));
            } else {
                assert!(
                    String::from_utf8_lossy(&result.stderr).contains("input byte limit"),
                    "{result:?}"
                );
                assert_eq!(fs::read(&output).unwrap(), b"previous");
            }
        }
    }
    fs::write(&source, [0xff]).unwrap();
    fs::write(&output, b"previous").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["asm", "--amd64"])
        .arg(&source)
        .arg("-o")
        .arg(&output)
        .args(["--max-input", "1"])
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(&output).unwrap(), b"previous");
    assert!(String::from_utf8_lossy(&result.stderr).contains("UTF-8"));
    assert_eq!(fs::read(&source).unwrap(), [0xff]);
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_fail_normally_without_modifying_files() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join(OsString::from_vec(b"input\xff".to_vec()));
    let output = directory.path().join("output.o");
    // Linux accepts raw filename bytes; APFS rejects the fixture itself with
    // EILSEQ. In either case the CLI must diagnose raw OS arguments before I/O.
    let invalid_file_exists = match fs::write(&input, b"source") {
        Ok(()) => true,
        Err(error) if cfg!(target_os = "macos") && error.raw_os_error() == Some(92) => false,
        Err(error) => panic!("failed to create non-UTF-8 filename fixture: {error}"),
    };
    fs::write(&output, b"previous").unwrap();
    for invalid in [input.as_os_str().to_owned(), OsString::from_vec(b"--bad\xff".to_vec())] {
        let result = Command::new(env!("CARGO_BIN_EXE_whale"))
            .arg("object")
            .arg(invalid)
            .arg("-o")
            .arg(&output)
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(2));
        let diagnostic = String::from_utf8_lossy(&result.stderr);
        assert!(diagnostic.contains("UTF-8"), "{diagnostic}");
        assert!(!diagnostic.contains("panicked"));
    }
    let valid = directory.path().join("valid input");
    fs::write(&valid, b"source").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .arg("object")
        .arg(&valid)
        .arg("-o")
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(2));
    if invalid_file_exists {
        assert_eq!(fs::read(input).unwrap(), b"source");
    } else {
        assert!(!input.exists());
    }
    assert_eq!(fs::read(valid).unwrap(), b"source");
    assert_eq!(fs::read(output).unwrap(), b"previous");
}

#[test]
fn closed_ir_stdout_exits_without_a_panic() {
    use std::io::Read;
    let directory = tempfile::tempdir().unwrap();
    let input = directory.path().join("large.wir");
    let mut source = String::from("module { format_version 4 semantics_version 1 target \"x86_64-whale-linux\" datalayout { ptr=64, endian=little } declare @f0 \"large\": whale () -> i32, linkage internal fn @f0 \"large\"() -> i32, entry %b0 { %b0 \"entry\":\n");
    for id in 0..15000 {
        source.push_str(&format!("%v{id}: i32 = const i32 42\n"));
    }
    source.push_str("ret i32 %v0 } }");
    fs::write(&input, &source).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["ir", "print"])
        .arg(&input)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdout.take().unwrap();
    let mut byte = [0];
    assert_eq!(pipe.read(&mut byte).unwrap(), 1);
    drop(pipe);
    let result = child.wait_with_output().unwrap();
    assert!(result.status.success(), "{result:?}");
    assert!(result.stderr.is_empty(), "{result:?}");
    assert_eq!(fs::read_to_string(input).unwrap(), source);
}

#[cfg(target_os = "linux")]
#[test]
fn ordinary_ir_stdout_failure_is_a_diagnostic() {
    let full = fs::OpenOptions::new().write(true).open("/dev/full").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_whale"))
        .args(["ir", "--help"])
        .stdout(full)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let diagnostic = String::from_utf8_lossy(&result.stderr);
    assert!(diagnostic.contains("failed to write stdout"), "{diagnostic}");
    assert!(!diagnostic.contains("panicked"));
}
