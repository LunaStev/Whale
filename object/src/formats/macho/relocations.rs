// SPDX-License-Identifier: MPL-2.0
use crate::{Machine, ObjectRelocation, RelocKind};

pub(crate) fn encoding(kind: RelocKind, machine: Machine) -> Result<(u32, usize), String> {
    match (machine, kind) {
        (
            Machine::AArch64,
            RelocKind::Absolute64 | RelocKind::MachSpecific { machine: Machine::AArch64, code: 0 },
        ) => Ok((0, 8)),
        (Machine::AArch64, RelocKind::MachSpecific { machine: Machine::AArch64, code: 2 }) => {
            Ok((2, 4))
        },
        (Machine::X86_64, RelocKind::Absolute64) => Ok((0, 8)),
        (Machine::X86_64, RelocKind::Relative32) => Ok((1, 4)),
        (Machine::X86_64, RelocKind::PLT32) => Ok((2, 4)),
        (Machine::X86_64, RelocKind::MachSpecific { machine: Machine::X86_64, code: 0 }) => {
            Ok((0, 8))
        },
        (
            Machine::X86_64,
            RelocKind::MachSpecific { machine: Machine::X86_64, code: code @ (1 | 2) },
        ) => Ok((code.into(), 4)),
        _ => Err(format!("unsupported Mach-O relocation {kind:?} for {machine:?}")),
    }
}

pub(super) fn patch(
    data: &mut [u8],
    r: &ObjectRelocation,
    machine: Machine,
) -> Result<(u32, bool, usize), String> {
    let (code, width) = encoding(r.kind, machine)?;
    let pc = code != 0;
    if width == 8 {
        data[r.offset..r.offset + 8].copy_from_slice(&(r.addend as u64).to_le_bytes());
    } else if machine == Machine::AArch64 {
        if r.offset % 4 != 0 || r.addend != 0 {
            return Err(
                "initial ARM64 Mach-O branches require aligned fields and zero addends".into()
            );
        }
        let word = u32::from_le_bytes(data[r.offset..r.offset + 4].try_into().unwrap());
        if word & 0x7c000000 != 0x14000000 {
            return Err("ARM64 Mach-O BRANCH26 requires B/BL instruction".into());
        }
        data[r.offset..r.offset + 4].copy_from_slice(&(word & !0x03ffffff).to_le_bytes());
    } else {
        let addend = r
            .addend
            .checked_add(4)
            .and_then(|v| i32::try_from(v).ok())
            .ok_or("Mach-O PC32 implicit addend exceeds signed32")?;
        data[r.offset..r.offset + 4].copy_from_slice(&addend.to_le_bytes());
    }
    Ok((code, pc, width))
}
