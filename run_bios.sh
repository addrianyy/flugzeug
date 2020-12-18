#!/bin/sh
cargo run && qemu-system-x86_64 -serial stdio -smp 4 -cpu host -enable-kvm \
    -drive file=build/flugzeug_bios,format=raw,index=0,media=disk
