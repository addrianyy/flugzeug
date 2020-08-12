[bits 32]

section .text

global enter_kernel

; qword [esp + 0x04] - Entrypoint
; qword [esp + 0x0c] - Stack
; qword [esp + 0x14] - Boot block
; dword [esp + 0x1c] - Kernel CR3
; dword [esp + 0x20] - Trampoline CR3
; qword [esp + 0x24] - Physical region base
enter_kernel:
    ; Load 64 bit GDT.
    lgdt [gdt_64.r]

    ; Enable LME and NXE.
    mov ecx, 0xc0000080
    mov eax, 0x00000900
    mov edx, 0
    wrmsr

    ; Load trampoline CR3 which maps first 1MB of memory at address 0 (identity map)
    ; and at address `Physical region base` (linear map).
    mov eax, [esp + 0x20]
    mov cr3, eax

    ; Enable some SSE stuff and PAE which is required for long mode.
    xor eax, eax
    or  eax, (1 <<  9) ; OSFXSR
    or  eax, (1 << 10) ; OSXMMEXCPT
    or  eax, (1 <<  5) ; PAE
    mov cr4, eax

    ; Enable paging, write protect and some other less important stuff.
    xor eax, eax
    or  eax,  (1 <<  0) ; Protected mode enable
    or  eax,  (1 <<  1) ; Monitor co-processor
    or  eax,  (1 << 16) ; Write protect
    or  eax,  (1 << 31) ; Paging enable
    mov cr0, eax

    ; Switch CPU to long mode.
    jmp 0x08:.entry_64

[bits 64]
.entry_64:
    ; Reload all segments.
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; We are currently executing code in identity map. Because kernel provides only 
    ; linear map, we need to switch to it. It's as simple as adding `Physical region base`
    ; to the target instruction address.
    mov rax, qword [rsp + 0x24]
    add rax, .entry_64_next
    jmp rax

.entry_64_next:
    ; In System-V ABI RDI is the first parameter to the function.
    ; Load boot block to RDI so it will be passed to the kernel entrypoint.
    mov rdi, qword [rsp + 0x14]

    ; Get the entrypoint of the kernel.
    mov rdx, qword [rsp + 0x04]

    ; Get the actual page tables used by the kernel.
    mov eax, dword [rsp + 0x1c]

    ; Switch to the new stack.
    mov rsp, qword [rsp + 0x0c]

    ; Because now both RIP and RSP use linear map instead of identity map,
    ; we can actually switch to the kernel CR3.
    mov cr3, rax

    ; Reserve some shadow stack space.
    sub rsp, 0x28

    ; Jump to the 64 bit kernel!
    jmp rdx

; GDT used to enter long mode.
align 8
gdt_64:
    dq 0x0000000000000000 ; Null segment.
    dq 0x00209a0000000000 ; Code segment.
    dq 0x0000920000000000 ; Data segment.
    .r:
        dw (.r - gdt_64) - 1
        dd gdt_64
