#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

/// Signature used to check if BDD is valid.
pub const SIGNATURE: u32 = 0x1778cf9d;

/// Disk data which is required to find all programs stored on the disk.
/// Don't change the offsets, they are hardcoded in the bootloader assembly file.
#[repr(C)]
pub struct BootDiskDescriptor {
    /// This signature needs to be equal to `SIGNATURE`, otherwise BDD is invalid.
    pub signature: u32,

    /// LBA address of the second stage bootloader.
    pub bootloader_lba: u32,

    /// Size (in sectors) of the second state bootloader.
    pub bootloader_sectors:  u32,

    /// Checksum (calculated by checksum() function) of the second stage bootloader.
    pub bootloader_checksum: u32,

    /// LBA address of the kernel.
    pub kernel_lba: u32,

    /// Size (in sectors) of the kernel.
    pub kernel_sectors: u32,

    /// Checksum (calculated by checksum() function) of the kernel.
    pub kernel_checksum: u32,
}

/// Disk data which is required to read from the disk using BIOS interrupts.
/// Don't change the offsets, they are hardcoded in the bootloader assembly file.
#[repr(C, packed)]
pub struct BootDiskData {
    /// Disk number used by the BIOS.
    pub disk_number: u8,

    /// Disk geometry used to convert LBA to CHS.
    pub sectors_per_track:    u32,
    pub heads_per_cylinder:   u32,
    pub sectors_per_cylinder: u32,
}

/// Calculate FNV-1a 32 bit checksum of the data.
pub fn checksum(data: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;

    for &byte in data {
        hash ^= byte as u32;
        hash  = hash.wrapping_mul(16_777_619);
    }

    hash
}
