[bits 64]
[default rel]

section .text

global enter_kernel

; Here we can only refer to things via memory operands which can be RIP-relative. Otherwise
; compiler generates wrong code.

%macro copy_argument 1
    mov rax, [r10+%1]
    mov [rsp+%1], rax
%endmacro

; qword [rsp + 0x08] - Entrypoint
; qword [rsp + 0x10] - Stack
; qword [rsp + 0x18] - Boot block
; dword [rsp + 0x20] - Kernel CR3
; dword [rsp + 0x28] - Trampoline CR3
; qword [rsp + 0x30] - Physical region base
; qword [rsp + 0x38] - Uninitialized GDT
; qword [rsp + 0x40] - Trampoline RSP
; qword [rsp + 0x48] - Boot TSC
enter_kernel:
    ; Move all register arguments to shadow space on the stack.
    mov [rsp + 0x8],  rcx
    mov [rsp + 0x10], rdx
    mov [rsp + 0x18], r8
    mov [rsp + 0x20], r9

    ; Save current stack in R10 and switch to trampoline stack.
    mov r10, rsp
    mov rsp, [rsp + 0x40]

    ; Allocate space on trampoline stack for arguments.
    sub rsp, 0x100

    ; Copy all function arguments to trampoline stack.
    copy_argument 0x08
    copy_argument 0x10
    copy_argument 0x18
    copy_argument 0x20
    copy_argument 0x28
    copy_argument 0x30
    copy_argument 0x38
    copy_argument 0x40
    copy_argument 0x48

    ; Get uninitialized GDT base.
    mov r10, [rsp + 0x38]

    ; Setup null segment as selector 0.
    mov rbx, GDT_NULL
    mov qword [r10 + 0], rbx

    ; Setup code segment at selector 8.
    mov rbx, GDT_CODE
    mov qword [r10 + 8], rbx

    ; Get selector for current CS.
    xor rax, rax
    mov ax, cs
    and rax, ~0b111

    ; Setup code segment at selector CS.
    mov rbx, GDT_CODE
    mov qword [r10 + rax], rbx

    ; Zero out all data segments before loading new GDT.
    xor ax, ax
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Create GDTR on stack and load it.
    sub  rsp, 0x10
    mov  word  [rsp + 0], 0xfff
    mov  qword [rsp + 2], r10
    lgdt [rsp]
    add  rsp, 0x10

    mov rax, rsp
    lea rbx, [.continue]

    ; Switch to CS = 8.
    push 0   ; SS (0)
    push rax ; RSP
    pushfq   ; RFLAGS
    push 8   ; CS
    push rbx ; RIP
    iretq

.continue:
    ; Old CS selector is not unused and our code segment has selector 0x08.

    ; Setup data segment at selector 16.
    mov rbx, GDT_DATA
    mov qword [r10 + 16], rbx

    ; Create GDTR on stack and load it again but with proper bounds.
    sub  rsp, 0x10
    mov  word  [rsp + 0], (3 * 8) - 1 ; Only 3 elements (null, code, data).
    mov  qword [rsp + 2], r10
    lgdt [rsp]
    add  rsp, 0x10

    ; Reload data segments.
    mov ax, 0x10
    mov ds, ax
    mov es, ax
    mov ss, ax
    mov fs, ax
    mov gs, ax

    ; Enable LME, LMA and NXE.
    mov rcx, 0xc0000080
    mov rax, (1 << 10) | (1 << 8) | (1 << 11)
    mov rdx, 0
    wrmsr

    ; Load trampoline CR3 which maps first part of memory at address 0 (identity map)
    ; and at address `Physical region base` (linear map).
    mov rax, [rsp + 0x28]
    mov cr3, rax

    ; Enable some SSE stuff and PAE which is required for long mode.
    xor rax, rax
    or  rax, (1 <<  9) ; OSFXSR
    or  rax, (1 << 10) ; OSXMMEXCPT
    or  rax, (1 <<  5) ; PAE
    or  rax, (1 << 18) ; OSXSAVE
    mov cr4, rax

    ; Enable paging, write protect and some other less important stuff.
    xor rax, rax
    or  eax,  (1 <<  0) ; Protected mode enable
    or  eax,  (1 <<  1) ; Monitor co-processor
    or  eax,  (1 << 16) ; Write protect
    or  eax,  (1 << 31) ; Paging enable
    mov cr0, rax

    ; Enable x87, SSE and AVX in XCR0.
    xor rax, rax
    xor rdx, rdx
    or  rax, (1 << 0) ; x87
    or  rax, (1 << 1) ; SSE
    or  rax, (1 << 2) ; AVX
    xor rcx, rcx
    xsetbv

    ; Clear IA32_XSS because we only use XCR0.
    mov ecx, 0xda0
    xor eax, eax
    xor edx, edx
    wrmsr

    ; We are currently executing code in identity map. Because kernel provides only
    ; linear map, we need to switch to it. It's as simple as adding `Physical region base`
    ; to the target instruction address.
    mov rax, qword [rsp + 0x30]
    lea rbx, [.entry_64_next]
    add rax, rbx
    jmp rax

.entry_64_next:
    ; Load kernel arguments.
    mov rdi, qword [rsp + 0x18]
    mov rsi, qword [rsp + 0x48]

    ; Get the entrypoint of the kernel.
    mov rdx, qword [rsp + 0x08]

    ; Get the actual page tables used by the kernel.
    mov rax, qword [rsp + 0x20]

    ; Switch to the new stack.
    mov rsp, qword [rsp + 0x10]

    ; Because now both RIP and RSP use linear map instead of identity map,
    ; we can actually switch to the kernel CR3.
    mov cr3, rax

    ; Reserve some shadow stack space. Keep the stack 16 byte aligned.
    sub rsp, 0x30

    ; Call the 64 bit kernel! (Jump cannot be used because of the ABI.)
    call rdx

GDT_NULL equ 0x0000000000000000
GDT_CODE equ 0x00209a0000000000
GDT_DATA equ 0x0000920000000000
