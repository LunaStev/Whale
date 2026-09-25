// SPDX-License-Identifier: MPL-2.0

/// Exact IEEE storage bits. Equality compares storage, including NaN payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FloatBits {
    F16(u16),
    F32(u32),
    F64(u64),
}

impl FloatBits {
    pub fn width(self) -> u16 {
        match self {
            Self::F16(_) => 16,
            Self::F32(_) => 32,
            Self::F64(_) => 64,
        }
    }

    pub fn bits(self) -> u64 {
        match self {
            Self::F16(x) => x.into(),
            Self::F32(x) => x.into(),
            Self::F64(x) => x,
        }
    }

    /// Exact-width hexadecimal storage, not a decimal floating-point number.
    pub fn parse(width: u16, text: &str) -> Result<Self, String> {
        let digits = match width {
            16 => 4,
            32 => 8,
            64 => 16,
            _ => return Err("float width must be 16, 32 or 64".into()),
        };
        let hex = text
            .strip_prefix("0x")
            .ok_or("float value must start with 0x")?;
        if hex.len() != digits || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!(
                "f{width} requires exactly {digits} hexadecimal digits"
            ));
        }
        let bits = u64::from_str_radix(hex, 16).map_err(|e| e.to_string())?;
        Ok(match width {
            16 => Self::F16(bits as u16),
            32 => Self::F32(bits as u32),
            _ => Self::F64(bits),
        })
    }

    /// Numeric convenience conversion. Use the variants to preserve source bits.
    pub fn from_f64(width: u16, value: f64) -> Result<Self, String> {
        Ok(match width {
            16 => Self::F16(f64_to_f16(value)),
            32 => Self::F32((value as f32).to_bits()),
            64 => Self::F64(value.to_bits()),
            _ => return Err("float width must be 16, 32 or 64".into()),
        })
    }

    pub fn to_f64(self) -> f64 {
        match self {
            Self::F64(bits) => f64::from_bits(bits),
            Self::F32(bits) => f32::from_bits(bits) as f64,
            Self::F16(bits) => {
                let sign = u64::from(bits >> 15) << 63;
                let exponent = (bits >> 10) & 31;
                let fraction = u64::from(bits & 1023);
                if exponent == 31 {
                    return f64::from_bits(sign | (0x7ff << 52) | (fraction << 42));
                }
                let magnitude = if exponent == 0 {
                    (fraction as f64) * 2f64.powi(-24)
                } else {
                    (1024 + fraction) as f64 * 2f64.powi(i32::from(exponent) - 25)
                };
                if sign != 0 {
                    -magnitude
                } else {
                    magnitude
                }
            }
        }
    }
}

impl std::fmt::Display for FloatBits {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "0x{:0width$x}",
            self.bits(),
            width = usize::from(self.width() / 4)
        )
    }
}

// Direct binary64 -> binary16 rounding avoids the intermediate f32 double round.
fn f64_to_f16(value: f64) -> u16 {
    let bits = value.to_bits();
    let sign = ((bits >> 48) & 0x8000) as u16;
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1u64 << 52) - 1);
    if exp == 0x7ff {
        return sign | if fraction == 0 { 0x7c00 } else { 0x7e00 };
    }
    if exp == 0 {
        return sign;
    }
    let unbiased = exp - 1023;
    if unbiased > 15 {
        return sign | 0x7c00;
    }
    if unbiased < -25 {
        return sign;
    }
    let mantissa = fraction | (1u64 << 52);
    let shift = if unbiased < -14 {
        (28 - unbiased) as u32
    } else {
        42
    };
    let base = mantissa >> shift;
    let remainder = mantissa & ((1u64 << shift) - 1);
    let halfway = 1u64 << (shift - 1);
    let rounded = base + u64::from(remainder > halfway || (remainder == halfway && base & 1 != 0));
    if unbiased < -14 {
        sign | rounded as u16
    } else {
        sign | ((((unbiased + 14) as u16) << 10) + rounded as u16)
    }
}
