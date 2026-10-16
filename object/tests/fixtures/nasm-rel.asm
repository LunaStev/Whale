bits 64
global entry:function
global __whale_elf_section_2
extern external
section .text
entry:
__whale_elf_section_2:
    call external
    lea rax, [rel message]
    ret
section .rodata
message: db 0x41, 0, 0xff
section .data
pointer: dq message
section .bss
space: resb 32
