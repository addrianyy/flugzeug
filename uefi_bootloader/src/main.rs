#![no_std]
#![no_main]
#![feature(abi_efiapi, panic_info_message, asm)]

extern crate libc_routines;

#[macro_use] mod serial;

mod framebuffer_resolutions;
mod ap_entrypoint;
mod acpi_locator;
mod framebuffer;
mod binaries;
mod kernel;
mod panic;
mod efi;
mod mm;

use core::sync::atomic::{AtomicBool, Ordering};

use boot_block::BootBlock;

// Bootloader is not thread safe. There can be only one instance of it running at a time.
// Kernel launches cores one by one to make sure that this is indeed what happens.

/// Boot block is a shared data structure between kernel and bootloader. It must have
/// exactly the same shape in 32 bit and 64 bit mode. It allows for concurrent memory
/// allocation and modification and serial port interface.
static BOOT_BLOCK:  BootBlock  = BootBlock::new();
static INITIALIZED: AtomicBool = AtomicBool::new(false);


#[no_mangle]
extern fn efi_main(image_handle: usize, system_table: *mut efi::EfiSystemTable) -> ! {
    if !INITIALIZED.load(Ordering::Relaxed) {
        // We are executing for the first time and we have EFI services available.

        unsafe {
            serial::initialize();

            // Get addresses of ACPI tables.
            acpi_locator::locate(system_table);

            // Try to initialize framebuffer device.
            framebuffer::initialize(system_table);

            mm::initialize_and_exit_boot_services(image_handle, system_table);
        }

        INITIALIZED.store(true, Ordering::Relaxed);
    } else {
        // AP entrypoint should pass zeroes here because EFI is unavailable.
        assert!(image_handle == 0 && system_table == core::ptr::null_mut(),
                "Invalid arguments passed to the bootloader.");
    }

    bootlib::verify_cpu();

    // No allocations should be done here to ensure that we will have enough low memory
    // to create AP entrypoint.

    unsafe {
        kernel::enter();
    }
}
