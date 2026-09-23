use linker::{core::linker::OutputFormat, Linker};
use object::{ObjectFile, ObjectFormat};

#[test]
fn unfinished_linker_never_reports_an_empty_success() {
    let mut linker = Linker::new(OutputFormat::ELF64Executable);
    assert!(linker.link().unwrap_err().contains("not implemented"));
    linker.add_object(ObjectFile::new(ObjectFormat::ELF64));
    assert!(linker.link().unwrap_err().contains("not implemented"));
}
