[bits 32]

section .text

global bios_interrupt

%define BASE_ADDRESS 0x10000

bios_interrupt:
    xchg bx, bx

    ; Save all registers.
    pushad

    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    jmp 0x08:(.start_interrupt)

.start_interrupt:
    ; Disable protected mode.
    mov eax, cr0
    and eax, ~1
    mov cr0, eax

    ; Clear all segment registers.
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    pushfd                                      ; eflags
    push dword (BASE_ADDRESS >> 4)              ; cs
    push dword (.realmode_entry - BASE_ADDRESS) ; eip
    iretd

.realmode_entry:

