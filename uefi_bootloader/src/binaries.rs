/// ELF image of the kernel.
pub const KERNEL: &[u8] = include_bytes!(env!("FLUGZEUG_KERNEL_PATH"));

/// Realmode AP entrypoint.
pub const AP_ENTRYPOINT: &[u8] = include_bytes!(env!("FLUGZEUG_AP_ENTRYPOINT_PATH"));
