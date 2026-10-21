use object::{Endian, Machine, ObjectFile, ObjectFormat, ObjectTarget, Target};

#[test]
fn amd64_identity_matches_independently_decoded_elf_header() {
    let target = Target::X86_64WhaleLinux.object_target();
    let explicit = ObjectFile::with_target(target);
    let legacy = ObjectFile::new(ObjectFormat::ELF64);
    assert_eq!(legacy.target, target);
    let bytes = explicit.write().unwrap();
    assert_eq!(bytes, legacy.write().unwrap());
    assert_eq!(&bytes[..7], b"\x7fELF\x02\x01\x01");
    assert_eq!(u16::from_le_bytes(bytes[16..18].try_into().unwrap()), 1); // ET_REL
    assert_eq!(u16::from_le_bytes(bytes[18..20].try_into().unwrap()), 62); // EM_X86_64
}

#[test]
fn both_writer_entry_points_reject_unsupported_object_identities() {
    let amd64 = Target::X86_64WhaleLinux.object_target();
    for target in
        [ObjectTarget { endian: Endian::Big, ..amd64 }, ObjectTarget { address_bits: 32, ..amd64 }]
    {
        let object = ObjectFile::with_target(target);
        assert_eq!(object.target, target);
        for result in [object.write(), object::formats::elf::write_elf(&object)] {
            let error = result.unwrap_err();
            assert!(error.contains("unsupported object target"), "{error}");
        }
    }
    // Public metadata can change after construction; serialization must recheck it.
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.target.endian = Endian::Big;
    assert!(object.write().is_err());
}

#[test]
fn multi_isa_writer_preserves_machine_and_abi_without_enabling_amd64_linking() {
    for (machine, elf, flags) in
        [(Machine::AArch64, 183, 0), (Machine::RiscV64, 243, 0), (Machine::LoongArch64, 258, 0x41)]
    {
        let target = ObjectTarget::elf64(machine);
        assert!(target.validate().is_ok());
        assert!(target.validate_amd64().is_err());
        let bytes = ObjectFile::with_target(target).write().unwrap();
        assert_eq!(u16::from_le_bytes(bytes[18..20].try_into().unwrap()), elf);
        assert_eq!(u32::from_le_bytes(bytes[48..52].try_into().unwrap()), flags);
        assert!(ObjectFile::read(&bytes).is_err(), "AMD64 reader must reject another ISA");
    }
}

#[test]
fn invalid_encoding_diagnostics_describe_serialization_and_consumer_contracts() {
    for machine in [Machine::X86_64, Machine::AArch64, Machine::RiscV64, Machine::LoongArch64] {
        let valid = ObjectTarget::elf64(machine);
        assert!(valid.validate().is_ok());
        for invalid in [
            ObjectTarget { endian: Endian::Big, ..valid },
            ObjectTarget { address_bits: 32, ..valid },
        ] {
            let message = invalid.validate().unwrap_err().to_string();
            for expected in [
                "ELF64",
                "little endian",
                "64-bit addresses",
                "X86_64",
                "AArch64",
                "RiscV64",
                "LoongArch64",
            ] {
                assert!(message.contains(expected), "missing {expected}: {message}");
            }
        }
        if machine != Machine::X86_64 {
            let message = valid.validate_amd64().unwrap_err().to_string();
            assert!(message.contains("this operation requires"));
            assert!(message.contains("AMD64"));
        }
    }
}
