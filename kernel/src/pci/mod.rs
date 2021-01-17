mod pci_data;
use pci_data::PciDeviceData;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct PciHeader {
    pub vendor_id:       u16,
    pub device_id:       u16,
    pub command:         u16,
    pub status:          u16,
    pub revision:        u8,
    pub prog_if:         u8,
    pub subclass:        u8,
    pub class:           u8,
    pub cache_line_size: u8,
    pub latency_timer:   u8,
    pub header_type:     u8,
    pub bist:            u8,
    pub bar0:            u32,
    pub bar1:            u32,
    pub bar2:            u32,
    pub bar3:            u32,
    pub bar4:            u32,
    pub bar5:            u32,
}

const INTEL_MMIO_SIZE: usize = 128 * 1024;

use crate::mm;
use page_table::PhysAddr;

struct Driver {
    mmio: &'static mut [u32; INTEL_MMIO_SIZE / core::mem::size_of::<u32>()],
}

impl Driver {
    fn new(pci_header: PciHeader) -> Self {
        let bar = pci_header.bar0;

        let memory       = (bar >> 0) & 1 == 0;
        let typ          = (bar >> 1) & 0b11;
        let prefetchable = (bar >> 3) & 1 == 1;

        assert!(memory, "Intel NIC BAR0 is not in memory space.");

        let base_address = match typ {
            0 => (bar & 0xffff_fff0) as u64,
            2 => {
                let lo = (pci_header.bar0 & 0xffff_fff0) as u64;
                let hi = (pci_header.bar1 & 0xffff_ffff) as u64;

                (hi << 32) | lo
            }
            _ => panic!("Intel NIC BAR0 invalid type {}.", typ),
        };

        assert!(base_address % 4096 == 0, "Intel NIC MMIO is not page aligned.");

        println!("Intel NIC MMIO base: 0x{:x} (prefetchable: {:?}).", base_address, prefetchable);

        let mmio = unsafe {
            let virt_addr = mm::map_mmio(PhysAddr(base_address), INTEL_MMIO_SIZE as u64,
                                         mm::PAGE_UNCACHEABLE);

            &mut *(virt_addr.0 as *mut [u32; INTEL_MMIO_SIZE / core::mem::size_of::<u32>()])
        };

        let mut driver = Self {
            mmio,
        };

        driver.initialize();

        driver
    }

    fn initialize(&mut self) {
    }

    unsafe fn read(&self, offset: usize) -> u32 {
        assert!(offset % core::mem::size_of::<u32>() == 0,
                "Unaligned register read {:x}.", offset);

        let index = offset / core::mem::size_of::<u32>();

        core::ptr::read_volatile(&self.mmio[index])
    }

    unsafe fn write(&mut self, offset: usize, value: u32) {
        assert!(offset % core::mem::size_of::<u32>() == 0,
                "Unaligned register write {:x}.", offset);

        let index = offset / core::mem::size_of::<u32>();

        core::ptr::write_volatile(&mut self.mmio[index], value)
    }
}

pub unsafe fn initialize() {
    const PCI_CONFIG_ADDRESS: u16 = 0xcf8;
    const PCI_CONFIG_DATA:    u16 = 0xcfc;

    println!();
    println!("PCI devices:");

    for bus in 0..256 {
        for device in 0..32 {
            for function in 0..8 {
                let address = (bus << 16) | (device << 11) | (function << 8) | (1 << 31);

                cpu::outd(PCI_CONFIG_ADDRESS, address);

                let device_vendor_id = cpu::ind(PCI_CONFIG_DATA);
                let vendor_id        = (device_vendor_id >>  0) as u16;
                let device_id        = (device_vendor_id >> 16) as u16;

                // Skip invalid devices.
                if vendor_id == 0xffff && device_id == 0xffff {
                    continue;
                }

                assert!(core::mem::size_of::<PciHeader>() % core::mem::size_of::<u32>() == 0,
                        "PCI header size is not divisible by 4.");

                let mut header =
                    [0u32; core::mem::size_of::<PciHeader>() / core::mem::size_of::<u32>()];

                for (index, register) in header.iter_mut().enumerate() {
                    let offset = (index * core::mem::size_of::<u32>()) as u32;

                    cpu::outd(PCI_CONFIG_ADDRESS, address + offset);

                    *register = cpu::ind(PCI_CONFIG_DATA);
                }

                let header: PciHeader = core::ptr::read_unaligned(
                    header.as_ptr() as *const PciHeader,
                );

                if vendor_id == 0x8086 && device_id == 0x100e {
                    Driver::new(header);
                }

                if vendor_id == 0x8086 && device_id == 0x1539 {
                    Driver::new(header);
                }

                if let Some(data) = PciDeviceData::find(vendor_id, device_id) {
                    println!("{:04x}:{:04x}: {} {}", vendor_id, device_id, data.vendor_name,
                             data.device_name);
                } else {
                    // Skip devices known by us but unknown for PCI data.
                    match (vendor_id, device_id) {
                        (0x1234, 0x1111) => continue, // QEMU VGA controller.
                        _                => (),
                    }

                    color_println!(0xffff00, "WARNING: Unknown PCI device {:04x}:{:04x}.",
                                   vendor_id, device_id);
                }
            }
        }
    }

    println!();
}
