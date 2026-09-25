use linker::core::layout::Layout;
use object::{ObjectFile, ObjectFormat, SectionKind};

#[test]
fn separate_file_and_memory_cursors_preserve_every_input_mapping() {
    let mut a = ObjectFile::new(ObjectFormat::ELF64);
    let text = a.add_section(".text", SectionKind::Text, 16);
    a.sections[text].data = vec![0x90; 3];
    let bss = a.add_section(".bss", SectionKind::Bss, 32);
    a.sections[bss].zero_fill = 33;
    let mut b = ObjectFile::new(ObjectFormat::ELF64);
    let data = b.add_section(".data", SectionKind::Data, 8);
    b.sections[data].data = vec![1, 2];
    let layout = Layout::compute(&[a, b], 0x1003).unwrap();
    assert_eq!(layout.section_offsets, [vec![0x1010, 0x1020], vec![0x1048]]);
    assert_eq!(layout.section_sizes, [3, 33, 2]);
    assert_eq!((layout.file_size, layout.memory_size), (10, 0x47));
    let mappings: Vec<_> = layout
        .sections
        .iter()
        .map(|s| {
            (
                s.object_index,
                s.section_index,
                s.file_offset,
                s.file_size,
                s.memory_size,
            )
        })
        .collect();
    assert_eq!(
        mappings,
        [(0, 0, 0, 3, 3), (0, 1, 32, 0, 33), (1, 0, 8, 2, 2)]
    );
}

#[test]
fn invalid_alignment_and_address_overflow_are_errors() {
    for (base, align, size) in [(0, 3, 0), (u64::MAX, 8, 0), (1, 1, u64::MAX)] {
        let mut object = ObjectFile::new(ObjectFormat::ELF64);
        let section = object.add_section(".bss", SectionKind::Bss, align);
        object.sections[section].zero_fill = size;
        assert!(Layout::compute(&[object], base).is_err());
    }
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    object.add_section(".bss", SectionKind::Bss, 0);
    let layout = Layout::compute(&[object], u64::MAX).unwrap();
    assert_eq!(layout.sections[0].align, 1);
    assert_eq!(layout.memory_size, 0);
}

#[test]
fn huge_bss_changes_memory_extent_without_file_growth() {
    let mut object = ObjectFile::new(ObjectFormat::ELF64);
    let index = object.add_section(".bss", SectionKind::Bss, 16);
    object.sections[index].zero_fill = 1 << 40;
    let layout = Layout::compute(&[object], 0x1000).unwrap();
    assert_eq!((layout.file_size, layout.memory_size), (0, 1 << 40));
}
