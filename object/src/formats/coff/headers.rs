// SPDX-License-Identifier: MPL-2.0
use crate::Machine;

pub(super) fn u16_at(out: &mut [u8], at: usize, value: u16) {
    out[at..at + 2].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn u32_at(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub(super) fn file(out: &mut [u8], machine: Machine, sections: u16, symbols: u32, count: u32) {
    u16_at(out, 0, if machine == Machine::X86_64 { 0x8664 } else { 0xaa64 });
    u16_at(out, 2, sections);
    u32_at(out, 8, symbols);
    u32_at(out, 12, count);
    // No optional PE header, no timestamp and no executable characteristics.
}
