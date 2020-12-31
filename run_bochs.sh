#!/bin/sh
cargo run && bochs -q -f bochsrc.linux
