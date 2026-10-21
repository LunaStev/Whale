# Numeric conversion contract (semantics 1)

Integer types are signed/unsigned 1, 8, 16, 32, 64 and 128 bits. Signed
integers use two's complement; bool is distinct from i1 and u1. Floating
storage is IEEE binary16, binary32 or binary64. No cast invokes undefined
behavior, saturation, host rounding modes, or a floating exception flag.

| Opcode | Legal source → destination | Result / trap |
| --- | --- | --- |
| zext | narrower integer → wider integer; bool → integer except i1 | Extend the raw pattern with zeros (bool is 0/1). |
| sext | narrower integer → wider integer | Extend the source high bit, regardless of signedness labels. |
| trunc | wider integer → narrower integer | Discard high bits; interpret remaining pattern in destination type. |
| checked_int | integer → integer, any width/sign | Preserve mathematical value; trap `InvalidConversion` if outside destination interval. |
| itof_s / itof_u | signed / unsigned integer → float | Round exact integer once to nearest, ties to even. Overflow gives signed infinity. |
| ftoi_s / ftoi_u | float → signed / unsigned integer | Truncate toward zero, then range-check. NaN, infinity and out-of-range integer results trap `InvalidConversion`. Negative fractions that truncate to zero are valid unsigned conversions. |
| fext / ftrunc | float → strictly wider / narrower float | Round once to nearest, ties to even (extension is exact for finite values). Overflow gives signed infinity; gradual underflow, signed zero retained. |
| bitcast | equal-bit-width integer/float pair, or data pointer → data pointer | Preserve every storage bit including signaling NaN payloads. Pointer identity/authority is retained. Bool and function pointers are excluded. |
| ptrtoint | data pointer → integer | Take numeric address modulo destination width. Does not transfer pointer authority. |
| inttoptr | integer → data pointer | Low 64 address bits become an untracked raw address. Zero is null. It cannot manufacture allocation identity or dereference/call authority. |

Floating **numeric** conversions canonicalize any NaN to positive quiet NaN
(f16 `0x7e00`, f32 `0x7fc00000`, f64 `0x7ff8000000000000`). Plain storage and
bitcasts preserve bits. Conversion traps occur at the cast, after earlier
observable effects and before later instructions. See tracked-memory tests for
raw-address rejection and pointer bitcast identity; numerical address equality
alone never establishes memory validity.

Shared boundary vectors for conversion implementations:

| Input / conversion | Expected |
| --- | --- |
| i8 -1 → i16, sext | -1 |
| u8 255 → i16, zext | 255 |
| u16 256 → u8, trunc / checked_int | 0 / trap |
| i1 -1 → u1, checked_int | trap |
| u1 1 → i1, checked_int | trap |
| u128 max → i128, checked_int | trap |
| i128 min → i128, checked_int | unchanged |
| u32 16777217 → f32 | 0x4b800000 (tie to even) |
| f64 0x3ff0020000000000 → f16 | 0x3c00 (tie to even) |
| f32 -0 → u128 | 0 |
| f32 -0.5 → u8 | 0 |
| f32 256 → u8 | trap |
| f16 ±infinity or any NaN → i128 | trap |
| f64 ±0 → f16 / f32 | corresponding signed zero |
| signaling NaN → wider/narrower float | destination canonical quiet NaN |

Frontend AST format 5 introduces `Cast {value, ty, kind}`. `Numeric` supports
integer pairs only: source-signed extension, truncation, or equal-width bitcast.
`Checked` selects `checked_int`. Other pairs and bool conversions are rejected
by lowering. Casts in compile-time declarations are explicitly unsupported.
Typed IR format 8 introduces `checked_int`; readers retain older formats.
