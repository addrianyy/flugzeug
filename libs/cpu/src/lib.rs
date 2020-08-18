#![no_std]
#![feature(asm)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

#[derive(Default, Copy, Clone)]
pub struct Cpuid {
    eax: u32,
    ebx: u32,
    ecx: u32,
    edx: u32,
}

#[derive(Default, Copy, Clone)]
pub struct CpuFeatures {
    pub page2m: bool,
    pub page1g: bool,
}

pub fn cpuid(eax: u32, ecx: u32) -> Cpuid {
    let mut cpuid = Cpuid::default();

    unsafe {
        asm!("cpuid", in("eax") eax, in("ecx") ecx,
            lateout("eax") cpuid.eax, lateout("ebx") cpuid.ebx,
            lateout("ecx") cpuid.ecx, lateout("edx") cpuid.edx);
    }

    cpuid
}

pub fn get_features() -> CpuFeatures {
    let mut features = CpuFeatures::default();

    let max_cpuid          = cpuid(0, 0).eax;
    let max_extended_cpuid = cpuid(0x80000000, 0).eax;

    if max_cpuid >= 1 {
        let cpuid = cpuid(1, 0);

        features.page2m = (cpuid.edx >> 3) & 1 != 0;
    }

    if max_extended_cpuid >= 0x80000001 {
        let cpuid = cpuid(0x80000001, 0);

        features.page1g = (cpuid.edx >> 26) & 1 != 0;
    }

    features
}

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

pub unsafe fn invlpg(addr: usize) {
    asm!("invlpg [{}]", in(reg) addr);
}

pub unsafe fn rdmsr(msr: u32) -> u64 {
    let low:  u32;
    let high: u32;

    asm!("rdmsr", out("edx") high, out("eax") low, in("ecx") msr);

    low as u64 | (high as u64) << 32
}


pub unsafe fn wrmsr(msr: u32, value: u64) {
    let low:  u32 = value as u32;
    let high: u32 = (value >> 32) as u32;

    asm!("wrmsr", in("edx") high, in("eax") low, in("ecx") msr);
}

pub fn halt() -> ! {
    loop {
        unsafe {
            asm!(r#"
                cli
                hlt
            "#);
        }
    }
}
