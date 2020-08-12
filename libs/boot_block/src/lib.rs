#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use lock::Lock;
use rangeset::RangeSet;
use page_table::PageTable;
use serial_port::SerialPort;

pub const KERNEL_STACK_BASE:    u64 = 0x0000_7473_0000_0000;
pub const KERNEL_STACK_SIZE:    u64 = 4  * 1024 * 1024;
pub const KERNEL_STACK_PADDING: u64 = 64 * 1024 * 1024;

pub const KERNEL_PHYSICAL_REGION_BASE: u64 = 0xffff_cafe_0000_0000;
pub const KERNEL_PHYSICAL_REGION_SIZE: u64 = 1024 * 1024 * 1024 * 1024;

#[repr(C)]
pub struct BootBlock {
    pub size: u64,

    pub free_memory: Lock<Option<RangeSet>>,
    pub serial_port: Lock<Option<SerialPort>>,
    pub page_table:  Lock<Option<PageTable>>,
}

impl BootBlock {
    pub const fn new() -> Self {
        Self {
            size:        core::mem::size_of::<Self>() as u64,
            free_memory: Lock::new(None),
            serial_port: Lock::new(None),
            page_table:  Lock::new(None),
        }
    }
}
