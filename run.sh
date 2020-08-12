#!/bin/sh
cargo run && qemu-system-x86_64 build/image -serial stdio -smp 4
