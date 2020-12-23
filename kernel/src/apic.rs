use page_table::{PageType, PAGE_PRESENT, PAGE_WRITE, PAGE_CACHE_DISABLE, PAGE_NX};
use crate::mm;

const IA32_APIC_BASE: u32 = 0x1b;
const APIC_BASE:      u64 = 0xfee0_0000;

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
    pub fn apic_id(&self) -> u32 {
        // Read the APIC ID register.
        let apic_id = unsafe { self.read(0x20) };

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

    pub fn mode(&self) -> ApicMode {
        match self {
            Apic::XApic(..) => ApicMode::XApic,
            Apic::X2Apic    => ApicMode::X2Apic,
        }
    }

    #[allow(dead_code)]
    unsafe fn read(&self, offset: u32) -> u32 {
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
    unsafe fn write(&mut self, offset: u32, value: u32) {
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
}

pub unsafe fn initialize() {
    // Make sure the APIC base address is valid.
    assert!(APIC_BASE > 0 && APIC_BASE == (APIC_BASE & 0xf_ffff_f000),
            "APIC base address is invalid.");

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

    // Mask off current APIC base.
    let state = state & !0xf_ffff_f000;

    // Set the new APIC base.
    let state = state | APIC_BASE;

    // Enable the xAPIC mode.
    let mut state = state | (1 << 11);

    // If the CPU supports x2APIC mode then enable it.
    if x2apic {
        state |= 1 << 10;
    }

    // Disable the PIC by masking off all interrupts from it.
    cpu::outb(0xa1, 0xff);
    cpu::outb(0x21, 0xff);

    // Set the new APIC state.
    cpu::wrmsr(IA32_APIC_BASE, state);

    let mut apic = if !x2apic {
        let mut page_table = core!().boot_block.page_table.lock();
        let page_table     = page_table.as_mut().unwrap();

        // Reserve 4K of memory for the APIC virtual region.
        let virt_addr = mm::reserve_virt_addr(4096);

        // Map APIC memory as writable, non-executable and non-cachable.
        page_table.map_raw(&mut mm::PhysicalMemory, virt_addr, PageType::Page4K,
                           PAGE_PRESENT | PAGE_WRITE | PAGE_CACHE_DISABLE | PAGE_NX | APIC_BASE,
                           true, false)
            .expect("Failed to map APIC to the virtual memory.");

        // Highest APIC register is at address 0x3f0, so whole mapping needs to be 0x400 bytes.
        Apic::XApic(core::slice::from_raw_parts_mut(virt_addr.0 as *mut u32,
                                                    0x400 / core::mem::size_of::<u32>()))
    } else {
        Apic::X2Apic
    };

    // Software enable the APIC, set spurious interrupt vector to 0xff.
    apic.write(0xf0, 0xff | (1 << 8));

    let apic_id = apic.apic_id();

    // Cache the APIC ID for this core.
    core!().set_apic_id(apic_id);

    *core!().apic.lock() = Some(apic);
}
