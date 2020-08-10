#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

pub const SIGNATURE: u32 = 0x1778cf9d;

// Don't change offsets, they are hardcoded in bootloader assembly file.
#[repr(C)]
pub struct BootDiskDescriptor {
    pub signature:           u32,
    pub bootloader_lba:      u32,
    pub bootloader_sectors:  u32,
    pub bootloader_checksum: u32,
    pub kernel_lba:          u32,
    pub kernel_sectors:      u32,
    pub kernel_checksum:     u32,
}

// Don't change offsets, they are hardcoded in bootloader assembly file.
#[repr(C, packed)]
pub struct BootDiskData {
    pub disk_number:          u8,
    pub sectors_per_track:    u32,
    pub heads_per_cylinder:   u32,
    pub sectors_per_cylinder: u32,
}

pub fn checksum(data: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;

    for &byte in data {
        hash ^= byte as u32;
        hash  = hash.wrapping_mul(16_777_619);
    }

    hash
}
