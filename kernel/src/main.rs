#![no_std]
#![no_main]

extern crate libc_routines;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
extern "C" fn _start() -> ! {
    loop {}
}
