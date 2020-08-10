#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use lock::Lock;
use rangeset::RangeSet;
use serial_port::SerialPort;

#[repr(C)]
pub struct BootBlock {
    pub size: u64,

    pub free_memory: Lock<Option<RangeSet>>,
    pub serial_port: Lock<Option<SerialPort>>,
}

impl BootBlock {
    pub const fn new() -> Self {
        Self {
            size:        core::mem::size_of::<Self>() as u64,
            free_memory: Lock::new(None),
            serial_port: Lock::new(None),
        }
    }
}
