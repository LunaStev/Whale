// SPDX-License-Identifier: MPL-2.0
//! COFF relocation identities and in-place addends (Microsoft PE/COFF contract).
use crate::{Machine, RelocKind};

pub(crate) fn encoding(kind: RelocKind, machine: Machine) -> Result<(u32, usize), String> {
    use Machine::*;

    use RelocKind::*;
    Ok(match (machine, kind) {
        (X86_64, Absolute64) => (1, 8),
        (X86_64, Absolute32) => (2, 4),
        (X86_64, Relative32 | PLT32) => (4, 4),
        (AArch64, Absolute64) => (14, 8),
        (AArch64, Absolute32) => (1, 4),
        (AArch64, CoffSpecific { machine: AArch64, code: code @ (3 | 5 | 15) }) => (code.into(), 4),
        _ => return Err(format!("unsupported COFF relocation {kind:?} for {machine:?}")),
    })
}

pub(super) fn apply(bytes: &mut [u8], kind: RelocKind, addend: i64) -> Result<(), String> {
    match kind {
        RelocKind::CoffSpecific { code, .. } => {
            let word = u32::from_le_bytes(bytes.try_into().unwrap());
            let (mask, value) = match code {
                3 => {
                    if word & 0x7c000000 != 0x14000000 {
                        return Err("BRANCH26 requires B/BL".into());
                    }
                    (0x03ffffff, branch(addend, 26)?)
                },
                15 => {
                    if !(word & 0xff000010 == 0x54000000 || word & 0x7e000000 == 0x34000000) {
                        return Err("BRANCH19 requires conditional B/CBZ/CBNZ".into());
                    }
                    (0x00ffffe0, branch(addend, 19)? << 5)
                },
                5 => {
                    if word & 0x9f000000 != 0x10000000 {
                        return Err("REL21 requires ADR".into());
                    }
                    if !(-(1 << 20)..(1 << 20)).contains(&addend) {
                        return Err("ADR addend exceeds signed 21 bits".into());
                    }
                    let bits = addend as u32 & 0x1fffff;
                    (0x60ffffe0, (bits & 3) << 29 | (bits >> 2) << 5)
                },
                _ => return Err("unsupported COFF instruction relocation".into()),
            };
            bytes.copy_from_slice(&((word & !mask) | value).to_le_bytes());
        },
        RelocKind::Absolute64 => bytes.copy_from_slice(&(addend as u64).to_le_bytes()),
        RelocKind::Absolute32 => {
            if !(i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(&addend) {
                return Err("COFF absolute addend exceeds 32 bits".into());
            }
            bytes.copy_from_slice(&(addend as u32).to_le_bytes());
        },
        RelocKind::Relative32 | RelocKind::PLT32 => {
            // ELF model is S+A-P, COFF REL32 uses the end of the field.
            let value = addend
                .checked_add(4)
                .and_then(|n| i32::try_from(n).ok())
                .ok_or("COFF relative addend exceeds signed 32 bits")?;
            bytes.copy_from_slice(&value.to_le_bytes());
        },
        _ => return Err("unsupported COFF addend".into()),
    }
    Ok(())
}

fn branch(addend: i64, bits: u32) -> Result<u32, String> {
    if addend & 3 != 0 || !(-(1i64 << (bits + 1))..(1i64 << (bits + 1))).contains(&addend) {
        return Err(format!("unaligned/out-of-range COFF branch{bits} addend"));
    }
    Ok((addend >> 2) as u32 & ((1u32 << bits) - 1))
}
