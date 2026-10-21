# Typed frontend storage (AST format 5)

Concrete `Struct {fields: [{name, ty}, ...]}` references are structural layouts.
Field names are frontend selection metadata; typed IR keeps the ordered field
types. `Field {base, layout, name}` carries the resolved layout, which must match
its base's physical struct type. Empty/duplicate names, unknown fields, missing
or duplicate initializers, and mismatched initializer types fail lowering.
Initial structs have scalar fields; initial arrays have scalar elements.
Pointers and function pointers are scalar storage types. Recursive named types,
struct arguments, aggregate returns, and aggregate compile-time declarations are outside
this initial boundary and fail explicitly. Legacy array parameters remain representable
for layout clients; interpreter preflight still rejects their by-value ABI.

`Struct {ty, fields: [{name, value}, ...]}` and `Array {ty, elements}` construct
tracked temporary storage. Array `ty` supplies the fixed element type and length.
Variables copy aggregate storage rather than loading unsupported aggregate SSA
values. Copies preserve initialization state, including uninitialized padding.
Field/index reads load only the selected scalar. Construction follows initializer
source order, even when struct initializer order differs from declaration order.
Allocation can be hoisted into entry; initialization stays at the expression site
and repeats on each evaluation. No host Rust layout is consulted.

`Store {place, value}` evaluates the place first and the value second, once each.
Places include variables, fields, array indices and `Deref(pointer)`.
`AddressOf(place)` retains allocation identity. `PointerOffset {base, index}`
uses the resolved pointee stride, identically for direct and compound bases.
Address computation does not dereference; subsequent loads/stores enforce null,
provenance, bounds, lifetime, alignment, permissions and initialization rules.
Returning a local address cannot extend the allocation's lifetime.

`Index {base, index}` evaluates base before index. The index must be an integer.
Negative or index >= fixed length traps with `array index out of bounds` before
the GEP/access; u128 indices are checked without narrowing. First/last elements
are valid, zero-length arrays have no valid index. A pointer offset can form a
one-past address but dereferencing it fails the tracked-memory bounds check.
Nested index expressions keep the same source evaluation order.

AST depth/node/string/output limits include every new type, field, initializer
and expression. A shared storage-expansion budget additionally bounds repeated
type annotations and environment copies across all lowered functions; it is
charged before cloning types. The budget allows one million annotation visits,
charging sixteen visits per source type node to cover emitted storage operations.
Large amplification fails `AST storage expansion limit exceeded`. Struct lookup
uses a name registry, and GEP verification uses a borrowed constant registry.
