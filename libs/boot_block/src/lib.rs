#![no_std]
#![feature(const_fn)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use lock::{Lock, Interrupts};
use rangeset::RangeSet;
use page_table::PageTable;
use serial_port::SerialPort;

/// A region which is used to allocate unique stacks for each core.
pub const KERNEL_STACK_BASE:    u64 = 0x0000_7473_0000_0000;
pub const KERNEL_STACK_SIZE:    u64 = 4  * 1024 * 1024;
pub const KERNEL_STACK_PADDING: u64 = 64 * 1024 * 1024;

/// A region which allows kernel (which uses paging) to access raw physical memory.
pub const KERNEL_PHYSICAL_REGION_BASE: u64 = 0xffff_cafe_0000_0000;
pub const KERNEL_PHYSICAL_REGION_SIZE: u64 = 1024 * 1024 * 1024 * 1024;

/// A region which is used by dynamic allocations in the kernel.
pub const KERNEL_HEAP_BASE:    u64 = 0xffff_8000_0000_0000;
pub const KERNEL_HEAP_PADDING: u64 = 4096;

pub const MAX_SUPPORTED_MODES: usize = 128;

#[repr(C)]
#[derive(Copy, Clone)]
pub struct AcpiTables {
    pub rsdt: Option<u64>,
    pub xsdt: Option<u64>,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct PixelFormat {
    pub red:   u32,
    pub green: u32,
    pub blue:  u32,
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct FramebufferInfo {
    pub width:               u32,
    pub height:              u32,
    pub pixel_format:        PixelFormat,
    pub pixels_per_scanline: u32,
    pub fb_base:             u64,
    pub fb_size:             u64,
}

#[repr(C)]
#[derive(Clone)]
pub struct SupportedModes {
    pub modes:    [(u32, u32); MAX_SUPPORTED_MODES],
    pub count:    u32,
    pub overflow: bool,
}

/// Data shared between the bootloader and the kernel. Allows for concurrent access.
#[repr(C)]
pub struct BootBlock<I: Interrupts> {
    /// Size of the `BootBlock` used to make sure that the shape of the structure is the same
    /// in 32 bit mode and 64 bit mode.
    pub size: u64,

    /// Free physical memory ranges available on the system.
    pub free_memory: Lock<Option<RangeSet>, I>,

    /// Free physical memory ranges available on the system.
    pub boot_memory: Lock<Option<RangeSet>, I>,

    /// Serial port connection which allows for `print!` macros.
    pub serial_port: Lock<Option<SerialPort>, I>,

    /// Page tables created by the bootloader and used by the kernel.
    pub page_table: Lock<Option<PageTable>, I>,

    pub physical_map_page_size: Lock<Option<u64>, I>,
    pub ap_entrypoint:          Lock<Option<u64>, I>,
    pub acpi_tables:            Lock<AcpiTables, I>,
    pub framebuffer:            Lock<Option<FramebufferInfo>, I>,
    pub supported_modes:        Lock<Option<SupportedModes>, I>,
}

impl<I: Interrupts> BootBlock<I> {
    /// Create an empty `BootBlock` and cache the size of it in current processor mode.
    pub const fn new() -> Self {
        Self {
            size:                   core::mem::size_of::<Self>() as u64,
            free_memory:            Lock::new(None),
            boot_memory:            Lock::new(None),
            serial_port:            Lock::new(None),
            page_table:             Lock::new(None),
            physical_map_page_size: Lock::new(None),
            ap_entrypoint:          Lock::new(None),
            acpi_tables:            Lock::new(AcpiTables {
                rsdt: None,
                xsdt: None,
            }),
            framebuffer:     Lock::new(None),
            supported_modes: Lock::new(None),
        }
    }
}
