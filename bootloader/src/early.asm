[org  0x7c00]
[bits 16]

jmp $

; Data between byte 446 and 510 will be overwritten by BIOS.
%if ($ - $$) > 446
%error "Early bootloader cannot be larger than 446 bytes.", $
%endif

; Pad first sector to 512 bytes and insert required boot signature.
times 510 - ($ - $$) db 0x00
dw 0xaa55
