use object::*;

fn fixture(sections: usize) -> ObjectFile {
    let mut obj = ObjectFile::new(ObjectFormat::ELF64);
    for n in 0..sections {
        let section = obj.add_section(&format!(".data{n}"), SectionKind::Data, 8);
        obj.sections[section].data = vec![0; 32];
    }
    for offset in [24, 0, 16, 8] {
        for section in (0..sections).rev() {
            obj.relocations.push(ObjectRelocation {
                section_index: section,
                offset,
                symbol: "external".into(),
                kind: RelocKind::Absolute64,
                addend: offset as i64 - 32,
            });
        }
    }
    obj
}

fn u64_at(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().unwrap())
}

fn u32_at(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().unwrap())
}

#[test]
fn interleaved_relocations_keep_section_order_offsets_and_signed_addends() {
    let mut obj = fixture(64);
    obj.add_section(".empty", SectionKind::Data, 1);
    let bss = obj.add_section(".bss", SectionKind::Bss, 8);
    obj.sections[bss].zero_fill = 1024;
    let bytes = obj.write().unwrap();
    assert_eq!(obj.write().unwrap(), bytes);
    let shoff = u64_at(&bytes, 40) as usize;
    let count = u16::from_le_bytes(bytes[60..62].try_into().unwrap()) as usize;
    let relas: Vec<_> = (0..count)
        .map(|n| shoff + n * 64)
        .filter(|&offset| u32_at(&bytes, offset + 4) == 4)
        .collect();
    assert_eq!(relas.len(), 64);
    for (section, &header) in relas.iter().enumerate() {
        assert_eq!(u32_at(&bytes, header + 44) as usize, section + 1);
        assert_eq!(u64_at(&bytes, header + 32), 4 * 24);
        let data = u64_at(&bytes, header + 24) as usize;
        for (n, offset) in [24u64, 0, 16, 8].into_iter().enumerate() {
            assert_eq!(u64_at(&bytes, data + n * 24), offset);
            assert_eq!(u64_at(&bytes, data + n * 24 + 8) & 0xffff_ffff, 1);
            assert_eq!(u64_at(&bytes, data + n * 24 + 16) as i64, offset as i64 - 32);
        }
    }
    obj.relocations[0].section_index = 999;
    assert!(obj.write().unwrap_err().contains("invalid section"));
}

#[test]
#[ignore = "repeatable ELF scaling benchmark; run with --ignored --nocapture"]
fn benchmark_many_section_relocations() {
    for sections in [125, 500, 2000] {
        let obj = fixture(sections);
        let mut durations = Vec::new();
        let mut size = 0;
        for _ in 0..3 {
            let start = std::time::Instant::now();
            size = obj.write().unwrap().len();
            durations.push(start.elapsed());
        }
        durations.sort();
        println!(
            "ELF sections={sections} relocations={} output_bytes={size} median_us={}",
            obj.relocations.len(),
            durations[1].as_micros()
        );
    }
}
