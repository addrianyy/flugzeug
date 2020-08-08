#![no_std]
#![no_main]

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[no_mangle]
extern "C" fn _start() -> ! {
    unsafe { core::ptr::write(0x13370 as *mut u64, 0xcc); }

    loop {}
}
