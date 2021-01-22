/// ELF image of the kernel.
#[cfg(feature = "with_kernel")]
pub const KERNEL: &[u8] = include_bytes!(env!("FLUGZEUG_KERNEL_PATH"));

/// ELF image of the kernel.
#[cfg(not(feature = "with_kernel"))]
pub const KERNEL: &[u8] = &[];

/// Realmode AP entrypoint.
pub const AP_ENTRYPOINT: &[u8] = include_bytes!(
    concat!(env!("OUT_DIR"), "/ap_entrypoint.bin")
);