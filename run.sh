#!/bin/sh
cargo run && qemu-system-x86_64 -serial stdio -smp 4 -cpu host -enable-kvm \
    -drive file=build/image,index=0,media=disk,format=raw \
    -net none \
    -drive "if=pflash,format=raw,readonly=on,file=/usr/share/OVMF/OVMF_CODE.fd" \
    -drive "if=pflash,format=raw,file=uefi_vars.fd"
