// SPDX-License-Identifier: MPL-2.0
//! AMD64 reverse analysis with a Wave implementation and a checked Rust boundary.
//! Ordinary builds report an unavailable core; no Rust decoder substitutes for it.
mod cfg;

mod model;

pub use cfg::*;

pub use model::*;

use std::fmt;

pub const ABI_VERSION: u64 = 1;

pub const DECODE_WORDS: usize = 96;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    CoreUnavailable,
    Invalid { offset: usize },
    Truncated { offset: usize },
    Unsupported { offset: usize },
    Overlong { offset: usize },
    Capacity,
    AbiMismatch { expected: u64, actual: u64 },
    InvalidOutput(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CoreUnavailable => f.write_str("Wave reverse core unavailable; build tools/build_wave_reverse.py, set WHALE_WAVE_REVERSE_DIR and enable wave-reverse"),
            Self::Invalid { offset } => write!(f, "invalid AMD64 encoding or input range at byte {offset}"),
            Self::Truncated { offset } => write!(f, "truncated AMD64 instruction at byte {offset}"),
            Self::Unsupported { offset } => write!(f, "unsupported AMD64 encoding at byte {offset}"),
            Self::Overlong { offset } => write!(f, "AMD64 instruction exceeds 15 bytes at byte {offset}"),
            Self::Capacity => f.write_str("Wave reverse output capacity exhausted"),
            Self::AbiMismatch { expected, actual } => write!(f, "Wave reverse ABI mismatch: expected {expected}, got {actual}"),
            Self::InvalidOutput(reason) => write!(f, "invalid Wave reverse result: {reason}"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub const fn backend_available() -> bool {
    cfg!(whale_wave_reverse)
}

pub const fn backend_name() -> &'static str {
    if backend_available() {
        "wave-llvm-bootstrap"
    } else {
        "unavailable"
    }
}

#[cfg(whale_wave_reverse)]
extern "C" {
    fn whale_wave_reverse_abi() -> u64;

    fn whale_wave_reverse_decode(
        abi: u64,
        input: *const u8,
        length: u64,
        offset: u64,
        output: *mut u64,
        capacity: u64,
    ) -> i32;
}

/// Decode one instruction at a checked slice offset. Input and its original bytes
/// remain caller-owned; any failure publishes no partial instruction.
pub fn decode(input: &[u8], offset: usize) -> Result<Instruction, DecodeError> {
    #[cfg(not(whale_wave_reverse))]
    {
        let _ = (input, offset);
        Err(DecodeError::CoreUnavailable)
    }
    #[cfg(whale_wave_reverse)]
    {
        if offset > input.len() {
            return Err(DecodeError::Invalid { offset });
        }
        // SAFETY: the fixed no-argument capability entry has no memory operands.
        let actual = unsafe { whale_wave_reverse_abi() };
        if actual != ABI_VERSION {
            return Err(DecodeError::AbiMismatch { expected: ABI_VERSION, actual });
        }
        let mut record = [0u64; DECODE_WORDS];
        // SAFETY: both buffers live through this call, output is aligned and has
        // exactly DECODE_WORDS writable words, and offset is inside the input.
        // ABI1 requires the kernel to read <=15 bytes inside the supplied slice
        // and to commit output only after complete success. The linked, pinned
        // Wave archive is trusted implementation code, not an untrusted plugin.
        let status = unsafe {
            whale_wave_reverse_decode(
                ABI_VERSION,
                input.as_ptr(),
                input.len() as u64,
                offset as u64,
                record.as_mut_ptr(),
                DECODE_WORDS as u64,
            )
        };
        match status {
            0 => decode_record(input, offset, &record),
            1 => Err(DecodeError::Invalid { offset }),
            2 => Err(DecodeError::Truncated { offset }),
            3 => Err(DecodeError::Unsupported { offset }),
            4 => Err(DecodeError::Overlong { offset }),
            5 => Err(DecodeError::Capacity),
            6 => Err(DecodeError::AbiMismatch { expected: ABI_VERSION, actual }),
            _ => Err(DecodeError::InvalidOutput("unknown status")),
        }
    }
}

#[cfg(whale_wave_reverse)]
fn decode_record(
    input: &[u8],
    offset: usize,
    r: &[u64; DECODE_WORDS],
) -> Result<Instruction, DecodeError> {
    let bad = DecodeError::InvalidOutput;
    if r[0] != ABI_VERSION || r[1] != DECODE_WORDS as u64 {
        return Err(bad("record header"));
    }
    let length = u8::try_from(r[2]).map_err(|_| bad("instruction length"))?;
    if !(1..=15).contains(&length) {
        return Err(bad("instruction length"));
    }
    let end = offset
        .checked_add(length as usize)
        .filter(|end| *end <= input.len())
        .ok_or(bad("input range"))?;
    let bytes = &input[offset..end];
    let mut original = [0; 15];
    original[..bytes.len()].copy_from_slice(bytes);
    let opcode = Opcode::from_code(r[3]).ok_or(bad("opcode"))?;
    let condition =
        if r[4] == 16 { None } else { Some(Condition::from_code(r[4]).ok_or(bad("condition"))?) };
    if condition.is_some() != matches!(opcode, Opcode::Jcc | Opcode::Setcc | Opcode::Cmovcc) {
        return Err(bad("condition opcode"));
    }
    let operand_count =
        usize::try_from(r[5]).ok().filter(|n| *n <= 3).ok_or(bad("operand count"))?;
    let prefix_count =
        usize::try_from(r[7]).ok().filter(|n| *n < bytes.len()).ok_or(bad("prefix count"))?;
    if r[8] != prefix_count as u64 || !matches!(r[9], 1 | 2) || r[8] + r[9] > u64::from(length) {
        return Err(bad("opcode range"));
    }
    let mut prefixes = Vec::with_capacity(prefix_count);
    for (i, byte) in bytes.iter().take(prefix_count).enumerate() {
        if r[24 + i] != u64::from(*byte) {
            return Err(bad("prefix bytes"));
        }
        prefixes.push(*byte);
    }
    let rex = if r[20] == 0 {
        None
    } else {
        Some(
            u8::try_from(r[20])
                .ok()
                .filter(|b| (0x40..=0x4f).contains(b))
                .ok_or(bad("REX byte"))?,
        )
    };
    let byte_field = |at: usize| -> Result<Option<EncodingByte>, DecodeError> {
        if r[at] == 0 {
            return Ok(None);
        }
        let off = usize::try_from(r[at] - 1)
            .ok()
            .filter(|n| *n < bytes.len())
            .ok_or(bad("encoding byte range"))?;
        if r[at + 1] != u64::from(bytes[off]) {
            return Err(bad("encoding byte bits"));
        }
        Ok(Some(EncodingByte { offset: off as u8, value: bytes[off] }))
    };
    let field = |at: usize| -> Result<Option<EncodingField>, DecodeError> {
        if r[at] == 0 {
            return Ok(None);
        }
        let off = usize::try_from(r[at] - 1).map_err(|_| bad("field offset"))?;
        let width = usize::try_from(r[at + 1])
            .ok()
            .filter(|n| matches!(n, 1 | 2 | 4 | 8))
            .ok_or(bad("field width"))?;
        let end = off.checked_add(width).filter(|n| *n <= bytes.len()).ok_or(bad("field range"))?;
        let bits = bytes[off..end]
            .iter()
            .enumerate()
            .fold(0u64, |v, (i, b)| v | (u64::from(*b) << (i * 8)));
        if bits != r[at + 2] {
            return Err(bad("field bits"));
        }
        Ok(Some(EncodingField { offset: off as u8, width: width as u8, bits }))
    };
    let encoding = Encoding {
        prefixes,
        rex,
        opcode_offset: r[8] as u8,
        opcode_len: r[9] as u8,
        modrm: byte_field(10)?,
        sib: byte_field(12)?,
        displacement: field(14)?,
        immediate: field(17)?,
    };
    let mut operands = Vec::with_capacity(operand_count);
    let gpr = |code: u64| -> Result<Register, DecodeError> {
        let index = u8::try_from(code).ok().filter(|n| *n < 16).ok_or(bad("address register"))?;
        Ok(Register { class: RegisterClass::General, index, width: 64, high8: false })
    };
    for operand in 0..operand_count {
        let at = 40 + operand * 16;
        let width = u16::try_from(r[at + 1]).map_err(|_| bad("operand width"))?;
        let value = match r[at] {
            kind @ (1 | 2) => {
                let index = u8::try_from(r[at + 2])
                    .ok()
                    .filter(|n| *n < 16)
                    .ok_or(bad("register index"))?;
                let high8 = match r[at + 3] {
                    0 => false,
                    1 => true,
                    _ => return Err(bad("high byte flag")),
                };
                if kind == 1
                    && (!matches!(width, 8 | 16 | 32 | 64)
                        || (high8 && (width != 8 || index >= 4 || rex.is_some())))
                {
                    return Err(bad("GPR shape"));
                }
                if kind == 2 && (width != 128 || high8) {
                    return Err(bad("XMM shape"));
                }
                Operand::Register(Register {
                    class: if kind == 1 { RegisterClass::General } else { RegisterClass::Xmm },
                    index,
                    width,
                    high8,
                })
            },
            3 => {
                if !matches!(width, 8 | 16 | 32 | 64) {
                    return Err(bad("memory width"));
                }
                let base = match r[at + 2] {
                    0 | 17 => None,
                    code @ 1..=16 => Some(gpr(code - 1)?),
                    _ => return Err(bad("memory base")),
                };
                let index = match r[at + 3] {
                    0 => None,
                    code @ 1..=16 => Some(gpr(code - 1)?),
                    _ => return Err(bad("memory index")),
                };
                let scale = u8::try_from(r[at + 4])
                    .ok()
                    .filter(|n| matches!(n, 1 | 2 | 4 | 8))
                    .ok_or(bad("memory scale"))?;
                Operand::Memory(MemoryOperand {
                    width,
                    base,
                    index,
                    scale,
                    displacement: r[at + 5] as i64,
                    rip_relative: r[at + 2] == 17,
                })
            },
            4 => {
                let encoded_width = u8::try_from(r[at + 6])
                    .ok()
                    .filter(|n| matches!(n, 0 | 8 | 16 | 32 | 64))
                    .ok_or(bad("immediate width"))?;
                if !matches!(width, 8 | 16 | 32 | 64) || r[at + 3] > 1 {
                    return Err(bad("immediate shape"));
                }
                Operand::Immediate(Immediate {
                    value: r[at + 2],
                    width,
                    encoded_width,
                    sign_extended: r[at + 3] == 1,
                })
            },
            5 => {
                let encoded_width = u8::try_from(r[at + 6])
                    .ok()
                    .filter(|n| matches!(n, 8 | 32))
                    .ok_or(bad("relative width"))?;
                Operand::Relative(Relative { displacement: r[at + 2] as i64, encoded_width })
            },
            _ => return Err(bad("operand kind")),
        };
        operands.push(value);
    }
    let relative = operands.iter().find_map(|o| {
        if let Operand::Relative(r) = o {
            Some(r.displacement)
        } else {
            None
        }
    });
    let required = || relative.ok_or(bad("missing relative operand"));
    let flow = match r[6] {
        0 => Flow::FallThrough,
        1 => Flow::Call { relative: Some(required()?) },
        2 => Flow::Call { relative: None },
        3 => Flow::Jump { relative: Some(required()?) },
        4 => Flow::Jump { relative: None },
        5 => Flow::Conditional { relative: required()? },
        6 => Flow::Loop { relative: required()? },
        7 => Flow::Return,
        8 => Flow::Trap,
        9 => Flow::Syscall,
        _ => return Err(bad("flow kind")),
    };
    Ok(Instruction { offset, length, bytes: original, opcode, condition, operands, encoding, flow })
}

pub mod format;

pub mod image;
