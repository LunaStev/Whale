section .text
global entry
symbol entry, function, 2, hidden
entry:
    nop
.local:
    ret
symbol .local, notype, 1, default
section .data
payload:
    dd 42
symbol payload, object, 4, hidden
extern imported
symbol imported, function, 0, default
