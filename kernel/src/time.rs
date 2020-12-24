use core::convert::TryInto;

use crate::mm;
use page_table::PhysAddr;

#[inline(always)]
#[allow(unused)]
pub fn get_tsc() -> u64 {
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
}

#[inline(always)]
fn get_tsc_ordered() -> u64 {
    let mut aux = 0;

    unsafe {
        core::arch::x86_64::__rdtscp(&mut aux)
    }
}

struct Hpet {
    registers:    &'static mut [u64],
    timers:       usize,
    clock_period: u64,
}

impl Hpet {
    unsafe fn read(&self, offset: usize) -> u64 {
        assert!(offset % 8 == 0, "Register offset {} is not 64 bit aligned.", offset);

        core::ptr::read_volatile(&self.registers[offset / 8])
    }

    unsafe fn write(&mut self, offset: usize, value: u64) {
        assert!(offset % 8 == 0, "Register offset {} is not 64 bit aligned.", offset);

        core::ptr::write_volatile(&mut self.registers[offset / 8], value);
    }

    unsafe fn get_capabilities(&mut self) {
        // General Capabilities and ID Register.
        let capabilities = self.read(0x00);

        // This read-only field indicates the period at which the counter
        // increments in femptoseconds (10^-15 seconds).
        let clock_period = capabilities >> 32;

        // This bit is a 0 to indicate that the main counter is 32 bits wide
        // (and cannot operate in 64-bit mode).
        let count_size_cap = (capabilities >> 13) & 0b1;

        // This indicates the number of timers in this block. The number in this
        // field indicates the last timer (i.e. if there are three timers, the value
        // will be 02h, four timers will be 03h, five timers will be 04h, etc.)
        let num_tim_cap = (capabilities >> 8) & 0b11111;

        self.timers       = (num_tim_cap + 1) as usize;
        self.clock_period = clock_period;

        assert!(count_size_cap    == 1, "HPET counter is 32 bit only.");
        assert!(self.timers       > 0,  "Invalid HPET timer count.");
        assert!(self.clock_period > 0,  "Invalid HPET clock period.");
    }

    unsafe fn initialize(&mut self) {
        self.get_capabilities();

        {
            // General Configuration Register.
            let mut configuration = self.read(0x10);

            // Disable LegacyReplacement route.
            configuration &= !(1 << 1);

            self.write(0x10, configuration);
        }

        {
            // General Interrupt Status Register.
            let mut interrupt_status = self.read(0x20);

            // Clear `Interrupt active` bit for every timer.
            for timer in 0..self.timers {
                interrupt_status &= !(1 << timer);
            }

            self.write(0x20, interrupt_status);
        }

        for timer in 0..self.timers {
            let registers_base      = 0x100 + timer * 0x20;
            let config_cap_register = registers_base + 0;

            // Setup Configuration and Capabilities Register.
            {
                let mut value = self.read(config_cap_register);

                // Disable FSB interrupt delivery.
                value &= !(1 << 14);

                // Disable 32 bit timer mode.
                value &= !(1 << 8);

                // Disable interrupts for this timer.
                value &= !(1 << 2);

                self.write(config_cap_register, value);
            }

            // Don't modify Comparator Value Register and FSB Interrupt Route Register.
        }
    }

    unsafe fn set_enabled(&mut self, enabled: bool) {
        // General Configuration Register.
        let mut configuration = self.read(0x10);

        if enabled {
            configuration |= 1 << 0;
        } else {
            configuration &= !(1 << 0);
        }

        self.write(0x10, configuration);
    }

    unsafe fn new(base_address: PhysAddr) -> Self {
        // Map HPET to the non-cacheable memory.
        let virt_addr = mm::map_mmio(base_address, 4096, false);

        // The timer register space is 1024 bytes. The registers are generally aligned on 64-bit
        // boundaries to simplify implementation with IA64 processors.
        let registers = core::slice::from_raw_parts_mut(virt_addr.0 as *mut u64,
                                                        1024 / core::mem::size_of::<u64>());

        let mut hpet = Self {
            registers,
            clock_period: 0,
            timers:       0,
        };

        hpet.initialize();

        hpet
    }

    fn counter(&self) -> u64 {
        // Main Counter Value Register.
        unsafe { self.read(0xf0) }
    }
}

unsafe fn create_hpet() -> Hpet {
    let (payload, payload_size) = crate::acpi::get_first_acpi_table("HPET")
        .expect("Failed to find HPET on the system.");

    assert!(payload_size >= core::mem::size_of::<acpi::HpetPayload>(),
            "Invalid HPET payload size {}.", payload_size);

    let payload: acpi::HpetPayload = mm::read_phys_unaligned(payload);

    assert!(payload.address.address_space == 0, "HPET is not memory mapped.");

    Hpet::new(PhysAddr(payload.address.address))
}

pub unsafe fn initialize() {
    // Amount of milliseconds that our TSC calibration will take.
    const CALIBRATION_MS: u128 = 50;

    const FEMTOSECONDS_IN_SECOND: u128 = 1_000_000_000_000_000;

    // Check if CPU supports invariant TSC which we rely on. This isn't hard error as some
    // VMs report that it's not supported and we want to test the kernel on them anyways.
    if !cpu::get_features().invariant_tsc {
        println!("WARNING: Timing may be off because CPU doesn't support invariant TSC.");
    }

    // Create HPET that we will use to calibrate TSC.
    let mut hpet = create_hpet();

    // Convert calibration milliseconds to femtoseconds.
    let calibration_fs = CALIBRATION_MS
        .checked_mul(FEMTOSECONDS_IN_SECOND / 1000)
        .expect("Cannot convert calibration milliseconds to femtoseconds.");

    // Get the number of HPET clocks that correspond to `CALIBRATION_MS` milliseconds.
    let calibration_clocks      = calibration_fs / (hpet.clock_period as u128);
    let calibration_clocks: u64 = calibration_clocks.try_into()
        .expect("Cannot fit calibration clocks in 64 bit integer.");

    // Start the calibration.
    hpet.set_enabled(true);

    let start_counter = hpet.counter();
    let start_tsc     = get_tsc_ordered();

    // Run for about `CALIBRATION_MS` milliseconds.
    while hpet.counter() < start_counter + calibration_clocks {}
    
    let end_counter = hpet.counter();
    let end_tsc     = get_tsc_ordered();

    hpet.set_enabled(false);

    let counter_delta = end_counter - start_counter;
    let tsc_delta     = end_tsc - start_tsc;

    // Calculate the amount of elapsed femtoseconds.
    let elapsed_fs = (counter_delta as u128).checked_mul(hpet.clock_period as u128)
        .expect("Failed to fit elapsed femtoseconds in 128 bit integer.");

    // Calculate how much femtoseconds every cycle takes.
    let fs_per_cycle = elapsed_fs.checked_div(tsc_delta as u128)
        .expect("Failed to calculate femtoseconds per cycle.");

    // Calculate the TSC frequency.
    let hz       = FEMTOSECONDS_IN_SECOND / fs_per_cycle;
    let khz: u64 = (hz / 1000).try_into()
        .expect("Failed to fit TSC frequency (KHz) in 64 bit integer.");

    println!("TSC frequency: {} KHz.", khz);
}
