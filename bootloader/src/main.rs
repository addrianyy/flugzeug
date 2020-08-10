#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;

mod bios;
mod mm;

use core::fmt::Write;
use serial_port::SerialPort;
use alloc::collections::btree_map::BTreeMap;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        core::ptr::write(0xb8000 as *mut u16, 0x4141);
    }

    loop {}
}

#[no_mangle]
extern "C" fn _start(_boot_disk_descriptor: u32, _boot_disk_data: u32) -> ! {
    let mut serial = unsafe { SerialPort::new() };

    unsafe { mm::initialize() };

    let mut test = BTreeMap::new();
    test.insert(123132, 1234);
    test.insert(43065, 77774);
    test.insert(1662, 12754);
    test.insert(1234352, 66234);

    let _ = write!(serial, "{:?}.\n", test);

    loop {}
}
