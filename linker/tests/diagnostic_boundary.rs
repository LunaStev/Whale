use linker::{core::linker::OutputFormat, LinkErrorKind, Linker};

use object::{ObjectErrorKind, ObjectFile, ObjectFormat, SectionKind};

use whale_diagnostic::{Context, ToDiagnostic};

#[test]
fn checked_linker_preserves_input_identity_and_original_typed_validation_cause() {
    let mut o = ObjectFile::new(ObjectFormat::ELF64);
    o.add_section("bad", SectionKind::Data, 3);
    let mut l = Linker::new(OutputFormat::ELF64Executable);
    l.add_object(o);
    let e = l.link_checked().unwrap_err();
    assert_eq!(e.kind, LinkErrorKind::Input);
    assert_eq!(e.input_cause.as_ref().unwrap().kind, ObjectErrorKind::InvalidAlignment);
    assert!(e.diagnostic().contexts.contains(&Context::Input(0)));
    assert!(e.diagnostic().contexts.contains(&Context::Section { index: 0, name: None }));
    let mut empty = Linker::new(OutputFormat::ELF64Executable);
    let e = empty.link_checked().unwrap_err();
    assert_eq!(e.kind, LinkErrorKind::Entry);
    assert_eq!(e.diagnostic().code, "link.entry");
}
