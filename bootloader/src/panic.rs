use core::panic::PanicInfo;

#[panic_handler]
fn panic(panic_info: &PanicInfo) -> ! {
    println!("Bootloader panic.");

    if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
        println!("message: {}", message);
    }

    if let Some(location) = panic_info.location() {
        println!("location: {}:{}", location.file(), location.line());
    }

    cpu::halt();
}
