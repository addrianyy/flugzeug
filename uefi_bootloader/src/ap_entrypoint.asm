[org  0]
[bits 16]

; We don't know our base address at compile time so every memory access needs to add code base.

; `ap_entrypoint.rs` will expect this instruction sequence.
; It will modify `0xaabbccdd` to be our code base address.
mov eax, 0xaabbccdd
jmp entry_16

; Don't change this without changing `ap_entrypoint.rs`.
; This will be filled in.
trampoline_cr3:        dq 0
bootloader_entrypoint: dq 0

entry_16:
    ; Disable interrupts and clear direction flag.
    cli
    cld

    ; NMIs were already disabled in `enter_kernel`. A20 line is enabled.

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

    ; Get absolute address of 32 bit entrypoint.
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

    ; Offset pointer to GDT by base address.  Upper 32 bits of the pointer are 
    ; guaranteed to be zero.
    mov eax, gdt_64
    add eax, ebx
    mov dword [ebx + gdt_64.pointer], eax

    ; Enable PAE.
    mov eax, cr4
    or  eax, 1 << 5
    mov cr4, eax

    ; Load page table. It will contain first 4GB of memory and bootloader mapped in.
    ; Even though `trampoline_cr3` is qword it is guaranteed to be in lower 4GB of memory.
    mov eax, [ebx + trampoline_cr3]
    mov cr3, eax

    ; Enable long mode.
    mov ecx, 0xC0000080
    rdmsr
    or eax, 1 << 8
    wrmsr

    ; Enable paging.
    mov eax, cr0
    or  eax, 1 << 31
    mov cr0, eax

    ; Load 64 bit GDT.
    lgdt [ebx + gdt_64.register]

    ; Get absolute address of 64 bit entrypoint.
    mov eax, entry_64
    add eax, ebx

    ; Enter 64 bit mode. Normal far jump cannot be used because it uses absolute destination.
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

    ; Clear upper 32 bits of our base address.
    mov ebx, ebx

    ; Set stack pointer to base address - 0x40. (To comply with Microsoft calling convention.)
    mov rsp, rbx
    sub rsp, 0x40

    ; Clear two arguments to the bootloader.
    xor rcx, rcx
    xor rdx, rdx

    ; Call the bootloader entrypoint. It is guaranteed to be mapped in `trampoline_cr3`.
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

    ; Code must change this.
    .pointer:
        dd 0


; GDT used to enter long mode.
align 8
gdt_64:
    dq 0x0000000000000000 ; Null segment.
    dq 0x00209a0000000000 ; Code segment.
    dq 0x0000920000000000 ; Data segment.

    .register:
        dw (.register - gdt_64) - 1

    ; Code must change this.
    .pointer:
        dq 0
