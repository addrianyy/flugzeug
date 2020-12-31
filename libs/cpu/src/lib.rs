#![no_std]
#![feature(asm)]
#![allow(clippy::identity_op, clippy::missing_safety_doc)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

#[derive(Default, Copy, Clone)]
pub struct Cpuid {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
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

pub fn cpuid_identifier<'a>(start: u32, count: usize, mut buffer: &'a mut [u8])
    -> Option<&'a str>
{
    buffer = &mut buffer[..count * 4 * 4];

    for (index, chunk) in buffer.chunks_mut(4 * 4).enumerate() {
        let command: u32 = start + index as u32;

        let cpuid  = cpuid(command, 0);
        let output = [cpuid.eax, cpuid.ebx, cpuid.ecx, cpuid.edx];

        for (chunk, value) in chunk.chunks_mut(4).zip(output.iter()) {
            chunk.copy_from_slice(&value.to_le_bytes());
        }
    }

    let size = buffer.iter().position(|x| *x == 0).unwrap_or(buffer.len());
    let name = core::str::from_utf8(&buffer[..size]).map(|identifier| {
        // This is safe as CPUID identifiers are ASCII only.
        let size = identifier.rfind(|x| !char::is_whitespace(x))
            .map(|index| index + 1)
            .unwrap_or(identifier.len());

        &identifier[..size]
    }).ok();

    name
}

#[derive(Default, Debug)]
pub struct CpuFeatures {
    pub fpu:            bool,
    pub vme:            bool,
    pub de:             bool,
    pub pse:            bool,
    pub tsc:            bool,
    pub mmx:            bool,
    pub fxsr:           bool,
    pub sse:            bool,
    pub sse2:           bool,
    pub htt:            bool,
    pub sse3:           bool,
    pub ssse3:          bool,
    pub sse4_1:         bool,
    pub sse4_2:         bool,
    pub x2apic:         bool,
    pub aesni:          bool,
    pub xsave:          bool,
    pub avx:            bool,
    pub avx2:           bool,
    pub fma:            bool,
    pub apic:           bool,
    pub vmx:            bool,
    pub svm:            bool,
    pub lahf:           bool,
    pub lzcnt:          bool,
    pub prefetchw:      bool,
    pub syscall:        bool,
    pub xd:             bool,
    pub rdtscp:         bool,
    pub bits64:         bool,
    pub avx512f:        bool,
    pub page2m:         bool,
    pub page1g:         bool,
    pub invariant_tsc:  bool,
}

pub fn get_features() -> CpuFeatures {
    let mut features = CpuFeatures::default();

    let max_cpuid          = cpuid(0, 0).eax;
    let max_extended_cpuid = cpuid(0x80000000, 0).eax;

    if max_cpuid >= 1 {
        let cpuid = cpuid(1, 0);

        features.fpu    = ((cpuid.edx >>  0) & 1) == 1;
        features.vme    = ((cpuid.edx >>  1) & 1) == 1;
        features.de     = ((cpuid.edx >>  2) & 1) == 1;
        features.pse    = ((cpuid.edx >>  3) & 1) == 1;
        features.page2m = ((cpuid.edx >>  3) & 1) == 1;
        features.tsc    = ((cpuid.edx >>  4) & 1) == 1;
        features.apic   = ((cpuid.edx >>  9) & 1) == 1;
        features.mmx    = ((cpuid.edx >> 23) & 1) == 1;
        features.fxsr   = ((cpuid.edx >> 24) & 1) == 1;
        features.sse    = ((cpuid.edx >> 25) & 1) == 1;
        features.sse2   = ((cpuid.edx >> 26) & 1) == 1;
        features.htt    = ((cpuid.edx >> 28) & 1) == 1;

        features.sse3    = ((cpuid.ecx >>  0) & 1) == 1;
        features.vmx     = ((cpuid.ecx >>  5) & 1) == 1;
        features.ssse3   = ((cpuid.ecx >>  9) & 1) == 1;
        features.fma     = ((cpuid.ecx >> 12) & 1) == 1;
        features.sse4_1  = ((cpuid.ecx >> 19) & 1) == 1;
        features.sse4_2  = ((cpuid.ecx >> 20) & 1) == 1;
        features.x2apic  = ((cpuid.ecx >> 21) & 1) == 1;
        features.aesni   = ((cpuid.ecx >> 25) & 1) == 1;
        features.xsave   = ((cpuid.ecx >> 26) & 1) == 1;
        features.avx     = ((cpuid.ecx >> 28) & 1) == 1;
    }

    if max_cpuid >= 7 {
        let cpuid = cpuid(7, 0);

        features.avx2    = ((cpuid.ebx >>  5) & 1) == 1;
        features.avx512f = ((cpuid.ebx >> 16) & 1) == 1;
    }

    if max_extended_cpuid >= 0x80000001 {
        let cpuid = cpuid(0x80000001, 0);

        features.lahf      = ((cpuid.ecx >> 0) & 1) == 1;
        features.svm       = ((cpuid.ecx >> 2) & 1) == 1;
        features.lzcnt     = ((cpuid.ecx >> 5) & 1) == 1;
        features.prefetchw = ((cpuid.ecx >> 8) & 1) == 1;

        features.syscall     = ((cpuid.edx >> 11) & 1) == 1;
        features.xd          = ((cpuid.edx >> 20) & 1) == 1;
        features.page1g      = ((cpuid.edx >> 26) & 1) == 1;
        features.rdtscp      = ((cpuid.edx >> 27) & 1) == 1;
        features.bits64      = ((cpuid.edx >> 29) & 1) == 1;
    }

    if max_extended_cpuid >= 0x80000007 {
        let cpuid = cpuid(0x80000007, 0);

        features.invariant_tsc = ((cpuid.edx >> 8) & 1) == 1;
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

pub unsafe fn invlpg(addr: usize) {
    asm!("invlpg [{}]", in(reg) addr);
}

pub fn get_xcr0() -> u64 {
    let low:  u32;
    let high: u32;

    unsafe {
        asm!(
            r#"
                xor eax, eax
                xor edx, edx
                xor ecx, ecx
                xgetbv
            "#,
            out("edx") high, out("eax") low, out("ecx") _,
        );
    }

    low as u64 | (high as u64) << 32
}

pub unsafe fn enable_interrupts() {
    asm!("sti");
}

pub unsafe fn disable_interrupts() {
    asm!("cli");
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

#[derive(Copy, Clone, Default)]
#[repr(C, packed)]
pub struct TableRegister {
    pub limit: u16,
    pub base:  usize,
}

impl TableRegister {
    pub fn zero() -> Self {
        Self::default()
    }
}

pub fn get_gdt() -> TableRegister {
    let mut table = TableRegister::default();

    unsafe {
        asm!("sgdt [{}]", in(reg) &mut table);
    }

    table
}

pub fn get_idt() -> TableRegister {
    let mut table = TableRegister::default();

    unsafe {
        asm!("sidt [{}]", in(reg) &mut table);
    }

    table
}

pub fn get_tr() -> u16 {
    let tr: u16;

    unsafe {
        asm!("str {:x}", out(reg) tr);
    }

    tr
}

pub unsafe fn set_gdt(table: &TableRegister) {
    asm!("lgdt [{}]", in(reg) table);
}

pub unsafe fn set_idt(table: &TableRegister) {
    asm!("lidt [{}]", in(reg) table);
}

pub unsafe fn set_tr(tr: u16) {
    asm!("ltr {:x}", in(reg) tr);
}

pub fn get_cr0() -> usize {
    let result;

    unsafe {
        asm!("mov {}, cr0", out(reg) result);
    }

    result
}

pub fn get_cr2() -> usize {
    let result;

    unsafe {
        asm!("mov {}, cr2", out(reg) result);
    }

    result
}

pub fn get_cr3() -> usize {
    let result;

    unsafe {
        asm!("mov {}, cr3", out(reg) result);
    }

    result
}

pub fn get_cr4() -> usize {
    let result;

    unsafe {
        asm!("mov {}, cr4", out(reg) result);
    }

    result
}

pub unsafe fn set_cr3(cr3: usize) {
    asm!("mov cr3, {}", in(reg) cr3);
}
