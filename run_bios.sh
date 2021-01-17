#!/bin/sh
cargo run && qemu-system-x86_64 -serial stdio -smp 4 -cpu host -enable-kvm \
    -net nic,model=e1000 -net tap,ifname=guest_net,script=no,downscript=no \
    -drive file=build/flugzeug_bios,format=raw,index=0,media=disk
