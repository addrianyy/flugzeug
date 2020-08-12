[bits 32]

; WARNING: If you change this make sure to change entry in 16-bit mode GDT too.
%define BASE_ADDRESS 0x10000

section .text

struc register_state
    .eax:    resd 1
    .ecx:    resd 1
    .edx:    resd 1
    .ebx:    resd 1
    .esp:    resd 1
    .ebp:    resd 1
    .esi:    resd 1
    .edi:    resd 1
    .eflags: resd 1
    .es:     resw 1
    .ds:     resw 1
    .fs:     resw 1
    .gs:     resw 1
    .ss:     resw 1
endstruc

global bios_interrupt

bios_interrupt:
    ; Save all registers to restore them in the feature.
    pushad

    ; Save protected mode GDT which will be reloaded later.
    sgdt [previous_gdt]

    ; Load 16 bit GDT with CS base == BASE_ADDRESS.
    lgdt [gdt_16.r]

    ; Reload all segments to 16 bit data ones. All base adresses are zero, which is fine, as
    ; stack is somewhere near 0x7c00.
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Enter protected 16-bit mode by jumping to the first stage handler.
    ; We need to adjust IP because it has now implicit base BASE_ADDRESS.
    jmp 0x08:(.stage0 - BASE_ADDRESS)

[bits 16]
.stage0:
    ; Disable protected mode.
    mov eax, cr0
    and eax, ~1
    mov cr0, eax

    ; Clear all segment registers, their base is now 0.
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Enter actual real mode by jumping to stage 1 handler.
    pushfd                              ; eflags
    push dword (BASE_ADDRESS >> 4)      ; cs
    push dword (.stage1 - BASE_ADDRESS) ; eip
    iretd

.stage1:
    ; Get the interrupt number.
    movzx ebx, byte [esp + (4 * 9)]

    ; Get pointer to the register state.
    mov eax, dword [esp + (4 * 10)]

    ; Calculate interrupt offset in BIOS IVT.
    shl ebx, 2

    ; Setup interrupt frame that will be used by BIOS iret instruction.
    mov ebp, (.bios_return_16 - BASE_ADDRESS)
    pushfw    ; flags
    push cs   ; cs
    push bp   ; ip

    ; Setup interrupt frame that we we will use to jump to the BIOS interrupt handler.
    pushfw              ; flags
    push word [bx + 2]  ; cs
    push word [bx + 0]  ; ip

    ; Load requested register state. Note that both flags and registers are not loaded.
    ; They are only used for output.
    mov ecx, dword [eax + register_state.ecx]
    mov edx, dword [eax + register_state.edx]
    mov ebx, dword [eax + register_state.ebx]
    mov ebp, dword [eax + register_state.ebp]
    mov esi, dword [eax + register_state.esi]
    mov edi, dword [eax + register_state.edi]
    mov eax, dword [eax + register_state.eax]

    ; Jump to interrupt BIOS handler.
    iretw

.bios_return_16:
    ; Clear interrupt flag and direction flag just to be sure.
    cli
    cld

    ; BIOS returned from interrupt. Immediately save all registers.
    push eax
    push ecx
    push edx
    push ebx
    push ebp
    push esi
    push edi
    pushfd
    push es
    push ds
    push fs
    push gs
    push ss

    ; Get pointer to the register state. We have pushed additional 8 GPRs and 5 segment registers.
    mov eax, dword [esp + (4 * 10) + (4 * 8) + (5 * 2)]

    ; Update register state to one captured after BIOS interrupt return.
    pop  word [eax + register_state.ss]
    pop  word [eax + register_state.gs]
    pop  word [eax + register_state.fs]
    pop  word [eax + register_state.ds]
    pop  word [eax + register_state.es]
    pop dword [eax + register_state.eflags]
    pop dword [eax + register_state.edi]
    pop dword [eax + register_state.esi]
    pop dword [eax + register_state.ebp]
    pop dword [eax + register_state.ebx]
    pop dword [eax + register_state.edx]
    pop dword [eax + register_state.ecx]
    pop dword [eax + register_state.eax]

    ; Calculate segment & offset for previous GDT.
    mov eax, BASE_ADDRESS >> 4
    mov ds,  ax
    mov esi, (previous_gdt - BASE_ADDRESS)

    ; Reload protected mode GDT.
    lgdt [ds:si]

    ; Reenable protected mode.
    mov eax, cr0
    or  eax, 1 << 0
    mov cr0, eax

    ; Return to protected mode (bios_return_32). We need to use iret here because jump
    ; offset cannot be properly encoded in jmpf here.
    pushfd                      ; eflags
    push dword 0x08             ; cs
    push dword .bios_return_32  ; eip
    iretd

[bits 32]
.bios_return_32:
    ; Reload all segments to 32 bit ones.
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Restore all registers.
    popad

    ret

align 8
gdt_16:
    dq 0x0000000000000000 ; Null segment.
    dq 0x00009a010000ffff ; Code segment. (Base 0x10000)
    dq 0x000092000000ffff ; Data segment.
    .r:
        dw (.r - gdt_16) - 1
        dd gdt_16

; This will be filled at the begnnining of bios_interrupt. This memory is writable because whole
; bootloader is mapped with RWX permissions.
previous_gdt:
    dw 0 ; Limit.
    dd 0 ; Base.
