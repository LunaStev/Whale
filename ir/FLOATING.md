# Scalar IEEE execution

FAdd/FSub/FMul/FDiv execute once at the declared binary16/32/64 precision,
rounding to nearest with ties to even. Subnormals are gradual. There is no
implicit FMA, fast math, flush-to-zero or floating exception state. Arithmetic
NaNs become the positive canonical quiet NaN listed in [CASTS.md](CASTS.md).
Division by floating zero yields the signed infinity or NaN required by IEEE;
it does not use integer division traps.

FRem is truncating-quotient remainder (`fmod`, also the LLVM
[frem contract](https://llvm.org/docs/LangRef.html#frem-instruction)), not the
nearest-integer-quotient IEEE remainder operation. The finite result has the
sign of its dividend, including zero. Infinite dividend or zero divisor gives
canonical NaN; finite dividend and infinite divisor returns the dividend.

Neg reverses the sign bit, including zeros, infinities and every NaN payload.
It differs from subtraction from zero and does not quiet signaling NaNs.
Integer Neg wraps modulo its width; signed minimum stays minimum. AST format 5
supports `Neg` for variables and constants; typed IR format 8 supports `neg` and
constant `neg(...)`. Invalid bool/pointer/aggregate negation is rejected.

## Implementation and independent evidence

`float/arithmetic.rs` uses integer significands and binary exponents. Exact
multiplication fits 106 bits of u128. Addition aligns with guard/round/sticky
bits before cancellation; division computes additional quotient bits and jams
any nonzero remainder. A single packing operation rounds, normalizes and
handles subnormal/overflow boundaries. Remainder reduces integer significands
using modular powers of two, with logarithmic work in the exponent gap.
There is no host f64 intermediary.

`tools/generate_float_vectors.py` independently computes exact rational results
with Python Fraction and binary-searches the adjacent representable values to
select the nearest even result. The checked-in 5,760 vectors cover all three
widths, signed zeros, cancellation, ties, subnormal boundaries, overflow,
infinities, signaling/quiet NaNs and seeded random operands. Tests execute the
vectors through both the arithmetic module and the typed interpreter; supported
constant arithmetic uses the same contract. Further native f32/f64 randomized
comparisons check ordinary hosts independently. Regeneration requires Python
only; ordinary Cargo tests use the checked-in data.

## Numeric conversions

IToF_S/U, FToI_S/U and all legal FExt/FTrunc pairs use the same exact
significand representation and a single destination round. Integer magnitudes
remain u128 until packing, including signed i128 minimum and unsigned u128
maximum. Float-to-int truncates magnitude before checking destination limits;
no intermediate i128 or host saturating cast can hide overflow. Numerical NaNs
canonicalize on float-width conversion and trap on integer conversion.

`tools/generate_conversion_vectors.py` adds 10,966 independent Fraction vectors
covering all integer types and all float width pairs, signed endpoints and their
neighboring representable floats, ties, infinities, payload NaNs and zeros.
Tests also exhaust all 65,536 binary16 patterns through both wider formats,
check direct narrowing near a double-rounding counterexample, and assert trap
identity/order after preceding storage operations.
