use page_table::PhysAddr;

pub struct Hpet {
    registers:    &'static mut [u64],
    timers:       usize,
    clock_period: u64,
    is_64bit:     bool,
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
        self.is_64bit     = count_size_cap != 0;

        assert!(self.timers       > 0,  "Invalid HPET timer count.");
        assert!(self.clock_period > 0,  "Invalid HPET clock period.");
    }

    unsafe fn initialize(&mut self) {
        self.get_capabilities();

        {
            // General Configuration Register.
            let mut configuration = self.read(0x10);

            // Disable the timer.
            configuration &= !(1 << 0);

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

                // Disable interrupts for this timer.
                value &= !(1 << 2);

                self.write(config_cap_register, value);
            }

            // Don't modify Comparator Value Register and FSB Interrupt Route Register.
        }

        // Reset Main Counter Value Register.
        self.write(0xf0, 0);
    }

    fn set_enabled(&mut self, enabled: bool) {
        // General Configuration Register.
        let mut configuration = unsafe { self.read(0x10) };

        if enabled {
            configuration |= 1 << 0;
        } else {
            configuration &= !(1 << 0);
        }

        unsafe {
            self.write(0x10, configuration)
        }
    }

    pub unsafe fn new(hpet_base: PhysAddr) -> Self {
        // Map HPET to the non-cacheable memory.
        let virt_addr = crate::mm::map_mmio(hpet_base, 4096, false);

        // The timer register space is 1024 bytes. The registers are generally aligned on 64-bit
        // boundaries to simplify implementation with IA64 processors.
        let registers = core::slice::from_raw_parts_mut(virt_addr.0 as *mut u64,
                                                        1024 / core::mem::size_of::<u64>());

        let mut hpet = Self {
            registers,
            clock_period: 0,
            timers:       0,
            is_64bit:     false,
        };

        hpet.initialize();

        hpet
    }

    pub fn counter(&self) -> u64 {
        // Main Counter Value Register.
        unsafe {
            self.read(0xf0)
        }
    }

    pub fn enable(&mut self) {
        self.set_enabled(true)
    }

    pub fn disable(&mut self) {
        self.set_enabled(false)
    }

    pub fn period(&self) -> u64 {
        self.clock_period
    }

    pub fn is_64bit(&self) -> bool {
        self.is_64bit
    }
}
