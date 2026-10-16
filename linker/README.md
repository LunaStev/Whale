# Static AMD64 relocation application

`core::relocation::apply_relocations` validates objects, owner-qualified symbol
resolution, section placements, patch ranges and the cumulative payload budget
before returning independently owned patched sections. Input objects are never
modified. Overlapping patches are rejected rather than depending on input order.

Absolute64 and Absolute32 use unsigned 64/32-bit bounds on S+A. SignedAbsolute32
uses signed 32-bit bounds and serializes as R_X86_64_32S (ELF type 11); it is not
interchangeable with unsigned R_X86_64_32 (type 10). PC32 and PC8 use signed
bounds on S+A-P, where P is the relocation field address. All intermediates are
computed in i128. Addends may be negative.

PLT32 is supported only as a direct statically resolved PC32 call/reference;
there is no dynamic PLT. GOTPCREL is rejected because no GOT is constructed.
Object targets must be the supported little-endian AMD64 ELF64 identity.

The executable writer and disk/archive CLI are separate consumers of this API.

## Executable and archive consumers

`Linker::add_object`, `add_archive`, `set_entry`, and `link_with_limit` provide the
static Linux AMD64 path. The entry defaults to `_start`, must be globally defined,
and must point inside a text payload. `link_checked` retains typed validation
causes and shared diagnostic fields; `link` preserves the legacy String boundary.

The executable layout is separate from `Layout::compute`'s payload-only contract.
It includes ELF/program headers, page-congruent RX/R/RW PT_LOAD mappings, and a
non-executable GNU_STACK. BSS follows initialized RW sections and increases only
memory size. Output limits include headers, alignment gaps and payloads.

Ordinary ar inputs support GNU/BSD long names and odd-member padding. Members
are selected by unresolved strong references or entry demand; weak undefined
symbols do not extract members. Each archive reaches a fixed point when it is
encountered; there is no implicit rescan of earlier archives for later objects.
Unused duplicate definitions are excluded. Thin archives and dynamic libraries
are rejected. Parsing and selection have independent count/name/work budgets.
