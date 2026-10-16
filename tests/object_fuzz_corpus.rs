#[path = "../fuzz/support.rs"]
mod fuzz;

#[test]
fn binary_elf_is_checked_before_utf8_and_sparse_bss_is_not_materialized() {
    let valid = include_bytes!("../fuzz/corpus/object/nasm-helper.elf-data").to_vec();
    assert!(std::str::from_utf8(&valid).is_err());
    assert!(fuzz::check(fuzz::Target::Object, &valid));
    assert!(fuzz::check(
        fuzz::Target::Object,
        include_bytes!("../fuzz/corpus/object/sparse-bss.elf-data")
    ));
    for bad in [
        include_bytes!("../fuzz/corpus/object/truncated.elf-data").as_slice(),
        include_bytes!("../fuzz/corpus/object/overflow-table.elf-data").as_slice(),
        include_bytes!("../fuzz/corpus/object/bad-relocation.elf-data").as_slice(),
    ] {
        assert!(!fuzz::check(fuzz::Target::Object, bad));
    }
    assert!(!fuzz::check(fuzz::Target::Object, &vec![0; fuzz::MAX_INPUT + 1]));
}

#[test]
fn generated_public_models_validate_or_roundtrip_without_losing_link_metadata() {
    let mut accepted = 0;
    for section in 0..4 {
        for kind in 0..6 {
            for addend in [0, 127, 128, 255] {
                let mut bytes = b"WOM1".to_vec();
                bytes.extend([0, section, 0, 0, 0, kind, 0, addend]);
                let valid = fuzz::check(fuzz::Target::Object, &bytes);
                assert_eq!(valid, section != 3, "BSS relocation must be rejected");
                accepted += usize::from(valid);
                bytes[4 + 5] = 255;
                assert!(
                    fuzz::check(fuzz::Target::Object, &bytes),
                    "each relocation-free section remains valid"
                );
            }
        }
    }
    assert_eq!(accepted, 72);
    for kind in [0, 4, 8] {
        for visibility in [0, 128] {
            for local in [0, 64] {
                let mut bytes = b"WOM1".to_vec();
                bytes.extend([0, 1 | kind | visibility | local, 0, 4, 8, 0, 24, 255]);
                assert!(fuzz::check(fuzz::Target::Object, &bytes));
            }
        }
    }
    for flags in 0..=255 {
        let mut bytes = b"WOM1".to_vec();
        bytes.extend([flags, 1, 0, 0, 0, 0, 0, 255]);
        fuzz::check(fuzz::Target::Object, &bytes);
    }
    for overflow in [3, 4, 6] {
        let mut bytes = b"WOM1".to_vec();
        bytes.extend([0, 0, 0, 0, 0, 0, 0, 0]);
        bytes[4 + overflow] = 255;
        assert!(!fuzz::check(fuzz::Target::Object, &bytes));
    }
}

#[test]
fn nonfile_bss_offset_beyond_eof_does_not_invalidate_valid_writer_output() {
    let seed = include_bytes!("../fuzz/corpus/object/aligned-bss.model");
    assert!(fuzz::check(fuzz::Target::Object, seed));
}
