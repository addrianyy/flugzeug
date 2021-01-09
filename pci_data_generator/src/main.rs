use std::io::{Write, BufWriter};
use std::fs::File;

const API: &'static str = include_str!("api.rs");

struct Vendor {
    vendor_id: u16,
    name:      String,
    devices:   Vec<Device>,
}

struct Device {
    device_id: u16,
    name:      String,
}

fn escape_string(string: &str) -> String {
    let mut result = String::with_capacity(string.len());

    for ch in string.chars() {
        let escape = match ch {
            '"'  => true,
            '\\' => true,
            _    => false,
        };

        if escape {
            result.push('\\');
        }

        result.push(ch);
    }

    result
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const PCI_IDS_URL: &str = "https://pci-ids.ucw.cz/v2.2/pci.ids";

    let mut vendors = Vec::new();
    let pci_ids     = reqwest::blocking::get(PCI_IDS_URL)?.text()?;

    for line in pci_ids.lines() {
        let trimmed = line.trim();
        if  trimmed.is_empty() || trimmed.starts_with("#") {
            continue;
        }

        let tabs = if line.starts_with("\t\t\t") {
            panic!()
        } else if line.starts_with("\t\t") {
            2
        } else if line.starts_with("\t") {
            1
        } else {
            0
        };

        // Ignore subvendors and subdevices.
        if tabs == 2 {
            continue;
        }

        let whitespace = trimmed.find(|c| char::is_whitespace(c)).unwrap();
        let id         = &trimmed[..whitespace];

        // Break when device classes list starts.
        if id == "C" {
            break;
        }

        let id   = u16::from_str_radix(id, 16).unwrap();
        let name = trimmed[whitespace..].trim();

        println!("{} {}", id, name);

        match tabs {
            0 => {
                let vendor = Vendor {
                    vendor_id: id,
                    name:      name.to_string(),
                    devices:   Vec::new(),
                };

                vendors.push(vendor);
            }
            1 => {
                let device = Device {
                    device_id: id,
                    name:      name.to_string(),
                };

                let vendor_index = vendors.len() - 1;

                vendors[vendor_index].devices.push(device);
            }
            _ => unreachable!(),
        }
    }

    vendors.retain(|vendor| !vendor.devices.is_empty());
    vendors.sort_by_key(|vendor| vendor.vendor_id);

    for vendor in &mut vendors {
        vendor.devices.sort_by_key(|device| device.device_id);
    }

    for vendor in &vendors {
        println!("{:04x} - {}", vendor.vendor_id, vendor.name);

        for device in &vendor.devices {
            println!("    {:04x} - {}", device.device_id, device.name);
        }

        println!();
    }

    let output_path = std::env::args().nth(1).unwrap_or_else(|| String::from("pci_data.rs"));
    let mut output  = BufWriter::new(File::create(output_path)?);
    
    writeln!(output, "{}", API)?;

    writeln!(output, "type VendorEntry = (u16, &'static str, &'static [DeviceEntry]);")?;
    writeln!(output, "type DeviceEntry = (u16, &'static str);")?;
    writeln!(output)?;

    writeln!(output, "const VENDORS: &[VendorEntry] = &[")?;

    for (index, vendor) in vendors.iter().enumerate() {
        writeln!(output, "    (0x{:04x}, \"{}\", VENDOR_{}_DEVICES),", vendor.vendor_id,
                 escape_string(&vendor.name), index)?;
    }

    writeln!(output, "];")?;

    for (index, vendor) in vendors.iter().enumerate() {
        writeln!(output, "const VENDOR_{}_DEVICES: &[DeviceEntry] = &[", index)?;

        for device in &vendor.devices {
            writeln!(output, "    (0x{:04x}, \"{}\"),", device.device_id,
                    escape_string(&device.name))?;
        }

        writeln!(output, "];")?;
        writeln!(output)?;
    }

    Ok(())
}
