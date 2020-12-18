#!/bin/sh
cargo run && qemu-system-x86_64 build/flugzeug_bios -serial stdio -smp 4 -cpu host -enable-kvm
