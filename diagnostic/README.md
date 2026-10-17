# Shared diagnostic conversion boundary

`diagnostic::ToDiagnostic` converts a component's existing typed error into
one data-only envelope with stable `component`, `phase`, `code`, human `message`
and ordered typed `contexts`. Match codes and contexts directly; do not classify
English messages. `Display` is one renderer of those same fields. JSON consumers
can serialize the public fields without replacing the original component error.
The crate has no serialization or process-exit dependency.

Object APIs additionally return enum-typed `ObjectError` validation, read and
write failures; the reusable linker `link_checked` preserves a boxed typed input
cause and the input/section context. Legacy String object/linker methods remain
rendering compatibility paths. Assembler, parser, verifier, AST lowering and
interpreter APIs keep their existing component-specific error types.

Source contexts carry one-based line/column coordinates and optional UTF-8 byte
ranges. Assembler primary context precedes any related declaration. IR runtime
conversion retains its precise leaf operation and available original source ID
and source span; the original typed error retains its complete bounded call
trace. Parser APIs without a supplied filename identify their source as
`<input>`; callers can attach actual object/path identity separately.

Use `ToDiagnostic::diagnostic(&error)` when the component has an inherent method
with the same name (the interpreter's existing `diagnostic()` inspects its runtime
metadata). Human or structured CLI renderers belong outside reusable libraries.
