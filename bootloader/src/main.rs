#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

#[macro_use] mod serial;
mod panic;
mod bios;
mod mm;

use boot_block::BootBlock;

pub static BOOT_BLOCK: BootBlock = BootBlock::new();

#[no_mangle]
extern "C" fn _start(_boot_disk_descriptor: u32, _boot_disk_data: u32) -> ! {
    unsafe {
        serial::initialize();
        mm::initialize();
    }

    println!("Bootloader loaded!");

    cpu::halt();
}
