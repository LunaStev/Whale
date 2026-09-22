use object::{ObjectFile, ObjectFormat, ObjectRelocation, RelocKind, SectionKind};

fn object(size: usize) -> ObjectFile {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    let section = object.add_section(".text", SectionKind::Text, 1);
    object.sections[section].data.resize(size, 0);
    object
}

fn relocation(section_index: usize, offset: usize, kind: RelocKind) -> ObjectRelocation {
    ObjectRelocation {
        section_index,
        offset,
        kind,
        symbol: "external".into(),
        addend: -4,
    }
}

#[test]
fn relocation_section_must_exist() {
    let mut object = object(8);
    object
        .relocations
        .push(relocation(999, 0, RelocKind::Absolute64));
    let error = object
        .write()
        .expect_err("invalid relocation section must fail");
    assert!(
        error.contains("section") && error.contains("999"),
        "{error}"
    );
}

#[test]
fn every_relocation_width_must_fit_the_target_section() {
    for (kind, width) in [
        (RelocKind::Absolute64, 8),
        (RelocKind::Absolute32, 4),
        (RelocKind::Relative32, 4),
        (RelocKind::Relative8, 1),
        (RelocKind::GOTPCREL, 4),
        (RelocKind::PLT32, 4),
    ] {
        let mut object = object(width + 3);
        object.relocations.push(relocation(0, 3, kind));
        assert!(object.write().is_ok(), "boundary-valid {kind:?}");
        object.relocations[0].offset = 4;
        let error = object.write().expect_err("patch crosses section boundary");
        assert!(
            error.contains("range") && error.contains(".text"),
            "{error}"
        );
        object.relocations[0].offset = width + 3;
        assert!(object.write().is_err(), "patch starts at section end");
    }
}

#[test]
fn relocation_offset_overflow_and_empty_sections_are_rejected() {
    let mut object = object(8);
    object
        .relocations
        .push(relocation(0, usize::MAX, RelocKind::Absolute64));
    assert!(object
        .write()
        .expect_err("overflow must fail")
        .contains("overflow"));
    object.sections[0].data.clear();
    object.relocations[0].offset = 0;
    assert!(object
        .write()
        .expect_err("empty section has no patch range")
        .contains("range"));
}

#[test]
fn bss_cannot_silently_discard_relocation_patch_storage() {
    let mut object = object(8);
    object.sections[0].kind = SectionKind::Bss;
    object.sections[0].name = ".bss".into();
    object
        .relocations
        .push(relocation(0, 0, RelocKind::Absolute64));
    assert!(object
        .write()
        .expect_err("BSS relocation unsupported")
        .contains("BSS"));
    object.relocations.clear();
    assert!(
        object.write().is_ok(),
        "BSS with no relocation remains valid"
    );
}

#[test]
fn invalid_section_alignment_is_rejected() {
    for alignment in [3, 6, 7, 12, u64::MAX] {
        let mut object = object(1);
        object.sections[0].align = alignment;
        let error = object.write().expect_err("invalid alignment must fail");
        assert!(
            error.contains("alignment") && error.contains(".text"),
            "{error}"
        );
    }
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

#[test]
fn zero_alignment_means_one_and_valid_alignments_match_offsets() {
    for alignment in [0, 1, 2, 4, 8, 16, 32] {
        let mut object = object(3);
        let second = object.add_section(".data", SectionKind::Data, alignment);
        object.sections[second].data = vec![0x12, 0x34];
        let bytes = object.write().unwrap();
        let headers = u64_at(&bytes, 40) as usize;
        let header = headers + (second + 1) * 64;
        let offset = u64_at(&bytes, header + 24);
        let reported = u64_at(&bytes, header + 48);
        assert_eq!(reported, alignment.max(1));
        assert_eq!(offset % reported, 0);
        assert_eq!(&bytes[offset as usize..offset as usize + 2], &[0x12, 0x34]);
    }
}
