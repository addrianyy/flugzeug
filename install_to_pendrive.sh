#!/bin/bash

DEVICE=/dev/sda1

mount $DEVICE /mnt/pendrive

# Check for marker empty file to make sure we won't replace something important.
if test -f "/mnt/pendrive/EFI/boot/flugzeug_marker"; then
    cp build/uefi_bootloader/x86_64-unknown-uefi/release/uefi_bootloader.efi /mnt/pendrive/EFI/boot/BOOTX64.efi
else
    echo "No marker found - invalid device."
fi

umount /mnt/pendrive
