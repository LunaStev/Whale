use std::{fs, path::Path, process::Command};
fn whale() -> Command {
    Command::new(env!("CARGO_BIN_EXE_whale"))
}
fn assemble(dir: &Path, name: &str, source: &str) -> std::path::PathBuf {
    let input = dir.join(format!("{name}.asm"));
    let out = dir.join(format!("{name}.o"));
    fs::write(&input, source).unwrap();
    let status = whale()
        .args(["asm", "--amd64"])
        .arg(&input)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "{}",
        String::from_utf8_lossy(&status.stderr)
    );
    out
}
#[test]
fn link_arguments_and_failures_preserve_every_input_and_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    let first = assemble(
        dir.path(),
        "first",
        "section .text\nglobal _start\n_start: ret\n",
    );
    let second = assemble(dir.path(), "second", "section .data\ndb 1\n");
    let old = fs::read(&second).unwrap();
    let result = whale()
        .arg("link")
        .arg(&first)
        .arg(&second)
        .arg("-o")
        .arg(&second)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read(&second).unwrap(), old);
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let alias = dir.path().join("alias");
        symlink(&second, &alias).unwrap();
        assert!(!whale()
            .arg("link")
            .arg(&first)
            .arg(&second)
            .arg("-o")
            .arg(&alias)
            .status()
            .unwrap()
            .success());
        assert_eq!(fs::read(&second).unwrap(), old);
    }
    let output = dir.path().join("app");
    fs::write(&output, b"old").unwrap();
    for flags in [
        vec!["--entry", "missing"],
        vec!["--max-output", "1"],
        vec!["--max-input", "1"],
        vec!["--unknown"],
    ] {
        let status = whale()
            .arg("link")
            .arg(&first)
            .arg("-o")
            .arg(&output)
            .args(flags)
            .output()
            .unwrap();
        assert!(!status.status.success());
        assert_eq!(fs::read(&output).unwrap(), b"old");
    }
    assert!(!whale()
        .args(["link", "-o"])
        .output()
        .unwrap()
        .status
        .success());
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn two_disk_objects_link_and_execute_with_relative_call_and_data_reference() {
    let dir = tempfile::tempdir().unwrap();
    let a=assemble(dir.path(),"start","section .text\nglobal _start\nextern helper\nextern payload\n_start: lea rsi,[rel payload]\nmov eax,[rsi]\ncall helper\nmov edi,eax\nmov eax,60\nsyscall\n");
    let b = assemble(
        dir.path(),
        "helper",
        "section .text\nglobal helper\nhelper: ret\nsection .data\nglobal payload\npayload: dd 42\nsection .bss\nresb 64\n",
    );
    let out = dir.path().join("program with spaces");
    let result = whale()
        .arg("link")
        .arg(a)
        .arg(b)
        .arg("-o")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(Command::new(out).status().unwrap().code(), Some(42));
}
