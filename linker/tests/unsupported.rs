use linker::{core::linker::OutputFormat, Linker};

use object::{ObjectFile, ObjectFormat};

#[test]
fn missing_entry_never_reports_an_empty_success() {
    let mut linker = Linker::new(OutputFormat::ELF64Executable);
    assert!(linker.link().unwrap_err().contains("entry"));
    linker.add_object(ObjectFile::new(ObjectFormat::ELF64));
    assert!(linker.link().unwrap_err().contains("entry"));
}
