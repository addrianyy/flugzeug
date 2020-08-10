#![no_std]
#![feature(asm)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

pub unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value);
}

pub unsafe fn outw(port: u16, value: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") value);
}

pub unsafe fn outd(port: u16, value: u32) {
    asm!("out dx, eax", in("dx") port, in("eax") value);
}

pub unsafe fn inb(port: u16) -> u8 {
    let value;
    asm!("in al, dx", in("dx") port, out("al") value);
    value
}

pub unsafe fn inw(port: u16) -> u16 {
    let value;
    asm!("in ax, dx", in("dx") port, out("ax") value);
    value
}

pub unsafe fn ind(port: u16) -> u32 {
    let value;
    asm!("in eax, dx", in("dx") port, out("eax") value);
    value
}
