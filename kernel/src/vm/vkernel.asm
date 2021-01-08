[bits 64]
[org 0x0000_0013_3700_0000]
[default rel]

; Information for the VKernel loader.
base_address:      dq 0x0000_0013_3700_0000
stack_top_address: dq 0xffff_ffff_ffff_ff00

entry:
    mov  rax, 0xffff0
    push rax
    pop  rbx
    mov  dword [rbx], 0x1337

    xor rcx, rcx
    xor rax, rax
    xor rdx, rdx
    mov rax, 3
    xsetbv

    hlt
