//! Independent, test-only decoding of the ELF fields asserted by the fixtures.
#![allow(dead_code)]
pub fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes(b[p..p + 2].try_into().unwrap())
}

pub fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes(b[p..p + 4].try_into().unwrap())
}

pub fn u64_at(b: &[u8], p: usize) -> u64 {
    u64::from_le_bytes(b[p..p + 8].try_into().unwrap())
}

fn string(b: &[u8], p: usize) -> String {
    let n = b[p..].iter().position(|v| *v == 0).unwrap();
    String::from_utf8(b[p..p + n].to_vec()).unwrap()
}

#[derive(Debug)]
pub struct Section {
    pub index: usize,
    pub name: String,
    pub kind: u32,
    pub offset: usize,
    pub size: usize,
    pub link: usize,
    pub info: usize,
    pub entsize: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Symbol {
    pub index: usize,
    pub name: String,
    pub value: u64,
    pub size: u64,
    pub binding: u8,
    pub kind: u8,
    pub visibility: u8,
    pub section: usize,
}

pub fn sections(b: &[u8]) -> Vec<Section> {
    assert_eq!(&b[..7], b"\x7fELF\x02\x01\x01");
    let table = u64_at(b, 40) as usize;
    assert_eq!(u16_at(b, 58), 64);
    let names_header = table + usize::from(u16_at(b, 62)) * 64;
    let names = u64_at(b, names_header + 24) as usize;
    (0..usize::from(u16_at(b, 60)))
        .map(|index| {
            let h = table + index * 64;
            Section {
                index,
                name: string(b, names + u32_at(b, h) as usize),
                kind: u32_at(b, h + 4),
                offset: u64_at(b, h + 24) as usize,
                size: u64_at(b, h + 32) as usize,
                link: u32_at(b, h + 40) as usize,
                info: u32_at(b, h + 44) as usize,
                entsize: u64_at(b, h + 56) as usize,
            }
        })
        .collect()
}

pub fn symbols(b: &[u8], sections: &[Section]) -> Vec<Symbol> {
    let symtab = sections.iter().find(|s| s.kind == 2).unwrap();
    let strings = &sections[symtab.link];
    assert_eq!(strings.kind, 3);
    assert_eq!(strings.name, ".strtab");
    assert_eq!(symtab.entsize, 24);
    assert_eq!(symtab.size % 24, 0);
    (0..symtab.size / 24)
        .map(|index| {
            let p = symtab.offset + index * 24;
            Symbol {
                index,
                name: string(b, strings.offset + u32_at(b, p) as usize),
                value: u64_at(b, p + 8),
                size: u64_at(b, p + 16),
                binding: b[p + 4] >> 4,
                kind: b[p + 4] & 15,
                visibility: b[p + 5],
                section: usize::from(u16_at(b, p + 6)),
            }
        })
        .collect()
}
