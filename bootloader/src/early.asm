[org  0x7c00]
[bits 16]

; VGA memory address.
%define VGA_ADDR 0xb8000

; VGA memory size (in words).
%define VGA_SIZE_WORDS 25 * 80

; Ensure that CS == 0 and IP == 0x7C00.
jmp 0x00:entry_16

; 16 bit entry point for bootloader.
[bits 16]
entry_16:
    ; Disable interrupts and clear direction flag.
    cli
    cld

    ; Zero out used segments.
    xor     ax, ax
    mov     ds, ax
    mov     es, ax
    mov     ss, ax

    ; Setup initial stack pointer just above our bootloader.
    mov     sp, 0x7c00

    ; After setting up stack interrupts can be enabled again.
    sti

    ; Save boot disk number.
    mov     byte [disk_number], dl

    ; Reset boot disk system.
    mov     word [error_string_addr], reset_error_str
    xor     ax, ax
    mov     dl, byte [disk_number]
    int     0x13
    jc      error_16
    call    fix_segs

    ; Get boot disk parameters.
    mov     word [error_string_addr], params_error_str
    mov     ah, 0x08
    mov     dl, byte [disk_number]
    int     0x13
    jc      error_16
    call    fix_segs

    ; Copy disk parameters for LBA calculations. Returned head count is
    ; 0 based and therafore needs to be incremented.
    and     cl, 0x3f
    movzx   eax, cl
    movzx   ecx, dh
    inc     ecx

    mov     dword [sectors_per_track], eax
    mov     dword [heads_per_cylinder], ecx

    ; sectors_per_cylinder = heads_per_cylinder * sectors_per_track.
    xor     edx, edx
    mov     eax, dword [heads_per_cylinder]
    mul     dword [sectors_per_track]
    mov     dword [sectors_per_cylinder], eax

    ; Read 1 additional sector so bootloader ends at 0x8000. Skip LBA 1 because it contains
    ; boot disk descriptor.
    mov     ebx, 0x7c00 + 1 * 0x200
    mov     eax, 2
    call    read_sector

    ; We are out of space so jump to newly loaded part of bootloader.
    jmp     entry_16_continue

; Read disk sector into buffer. Works only with 16 byte aligned buffers
; in low (<1MB) memory.
; Input: EAX = LBA
;        EBX = buffer address
; Doesn't return on error.
; Doesn't clobber anything.
read_sector:
    ; Push all registers to restore later.
    pushad

    push    ebx
    push    eax

    ; Try count. BIOS reads should be retried at least 5 times.
    xor     di, di

    ; Try reading 5 times.
    .try:
        call    fix_segs

        ; If we are reading for the first time don't reset disk system.
        test    di, di
        jz      .dont_reset

        ; Reset disk system on previous failure.
        xor     ax, ax
        mov     dl, byte [disk_number]
        int     0x13
        call    fix_segs

        .dont_reset:

        ; If reading failed 5 times, abort.
        mov     word [error_string_addr], read_error_str
        cmp     di, 5
        jae     error_16

        ; Increase try count.
        inc     di

        ; Get buffer address and convert it to segment & offset.
        mov     ebx, dword [esp + 4]

        ; Buffer is 16 byte aligned.
        shr     ebx, 4
        mov     es,  bx
        xor     ebx, ebx

        ; Convert LBA to CHS. BIOS routines use CHS. Output values are already
        ; in registers used by INT 0x13, AH=0x02.
        mov     eax, dword [esp]
        call    lba_to_chs

        ; Read 1 sector into buffer. Data is already filled by previous
        ; call to lba_to_chs.
        mov     ah, 0x02
        mov     al, 0x01
        mov     dl, byte [disk_number]
        int     0x13

        ; Retry if there was an error or we haven't read 1 sector.
        jc      .try
        cmp     al, 1
        jne     .try

        call    fix_segs

    ; Read succedded.

    pop     eax
    pop     ebx

    ; Restore all registers.
    popad

    call    fix_segs
    ret

; Convert LBA to CHS.
; Input:  EAX = LBA
; Output: CH = cylinder
;         CL = sector (and part of cylinder)
;         DH = head
; Clobbers EAX.
lba_to_chs:
    push    ebx
    mov     ebx, eax

    ; cylinder = LBA / sectors_per_cylinder
    mov     eax, ebx
    xor     edx, edx
    div     dword [sectors_per_cylinder]

    ; Move low 8 bits of cylinder to CH and remaining part to CL.
    ; CL = (cylinder >> 2) & 0xc0
    mov     ch, al
    shr     eax, 2
    and     eax, 0xc0
    mov     cl, al

    ; temp = LBA / sectors_per_track
    mov     eax, ebx
    xor     edx, edx
    div     dword [sectors_per_track]

    ; head = temp % heads_per_cylinder
    xor     edx, edx
    div     dword [heads_per_cylinder]
    push    edx

    ; sector = (LBA % sectors_per_track) + 1
    mov     eax, ebx
    xor     edx, edx
    div     dword [sectors_per_track]
    inc     edx

    ; CL already has cylinder part, so OR sector in.
    or      cl, dl

    ; Pop head to EDX and restore EBX.
    pop     edx
    pop     ebx
    mov     dh, dl

    ret

; Zero out data segments after INT calls. BIOS messes them up and bootloader
; expects all segments to be 0.
; Doesn't clobber any register.
fix_segs:
    push    ax

    ; Zero out all data segments.
    xor     ax, ax
    mov     es, ax
    mov     ds, ax
    mov     ss, ax

    pop     ax
    ret

; Inform user about unrecoverable error.
; Clears screen and prints string set in error_string_addr.
; Never returns.
error_16:
    ; Disable interrupts on unrecoverable errors.
    cli

    ; Convert address to segment value.
    mov     ax, (VGA_ADDR >> 4)
    mov     es, ax
    xor     di, di

    ; 0x0e00 = yellow background.
    mov     ax, 0x0e00

    ; Fill the screen.
    mov     cx, VGA_SIZE_WORDS
    rep     stosw

    ; Start printing at position (0, 0).
    mov     ax, (VGA_ADDR >> 4)
    mov     es, ax
    xor     di, di

    mov     bx, word [error_string_addr]

    ; Print null terminated string.
    .print_char:
        mov     al, byte [bx]
        test    al, al
        jz      .string_end

        ; Write character to screen.
        mov     byte [es:di], al
        add     di, 2
        inc     bx
        jmp     .print_char

    .string_end:

    ; Halt forever.
    .halt_loop:
        cli
        hlt
        jmp     .halt_loop

; Information about boot disk. Gets filled by entry point and is used
; by lba_to_chs and read_sector.
disk_bios_data:
disk_number:            db 0
sectors_per_track:      dd 0
heads_per_cylinder:     dd 0
sectors_per_cylinder:   dd 0

; Error message. Set it before calling error_16.
error_string_addr: dw 0

reset_error_str:  db "RESET FAIL.",  0
params_error_str: db "PARAMS FAIL.", 0
read_error_str:   db "READ FAIL.",   0

; Data between byte 446 and 510 will be overwritten by BIOS.
%if ($ - $$) > 446
%error "Code over 446 bytes", $
%endif

; Pad first sector to 512 bytes and insert required boot signature.
times 510 - ($ - $$) db 0x00
dw 0xaa55

%define BDD_SIGNATURE   0x1778cf9d
%define BDD_SIZE        512
%define BOOT_DISK_DESC  0x8000
%define BOOTLOADER_BASE 0x10000

entry_16_continue:
    ; Read boot disk descriptor.
    mov     ebx, BOOT_DISK_DESC
    mov     eax, 1
    call    read_sector

    mov     word [error_string_addr], signature_error_str
    cmp     dword [BOOT_DISK_DESC], BDD_SIGNATURE
    jne     error_16

    ; Get bootloader LBA offset.
    mov     eax, [BOOT_DISK_DESC + 4]

    ; Get bootloader size in sectors.
    mov     edx, [BOOT_DISK_DESC + 8]

    ; Current address to map bootloader.
    mov     ebx, BOOTLOADER_BASE

    ; Current sector index.
    mov     ecx, 0

    .load_sector:
        call    read_sector

        ; Go to next sector.
        inc     eax
        add     ebx, 512
        inc     ecx

        ; Check if we are done.
        cmp     ecx, edx
        jb      .load_sector

    ; Get bootloader base.
    mov     esi, BOOTLOADER_BASE

    ; Get bootloader size in bytes.
    mov     ecx, [BOOT_DISK_DESC + 8]
    shl     ecx, 9

    ; FNV_offset_basis
    mov     ebx, 0x811c9dc5

    ; FNV_prime
    mov     ebp, 16777619

    ; Hash every byte using FNV-1a.
    .hash_byte:
        ; hash = hash ^ byte.
        movzx   eax, byte [esi]
        xor     ebx, eax

        ; hash = hash * FNV_prime (16777619)
        mov     eax, ebx
        xor     edx, edx
        mul     ebp
        mov     ebx, eax

        inc     esi
        dec     ecx
        jnz     .hash_byte

    ; Compare hash with checksum.
    mov     word [error_string_addr], checksum_error_str
    cmp     ebx, [BOOT_DISK_DESC + 12]
    jne     error_16

    ; Enable A20 line.
    in      al, 0x92
    test    al, 0x02
    jnz     after_enable
    or      al, 0x02
    and     al, 0xfe
    out     0x92, al
    after_enable:

    ; Disable NMIs.
    in      al, 0x70
    or      al, 0x80
    out     0x70, al
    
    ; Disable interrupts before entering 32bit mode.
    cli

    ; Load 32 bit GDT.
    lgdt    [gdt_32.r]

    ; Enable protected mode.
    mov     eax, cr0
    or      eax, 1 << 0
    mov     cr0, eax

    ; Other required steps like disabling interrupts and NMIs and enabling
    ; A20 gates are done on beginning, before switch to unreal mode.
    ; Switch CPU to protected mode.
    jmp     0x08:entry_32

[bits 32]
; 32 bit entry point for bootloader.
entry_32:
    ; Reload segments.
    mov     ax, 0x10
    mov     ds, ax
    mov     es, ax
    mov     ss, ax
    mov     fs, ax

    push    BOOT_DISK_DESC
    push    BOOT_DISK_DESC + BDD_SIZE
    push    disk_bios_data

    ; Get bootloader entrypoint and jump there.
    mov     eax, dword [BOOTLOADER_BASE + 0x18]
    jmp     eax

signature_error_str: db "SIG FAIL.",  0
checksum_error_str:  db "CHKSUM FAIL.",  0

; GDT used to enter protected mode.
align 8
gdt_32:
    dq 0x0000000000000000 ; Null segment.
    dq 0x00cf9a000000ffff ; Code segment.
    dq 0x00cf92000000ffff ; Data segment.
    .r:
        dw (.r - gdt_32) - 1
        dd gdt_32

%if ($ - $$) > 1024
%error "Code over 1024 bytes", $
%endif

; Make sure that this bootloader stage has 1024 bytes.
times 1024 - ($ - $$) db 0xff
