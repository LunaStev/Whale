// SPDX-License-Identifier: MPL-2.0
use crate::Machine;

pub(super) fn u32_at(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn u64_at(out: &mut [u8], at: usize, value: u64) {
    out[at..at + 8].copy_from_slice(&value.to_le_bytes());
}

pub(super) struct FileLayout {
    pub machine: Machine,
    pub sections: usize,
    pub data: usize,
    pub data_end: usize,
    pub memory: u64,
    pub symbols: usize,
    pub count: u32,
    pub strings: usize,
    pub string_size: usize,
    pub groups: [u32; 3],
}

pub(super) fn file(out: &mut [u8], layout: FileLayout) {
    let FileLayout {
        machine,
        sections,
        data,
        data_end,
        memory,
        symbols,
        count,
        strings,
        string_size,
        groups,
    } = layout;
    u32_at(out, 0, 0xfeedfacf);
    u32_at(out, 4, if machine == Machine::X86_64 { 0x01000007 } else { 0x0100000c });
    u32_at(out, 8, if machine == Machine::X86_64 { 3 } else { 0 });
    u32_at(out, 12, 1);
    u32_at(out, 16, 4);
    u32_at(out, 20, (200 + sections * 80) as u32);
    let at = 32;
    u32_at(out, at, 0x19);
    u32_at(out, at + 4, (72 + sections * 80) as u32);
    u64_at(out, at + 32, memory);
    u64_at(out, at + 40, data as u64);
    u64_at(out, at + 48, (data_end - data) as u64);
    u32_at(out, at + 56, 7);
    u32_at(out, at + 60, 7);
    u32_at(out, at + 64, sections as u32);
    let at = 104 + sections * 80;
    u32_at(out, at, 2);
    u32_at(out, at + 4, 24);
    u32_at(out, at + 8, symbols as u32);
    u32_at(out, at + 12, count);
    u32_at(out, at + 16, strings as u32);
    u32_at(out, at + 20, string_size as u32);
    let at = at + 24;
    u32_at(out, at, 0xb);
    u32_at(out, at + 4, 80);
    u32_at(out, at + 12, groups[0]);
    u32_at(out, at + 16, groups[0]);
    u32_at(out, at + 20, groups[1]);
    u32_at(out, at + 24, groups[0] + groups[1]);
    u32_at(out, at + 28, groups[2]);
    let at = at + 80;
    u32_at(out, at, 0x32);
    u32_at(out, at + 4, 24);
    u32_at(out, at + 8, 1);
    u32_at(out, at + 12, 11 << 16);
}
