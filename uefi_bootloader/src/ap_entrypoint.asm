[org  0]
[bits 16]

mov eax, 0xaabbccdd
jmp entry_16

trampoline_cr3:        dq 0
bootloader_entrypoint: dq 0

entry_16:
    ; Disable interrupts and clear direction flag.
    cli
    cld

    ; Save base address to EBX. We need to make sure that nothing in this whole code
    ; will clobber it.
    mov ebx, eax

    ; Convert our base address to value which we can use as a segment.
    shr eax, 4
    mov ds, ax
    mov es, ax

    ; Set stack pointer to base address - 0x10.
    sub ax, 0x0100
    mov ss, ax
    mov sp, 0x0ff0

    ; Enable A20 line.
    in   al, 0x92
    test al, 0x02
    jnz  after_enable
    or   al, 0x02
    and  al, 0xfe
    out  0x92, al
    after_enable:

    ; Disable NMIs.
    in  al, 0x70
    or  al, 0x80
    out 0x70, al

    ; Offset pointer to GDT by base address.
    mov eax, gdt_32
    add eax, ebx
    mov dword [ds:gdt_32.pointer], eax

    ; Load 32 bit GDT.
    lgdt [ds:gdt_32.register]

    ; Enable protected mode.
    mov eax, cr0
    or  eax, 1 << 0
    mov cr0, eax

    mov eax, entry_32
    add eax, ebx

    ; Enter 32 bit mode. Normal far jump cannot be used because it uses absolute destination.
    pushfd            ; EFLAGS
    push dword 0x08   ; CS
    push dword eax    ; EIP
    iretd

[bits 32]
entry_32:
    ; Reload segments.
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Set stack pointer to base address - 0x10.
    mov esp, ebx
    sub esp, 0x10

    ; Offset pointer to GDT by base address.
    ; Upper 32 bits are guaranteed to be zero.
    mov eax, gdt_64
    add eax, ebx
    mov dword [ebx + gdt_64.pointer], eax

    ; Load 64 bit GDT.
    lgdt [ebx + gdt_64.register]

    ; Enable long mode.
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    ; Load page tables.
    mov eax, [ebx + trampoline_cr3]
    mov cr3, eax

    ; Enable PAE.
    mov eax, cr4
    or  eax, 1 << 5
    mov cr4, eax

    ; Enable paging.
    mov eax, cr0
    or  eax, 1 << 31
    mov cr0, eax

    mov eax, entry_64
    add eax, ebx

    ; Enter 32 bit mode. Normal far jump cannot be used because it uses absolute destination.
    pushfd            ; EFLAGS
    push dword 0x08   ; CS
    push dword eax    ; EIP
    iretd

[bits 64]
entry_64:
    ; Reload segments.
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Clear upper 32 bits of base address.
    mov ebx, ebx

    ; Set stack pointer to base address - 0x40. (To comply with Microsoft calling convention.)
    mov rsp, rbx
    sub rsp, 0x40

    ; Clear two arguments to the bootloader.
    xor rcx, rcx
    xor rdx, rdx

    ; Call the bootloader entrypoint.
    call [rbx + bootloader_entrypoint]

    ; We should never return here.
    .next:
        cli
        hlt
        jmp .next

; GDT used to enter protected mode.
align 8
gdt_32:
    dq 0x0000000000000000 ; Null segment.
    dq 0x00cf9a000000ffff ; Code segment.
    dq 0x00cf92000000ffff ; Data segment.

    .register:
        dw (.register - gdt_32) - 1

    .pointer:
        dd gdt_32


; GDT used to enter long mode.
align 8
gdt_64:
    dq 0x0000000000000000 ; Null segment.
    dq 0x00209a0000000000 ; Code segment.
    dq 0x0000920000000000 ; Data segment.

    .register:
        dw (.register - gdt_64) - 1

    .pointer:
        dq gdt_64
