# Original source identities and diagnostics

Canonical typed IR format **7** adds optional source metadata; the reader retains
formats 3–6 for their existing syntax. AST JSON format **4** adds source wrappers
and retains AST 2/3 compatibility for their existing syntax. Semantics remains 1.
Source metadata never opens a source file and is never inferred from display
names, canonical text offsets or SSA IDs. Unannotated inputs keep their existing
IR diagnostics and execution behavior.

The module's `SourceMetadata` holds ordered source files, original nodes and
operation-to-original mappings. Original node IDs are independent u64 identities.
A source span has a source-file ID and one-based Unicode-scalar line/column
coordinates with an exclusive end. An original node may have no source span.
Instruction ordinals identify every original instruction, including stores,
explicit traps and unused `const_decl` declarations. Terminators and global
declarations have explicit target variants. Many required lower-level operations
may retain the same original node; mapping targets themselves are unique.

Example module-level records, placed before or after function definitions:

```text
source_file @s9 "src/main.wave"
original 123 @s9 8 3 8 15
original 124 none
source_map instruction @f0 %b0 2 123
source_map terminator @f0 %b0 124
```

The mappings require the referenced function/block/operation to exist. The
verifier rejects duplicate IDs/targets, missing nodes/files, zero or reversed
coordinates and missing operations. Node and filename-byte budgets apply before
metadata indexing. Canonical print/parse preserves original identities, records
and order. `SourceMetadata::copy_mapping` propagates identity when a required
machine transformation splits an operation; validate after the transformation.
Native instruction selection is still separate work: these APIs do not imply
that a complete native backend has been implemented.

Frontend `Expr::Located` and `Stmt::Located` wrap existing nodes with a
`SourceAnnotation { id, source }`. The source is optional; when present it carries
the file spelling and the four coordinates. AST 4 serializes the expression as:

```json
{"Located":{"annotation":{"id":123,"source":{"file":"src/main.wave","line":8,"column":3,"end_line":8,"end_column":15}},"expression":{"Var":"missing"}}}
```

Statements use `statement` in place of `expression`. The existing AST envelopes,
strict duplicate-key rejection and resource limits still apply; AST 2/3 reject
the new wrappers. AST 4 has its own schema alongside the preserved AST 2/3
schemas. Node IDs must be unique within an AST. Lowering errors retain the
innermost annotated failing expression or statement, with the structured cause
available through `LowerError::root_cause` and the annotation through
`LowerError::annotation`. File/line/column and original ID also appear in CLI
lowering diagnostics. Inputs without annotations retain their original error.

O0 mappings preserve expression locations, statement stores, unused constants
and terminators without eliminating or reordering operations. Constant-expression
wrappers retain every original node, including nested or lazily skipped operands;
only an outer initializer wrapper maps the declaration itself. Nested nodes may
therefore remain valid unmapped originals. Constant errors retain the innermost
annotation of the actual failing typed operation after full static validation,
with arithmetic and reference resolution performed once. Required entry
alloca hoisting adjusts earlier mappings. Source mapping work is bounded by
`AstLimits.max_source_work` (16,777,216 units by default), in addition to existing
AST node/string/depth/output limits. A source-budget failure is a structured
resource error rather than partially returned IR.

Runtime diagnostics preserve the cause and actual execution site, with optional
original source and a bounded caller chain. Missing metadata uses IR identities;
source metadata does not change trap timing, fuel consumption or prior effects.
Call traces distinguish call sites and callees and include omitted-frame counts
when the configured trace budget is exceeded. See public `InterpreterError`,
`InterpreterOptions` and runtime diagnostic types for structured access.

Focused checks:

```sh
cargo test -p ir --all-features --test source_metadata --locked
cargo test -p ir --all-features --test runtime_diagnostics --locked
cargo test -p whale --all-features --test source_diagnostics_cli --locked
```
