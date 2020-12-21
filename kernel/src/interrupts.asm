[bits 64]
[default rel]

extern handle_interrupt

section .text

call_handler:
    ; Save the register state.
    push rax
    push rbx
    push rcx
    push qword [r15 + 0x00] ; RDX
    push qword [r15 + 0x08] ; RSI
    push qword [r15 + 0x10] ; RDI
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push qword [r15 + 0x18] ; R15


    ; Save processor state.
    push rax
    push rcx
    push rdx

    xor rcx, rcx
    xgetbv

    mov rcx, gs:[8]
    xsave [rcx]

    pop rdx
    pop rcx
    pop rax


    ; Save the current stack pointer for the 4th argument (register state).
    mov rcx, rsp

    ; Save the current stack pointer to restore it later.
    mov rbp, rsp

    ; Allocate shadow space and align the stack to 16 byte boundary.
    sub rsp, 0x20
    and rsp, ~0xf

    call handle_interrupt

    ; Restore the stack pointer.
    mov rsp, rbp


    ; Restore processor state.
    push rax
    push rcx
    push rdx

    xor rcx, rcx
    xgetbv

    mov rcx, gs:[8]
    xrstor [rcx]

    pop rdx
    pop rcx
    pop rax


    ; Restore the register state.
    pop qword [r15 + 0x18] ; R15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop qword [r15 + 0x10] ; RDI
    pop qword [r15 + 0x08] ; RSI
    pop qword [r15 + 0x00] ; RDX
    pop rcx
    pop rbx
    pop rax

    ret

%macro make_handler 2
    %assign interrupt_number %1
    %assign has_error_code   %2

    global interrupt_ %+ %1:

    interrupt_ %+ %1:
        ; Save registers which we will clobber.
        push r15
        push rdi
        push rsi
        push rdx

        ; Save the current stack frame.
        mov r15, rsp

        ; Get the interrupt ID.
        mov edi, interrupt_number

        %if has_error_code
            ; Get the interrupt frame and error code.
            lea rsi, [rsp+0x28]
            mov rdx, [rsp+0x20]

            ; Align the stack to 16 byte boundary.
            sub rsp, 8
        %else
            ; Get the interrupt frame and zero out error code.
            lea rsi, [rsp+0x20]
            mov rdx, 0

            ; Stack is already 16 byte aligned.
        %endif

        call call_handler

        %if has_error_code
            ; Remove stack alignment from before.
            add rsp, 8
        %endif

        ; Restore clobbered registers.
        pop rdx
        pop rsi
        pop rdi
        pop r15

        %if has_error_code
            ; Pop the error code.
            add rsp, 8
        %endif

        iretq
%endmacro

make_handler 0,   0
make_handler 1,   0
make_handler 2,   0
make_handler 3,   0
make_handler 4,   0
make_handler 5,   0
make_handler 6,   0
make_handler 7,   0
make_handler 8,   1
make_handler 9,   0
make_handler 10,  1
make_handler 11,  1
make_handler 12,  1
make_handler 13,  1
make_handler 14,  1
make_handler 15,  0
make_handler 16,  0
make_handler 17,  1
make_handler 18,  0
make_handler 19,  0
make_handler 20,  0
make_handler 21,  0
make_handler 22,  0
make_handler 23,  0
make_handler 24,  0
make_handler 25,  0
make_handler 26,  0
make_handler 27,  0
make_handler 28,  0
make_handler 29,  0
make_handler 30,  0
make_handler 31,  0
make_handler 32,  0
make_handler 33,  0
make_handler 34,  0
make_handler 35,  0
make_handler 36,  0
make_handler 37,  0
make_handler 38,  0
make_handler 39,  0
make_handler 40,  0
make_handler 41,  0
make_handler 42,  0
make_handler 43,  0
make_handler 44,  0
make_handler 45,  0
make_handler 46,  0
make_handler 47,  0
make_handler 48,  0
make_handler 49,  0
make_handler 50,  0
make_handler 51,  0
make_handler 52,  0
make_handler 53,  0
make_handler 54,  0
make_handler 55,  0
make_handler 56,  0
make_handler 57,  0
make_handler 58,  0
make_handler 59,  0
make_handler 60,  0
make_handler 61,  0
make_handler 62,  0
make_handler 63,  0
make_handler 64,  0
make_handler 65,  0
make_handler 66,  0
make_handler 67,  0
make_handler 68,  0
make_handler 69,  0
make_handler 70,  0
make_handler 71,  0
make_handler 72,  0
make_handler 73,  0
make_handler 74,  0
make_handler 75,  0
make_handler 76,  0
make_handler 77,  0
make_handler 78,  0
make_handler 79,  0
make_handler 80,  0
make_handler 81,  0
make_handler 82,  0
make_handler 83,  0
make_handler 84,  0
make_handler 85,  0
make_handler 86,  0
make_handler 87,  0
make_handler 88,  0
make_handler 89,  0
make_handler 90,  0
make_handler 91,  0
make_handler 92,  0
make_handler 93,  0
make_handler 94,  0
make_handler 95,  0
make_handler 96,  0
make_handler 97,  0
make_handler 98,  0
make_handler 99,  0
make_handler 100, 0
make_handler 101, 0
make_handler 102, 0
make_handler 103, 0
make_handler 104, 0
make_handler 105, 0
make_handler 106, 0
make_handler 107, 0
make_handler 108, 0
make_handler 109, 0
make_handler 110, 0
make_handler 111, 0
make_handler 112, 0
make_handler 113, 0
make_handler 114, 0
make_handler 115, 0
make_handler 116, 0
make_handler 117, 0
make_handler 118, 0
make_handler 119, 0
make_handler 120, 0
make_handler 121, 0
make_handler 122, 0
make_handler 123, 0
make_handler 124, 0
make_handler 125, 0
make_handler 126, 0
make_handler 127, 0
make_handler 128, 0
make_handler 129, 0
make_handler 130, 0
make_handler 131, 0
make_handler 132, 0
make_handler 133, 0
make_handler 134, 0
make_handler 135, 0
make_handler 136, 0
make_handler 137, 0
make_handler 138, 0
make_handler 139, 0
make_handler 140, 0
make_handler 141, 0
make_handler 142, 0
make_handler 143, 0
make_handler 144, 0
make_handler 145, 0
make_handler 146, 0
make_handler 147, 0
make_handler 148, 0
make_handler 149, 0
make_handler 150, 0
make_handler 151, 0
make_handler 152, 0
make_handler 153, 0
make_handler 154, 0
make_handler 155, 0
make_handler 156, 0
make_handler 157, 0
make_handler 158, 0
make_handler 159, 0
make_handler 160, 0
make_handler 161, 0
make_handler 162, 0
make_handler 163, 0
make_handler 164, 0
make_handler 165, 0
make_handler 166, 0
make_handler 167, 0
make_handler 168, 0
make_handler 169, 0
make_handler 170, 0
make_handler 171, 0
make_handler 172, 0
make_handler 173, 0
make_handler 174, 0
make_handler 175, 0
make_handler 176, 0
make_handler 177, 0
make_handler 178, 0
make_handler 179, 0
make_handler 180, 0
make_handler 181, 0
make_handler 182, 0
make_handler 183, 0
make_handler 184, 0
make_handler 185, 0
make_handler 186, 0
make_handler 187, 0
make_handler 188, 0
make_handler 189, 0
make_handler 190, 0
make_handler 191, 0
make_handler 192, 0
make_handler 193, 0
make_handler 194, 0
make_handler 195, 0
make_handler 196, 0
make_handler 197, 0
make_handler 198, 0
make_handler 199, 0
make_handler 200, 0
make_handler 201, 0
make_handler 202, 0
make_handler 203, 0
make_handler 204, 0
make_handler 205, 0
make_handler 206, 0
make_handler 207, 0
make_handler 208, 0
make_handler 209, 0
make_handler 210, 0
make_handler 211, 0
make_handler 212, 0
make_handler 213, 0
make_handler 214, 0
make_handler 215, 0
make_handler 216, 0
make_handler 217, 0
make_handler 218, 0
make_handler 219, 0
make_handler 220, 0
make_handler 221, 0
make_handler 222, 0
make_handler 223, 0
make_handler 224, 0
make_handler 225, 0
make_handler 226, 0
make_handler 227, 0
make_handler 228, 0
make_handler 229, 0
make_handler 230, 0
make_handler 231, 0
make_handler 232, 0
make_handler 233, 0
make_handler 234, 0
make_handler 235, 0
make_handler 236, 0
make_handler 237, 0
make_handler 238, 0
make_handler 239, 0
make_handler 240, 0
make_handler 241, 0
make_handler 242, 0
make_handler 243, 0
make_handler 244, 0
make_handler 245, 0
make_handler 246, 0
make_handler 247, 0
make_handler 248, 0
make_handler 249, 0
make_handler 250, 0
make_handler 251, 0
make_handler 252, 0
make_handler 253, 0
make_handler 254, 0
make_handler 255, 0
