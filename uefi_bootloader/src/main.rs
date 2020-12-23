#![no_std]
#![no_main]
#![feature(abi_efiapi, panic_info_message, asm)]

extern crate libc_routines;

#[macro_use] mod print;

mod framebuffer_resolutions;
mod ap_entrypoint;
mod acpi_locator;
mod framebuffer;
mod binaries;
mod kernel;
mod serial;
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
            // Initialize printing subsystem early so we can show errors.
            print::initialize(system_table);

            // Verify the CPU before exiting boot services so we can print errors.
            bootlib::verify_cpu();

            // Get addresses of ACPI tables.
            acpi_locator::locate(system_table);

            // Try to initialize framebuffer device.
            framebuffer::initialize(system_table);

            mm::initialize_and_exit_boot_services(image_handle, system_table);

            // Serial should be initialized after exiting boot services. This way we
            // make sure that we won't interfere with UEFI text output functions.
            serial::initialize();
        }

        INITIALIZED.store(true, Ordering::Relaxed);
    } else {
        bootlib::verify_cpu();

        // AP entrypoint should pass zeroes here because EFI is unavailable.
        assert!(image_handle == 0 && system_table == core::ptr::null_mut(),
                "Invalid arguments passed to the bootloader.");
    }

    // No allocations should be done here to ensure that we will have enough low memory
    // to create AP entrypoint.

    unsafe {
        // Zero out the IDT so if there is any exception we will triple fault.
        cpu::zero_idt();

        kernel::enter();
    }
}
