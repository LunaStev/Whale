# Retained ELF reader fixture

`nasm-rel.hex` contains the exact ELF64 little-endian object emitted by NASM
2.16.03 from `nasm-rel.asm`. SHA-256 of decoded bytes:
`62dff8de4b433a42c016cfa36e4d05a75cc9045c527e40cfd8e1634cdb1ce6d4`.

Regenerate from this directory with:

```sh
nasm -f elf64 nasm-rel.asm -o /tmp/whale-nasm-rel.o
readelf -SWsr /tmp/whale-nasm-rel.o
python3 -c 'from pathlib import Path; b=Path("/tmp/whale-nasm-rel.o").read_bytes(); Path("nasm-rel.hex").write_text("\n".join(b[i:i+32].hex() for i in range(0,len(b),32))+"\n")'
```

The ordinary Cargo tests decode the retained hex and need no NASM/readelf.
The fixture covers FILE/ABS auxiliary metadata, section-symbol relocations,
external calls, read-only bytes, sparse BSS and a collision with a generated
private section-symbol name. The reader preserves ordinary symbol names and
normalizes section symbols to deterministic object-local names; FILE metadata
is omitted from link definitions. Ordinary ABS/COMMON definitions, TLS, dynamic
symbols/relocations, non-UTF-8 names, extended numbering and non-allocating
payload sections are explicitly outside the initial imported object subset.
