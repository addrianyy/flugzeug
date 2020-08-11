#![no_std]
#![no_main]

use page_table::PhysAddr;
use boot_block::KERNEL_PHYSICAL_REGION_BASE;

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    cpu::halt();
}

#[no_mangle]
extern "C" fn _start(_boot_block: PhysAddr) -> ! {
    unsafe {
        core::ptr::write_volatile((KERNEL_PHYSICAL_REGION_BASE + 0xb8000) as *mut u16, 0x4141);
    }

    cpu::halt();
}
