mod pci_data;
use pci_data::PciDeviceData;

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
