use page_table::PhysAddr;
use crate::{mm, time};

const IA32_APIC_BASE: u32 = 0x1b;
const APIC_BASE:      u64 = 0xfee0_0000;

pub const APIC_TIMER_IRQ:    u8  = 0xfe;
pub const SPURIOUS_IRQ:      u8  = 0xff;
pub const APIC_TIMER_PERIOD: f64 = 0.05;

pub enum Register {
    ApicID                   = 0x20,
    Eoi                      = 0xb0,
    SpuriousInterruptVector  = 0xf0,
    TimerLvt                 = 0x320,
    TimerInitialCount        = 0x380,
    TimerCurrentCount        = 0x390,
    TimerDivideConfiguration = 0x3e0,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ApicMode {
    XApic,
    X2Apic,
}

pub enum Apic {
    XApic(&'static mut [u32]),
    X2Apic,
}

impl Apic {
    pub fn mode(&self) -> ApicMode {
        match self {
            Apic::XApic(..) => ApicMode::XApic,
            Apic::X2Apic    => ApicMode::X2Apic,
        }
    }

    pub fn apic_id(&self) -> u32 {
        // Read the APIC ID register.
        let apic_id = unsafe { self.read(Register::ApicID) };

        // Adjust the APIC ID based on current APIC mode.
        match self {
            Apic::XApic(_) => apic_id >> 24,
            Apic::X2Apic   => apic_id,
        }
    }

    pub unsafe fn ipi(&mut self, dest_apic: u32, ipi: u32) {
        // Adjust the destination APIC ID based on current APIC mode.
        let dest_apic = match self {
            Apic::XApic(_) => dest_apic << 24,
            Apic::X2Apic   => dest_apic,
        };

        // Create ICR for this IPI request.
        let icr = ((dest_apic as u64) << 32) | ipi as u64;

        // Perform the IPI.
        self.write_icr(icr);
    }

    pub unsafe fn write_icr(&mut self, value: u64) {
        match self {
            Apic::XApic(mapping) => {
                // Write the high part of the ICR.
                core::ptr::write_volatile(&mut mapping[0x310 / 4], (value >> 32) as u32);

                // Write the low part of the ICR. This will cause the interrupt to be sent.
                core::ptr::write_volatile(&mut mapping[0x300 / 4], (value >> 0) as u32);
            }
            Apic::X2Apic => {
                // X2Apic has a single, 64 bit ICR register.
                cpu::wrmsr(0x830, value);
            }
        }
    }

    #[allow(dead_code)]
    pub unsafe fn read(&self, register: Register) -> u32 {
        let offset = register as u32;

        // Make sure that provided offset is a APIC valid register.
        assert!(offset < 0x400 && offset % 16 == 0, "Invalid APIC register passed to `read`.");

        // Perform the read according to the APIC mode.
        match self {
            Apic::XApic(mapping) => {
                core::ptr::read_volatile(&mapping[offset as usize / 4])
            }
            Apic::X2Apic => {
                cpu::rdmsr(0x800 + offset / 16) as u32
            }
        }
    }

    #[allow(dead_code)]
    pub unsafe fn write(&mut self, register: Register, value: u32) {
        let offset = register as u32;

        // Make sure that provided offset is a APIC valid register.
        assert!(offset < 0x400 && offset % 16 == 0, "Invalid APIC register passed to `write`.");

        // Perform the write according to the APIC mode.
        match self {
            Apic::XApic(mapping) => {
                core::ptr::write_volatile(&mut mapping[offset as usize / 4], value);
            }
            Apic::X2Apic => {
                cpu::wrmsr(0x800 + offset / 16, value as u64);
            }
        }
    }

    pub unsafe fn enable_timer(&mut self) {
        let lvt          = APIC_TIMER_IRQ as u32;
        let periodic_lvt = lvt | (1 << 17);
        let masked_lvt   = lvt | (1 << 16);

        // Start the APIC timer. Set divide by 16.
        self.write(Register::TimerDivideConfiguration, 3);
        self.write(Register::TimerLvt,                 lvt);
        self.write(Register::TimerInitialCount,        0xffff_ffff);

        let start_time = time::get();

        // Wait for about `APIC_TIMER_PERIOD` seconds.
        loop {
            let time = time::get();
            if  time::difference(start_time, time) >= APIC_TIMER_PERIOD {
                break;
            }
        }

        // Stop the APIC timer.
        self.write(Register::TimerLvt, masked_lvt);

        // Get the amount of ticks it takes to elapse `APIC_TIMER_PERIOD` seconds.
        let ticks = 0xffff_ffff - self.read(Register::TimerCurrentCount);

        if core!().id == 0 {
            println!("APIC timer period: {}ms ({} ticks).",
                     (APIC_TIMER_PERIOD * 1000.0) as u32, ticks);
        }

        // Configure the APIC timer to tick in `APIC_TIMER_PERIOD`.
        self.write(Register::TimerLvt,          periodic_lvt);
        self.write(Register::TimerInitialCount, ticks);
    }

    pub unsafe fn eoi() {
        // Don't lock the APIC to avoid potential deadlocks. We will only EOI anyway.
        let apic = &mut *core!().apic.bypass();

        if let Some(apic) = apic {
            apic.write(Register::Eoi, 0);
        }
    }
}

/// Remap and disable the PIC. This should also drain all interrupts.
unsafe fn disable_pic() {
    unsafe fn write_pic(port: u16, data: u8) {
        cpu::outb(port, data);
        cpu::outb(0x80, 0);
    }

    // Disable the PIC by masking off all interrupts from it.
    write_pic(0xa1, 0xff);
    write_pic(0x21, 0xff);

    // Start the PIC initialization sequence in cascade mode.
    write_pic(0x20, 0x11);
    write_pic(0xa0, 0x11);

    // Setup IRQ offsets for master and slave PIC.
    write_pic(0x21, 32);
    write_pic(0xa1, 40);

    // Configure PIC layout.
    write_pic(0x21, 4);
    write_pic(0xa1, 2);

    // Set PIC 8086 mode.
    write_pic(0x21, 0x01);
    write_pic(0xa1, 0x01);

    // Disable the PIC again by masking off all interrupts from it.
    write_pic(0xa1, 0xff);
    write_pic(0x21, 0xff);
}

pub unsafe fn initialize() {
    #[allow(clippy::assertions_on_constants)]
    {
        // Make sure the APIC base address is valid.
        assert!(APIC_BASE > 0 && APIC_BASE == (APIC_BASE & 0xf_ffff_f000),
                "APIC base address is invalid.");
    }

    // Make sure that the APIC hasn't been initialized yet.
    assert!(core!().apic.lock().is_none(), "APIC was already initialized.");

    let features = cpu::get_features();

    // Make sure that the APIC is actually supported by the CPU.
    assert!(features.apic, "APIC is not supported by this CPU.");

    let x2apic = features.x2apic;

    // Get the current APIC state.
    let state = cpu::rdmsr(IA32_APIC_BASE);

    // We can't reenable APIC if it was disabled by the BIOS.
    assert!(state & (1 << 11) != 0, "APIC was disabled by the BIOS.");

    // Set the new APIC base.
    let mut state = (state & !0xf_ffff_f000) | APIC_BASE;

    // If the CPU supports x2APIC mode then enable it.
    if x2apic {
        state |= 1 << 10;
    }

    // Disable the PIC before enabling APIC.
    disable_pic();

    // Set the new APIC state.
    cpu::wrmsr(IA32_APIC_BASE, state);

    let mut apic = if !x2apic {
        let virt_addr = mm::map_mmio(PhysAddr(APIC_BASE), 4096, mm::PAGE_UNCACHEABLE);

        #[allow(clippy::size_of_in_element_count)]
        {
            // Highest APIC register is at address 0x3f0, so whole mapping needs to be 0x400 bytes.
            Apic::XApic(core::slice::from_raw_parts_mut(virt_addr.0 as *mut u32,
                                                        0x400 / core::mem::size_of::<u32>()))
        }
    } else {
        Apic::X2Apic
    };

    // Software enable the APIC, set spurious interrupt vector to `SPURIOUS_IRQ` (0xff).
    apic.write(Register::SpuriousInterruptVector, (SPURIOUS_IRQ as u32) | (1 << 8));

    let apic_id = apic.apic_id();

    // Cache the APIC ID for this core.
    core!().set_apic_id(apic_id);

    *core!().apic.lock() = Some(apic);
}
