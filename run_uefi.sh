#!/bin/sh
cargo run && qemu-system-x86_64 -serial stdio -smp 4 -cpu host -enable-kvm \
    -m 32G \
    -drive file=build/flugzeug_uefi,index=0,media=disk,format=raw \
    -drive "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd" \
    -drive "if=pflash,format=raw,file=uefi_vars.fd"
    