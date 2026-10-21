// SPDX-License-Identifier: MPL-2.0
//! Numeric conversion using the arithmetic packer's single nearest-even round.
use super::Format;

use crate::{CastOp, ConstValue, FloatBits, Type};

fn format(ty: &Type) -> Format {
    Format::of(match ty {
        Type::F16 => FloatBits::F16(0),
        Type::F32 => FloatBits::F32(0),
        Type::F64 => FloatBits::F64(0),
        _ => unreachable!("verified float type"),
    })
}

/// None is a defined invalid numerical conversion, never a poison value.
pub fn convert(op: &CastOp, src: &Type, dst: &Type, value: &ConstValue) -> Option<ConstValue> {
    use crate::interpreter::integer;
    match op {
        CastOp::FExt | CastOp::FTrunc => {
            let ConstValue::F(value) = value else { unreachable!("verified float operand") };
            let parts = Format::of(*value).unpack(*value);
            let f = format(dst);
            Some(ConstValue::F(if parts.nan {
                f.nan()
            } else if parts.infinity {
                f.special(parts.negative, true)
            } else {
                f.pack(parts.negative, parts.sig, parts.exp)
            }))
        },
        CastOp::IToF_S | CastOp::IToF_U => {
            let (bits, signed) = integer::shape(src).unwrap();
            let raw = integer::raw(value, bits);
            let negative = signed && raw & (1u128 << (bits - 1)) != 0;
            let magnitude = if negative { 0u128.wrapping_sub(raw) } else { raw };
            // Mask after taking two's-complement magnitude for narrow negatives.
            let magnitude = if bits < 128 { magnitude & ((1u128 << bits) - 1) } else { magnitude };
            Some(ConstValue::F(format(dst).pack(negative, magnitude, 0)))
        },
        CastOp::FToI_S | CastOp::FToI_U => {
            let ConstValue::F(value) = value else { unreachable!("verified float operand") };
            let p = Format::of(*value).unpack(*value);
            if p.nan || p.infinity {
                return None;
            }
            let magnitude = if p.sig == 0 {
                0
            } else if p.exp >= 0 {
                let shift = p.exp as u32;
                if shift >= 128 || p.sig > u128::MAX >> shift {
                    return None;
                }
                p.sig << shift
            } else {
                p.sig.checked_shr((-p.exp) as u32).unwrap_or(0)
            };
            let (bits, signed) = integer::shape(dst).unwrap();
            let maximum = if signed {
                (1u128 << (bits - 1)) - u128::from(!p.negative)
            } else if bits == 128 {
                u128::MAX
            } else {
                (1u128 << bits) - 1
            };
            if (p.negative && !signed && magnitude != 0) || magnitude > maximum {
                return None;
            }
            Some(integer::pack(
                dst,
                if p.negative { 0u128.wrapping_sub(magnitude) } else { magnitude },
            ))
        },
        _ => None,
    }
}
