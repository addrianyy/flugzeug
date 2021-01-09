use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use crate::mm;
use acpi::Header;
use page_table::PhysAddr;

pub type TableSignature = [u8; 4];
pub type TablePayload   = (PhysAddr, usize);

// We don't use lock here as we will initialize this before launching APs and never modify it
// again.
static mut ACPI_TABLES: Option<BTreeMap<TableSignature, Vec<TablePayload>>> = None;

enum SdtType {
    Rsdt,
    Xsdt,
}

impl core::fmt::Display for SdtType {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        match self {
            SdtType::Rsdt => write!(f, "RSDT"),
            SdtType::Xsdt => write!(f, "XSDT"),
        }
    }
}

unsafe fn parse_header(phys_addr: PhysAddr) -> (Header, Option<(PhysAddr, usize)>) {
    let header: Header = mm::read_phys_unaligned(phys_addr);

    // Get the table address.
    let payload_addr = PhysAddr(phys_addr.0 + core::mem::size_of::<Header>() as u64);

    // Get the table size.
    let payload_size = header.length.checked_sub(core::mem::size_of::<Header>() as u32)
        .expect("ACPI payload size has underflowed.");

    // Calculate table checkum.
    let checksum = (phys_addr.0..phys_addr.0 + header.length as u64)
        .fold(0u8, |acc, phys_addr| {
            acc.wrapping_add(mm::read_phys(PhysAddr(phys_addr)))
        });

    // Get the payload only if the checksum is valid.
    let payload = if checksum == 0 {
        Some((payload_addr, payload_size as usize))
    } else {
        None
    };

    (header, payload)
}

unsafe fn parse_system_table(system_table: PhysAddr, sdt_type: SdtType)
    -> Vec<(Header, PhysAddr, usize)>
{
    let (sdt, payload) = parse_header(system_table);
    let (sdt_payload, sdt_size) = payload
        .unwrap_or_else(|| panic!("{} checksum is invalid.", sdt_type));

    // Make sure that the signature matches and get entry size.
    let entry_size = match sdt_type {
        SdtType::Rsdt => {
            assert!(&sdt.signature == b"RSDT", "RSDT signature is invalid.");

            // RSDT pointers are 32 bit wide.
            core::mem::size_of::<u32>()
        }
        SdtType::Xsdt => {
            assert!(&sdt.signature == b"XSDT", "XSDT signature is invalid.");

            // XSDT pointers are 64 bit wide.
            core::mem::size_of::<u64>()
        }
    };

    let sdt_size    = sdt_size as usize;
    let entry_count = sdt_size / entry_size;

    // Make sure that the SDT size is valid.
    assert!(sdt_size % entry_size == 0, "{} size is not divisible by entry size.", sdt_type);

    let mut tables = Vec::with_capacity(entry_count);

    // Go through each table in the SDT.
    for entry in 0..entry_count {
        let table_addr = PhysAddr({
            // Get the physical address of current SDT entry.
            let entry_addr = sdt_payload.0 as usize + entry * entry_size;
            let entry_addr = PhysAddr(entry_addr as u64);

            // Read the address of the table.
            match sdt_type {
                SdtType::Rsdt => mm::read_phys_unaligned::<u32>(entry_addr) as u64,
                SdtType::Xsdt => mm::read_phys_unaligned::<u64>(entry_addr),
            }
        });

        let (header, payload) = parse_header(table_addr);

        if let Some((payload, payload_size)) = payload {
            tables.push((header, payload, payload_size));
        } else {
            println!("Signature verification of ACPI table {:?} failed.",
                     core::str::from_utf8(&header.signature));
        }
    }

    tables
}

pub fn get_acpi_tables(signature: &str) -> Option<&'static [(PhysAddr, usize)]> {
    assert!(signature.len() == 4, "Invalid ACPI table signature {}.", signature);

    let acpi_tables = unsafe {
        ACPI_TABLES.as_ref().expect("Cannot get ACPI tables before ACPI initialization.")
    };

    if let Some(tables) = acpi_tables.get(signature.as_bytes()) {
        assert!(!tables.is_empty(), "Invalid empty ACPI table list.");

        Some(tables)
    } else {
        None
    }
}

pub fn get_only_acpi_table(signature: &str) -> Option<(PhysAddr, usize)> {
    if let Some(tables) = get_acpi_tables(signature) {
        if tables.len() == 1 {
            return Some(tables[0]);
        } else {
            println!("Multiple {} ACPI tables found on the system.", signature);
        }
    }

    None
}

pub fn get_first_acpi_table(signature: &str) -> Option<(PhysAddr, usize)> {
    if let Some(tables) = get_acpi_tables(signature) {
        Some(tables[0])
    } else {
        None
    }
}

pub unsafe fn initialize() {
    // Make sure that the ACPI hasn't been initialized yet.
    assert!(ACPI_TABLES.is_none(), "ACPI tables were already initialized.");

    let tables = {
        // Get the addreses of ACPI system tables.
        let system_tables = *core!().boot_block.acpi_tables.lock();

        // Get the preferred ACPI system table.
        let (sdt_addr, sdt_type) = match (system_tables.rsdt, system_tables.xsdt) {
            // XSDT takes priority.
            (_, Some(xsdt)) => (xsdt, SdtType::Xsdt),

            // If there is no XSDT then fallback to RSDT.
            (Some(rsdt), _) => (rsdt, SdtType::Rsdt),

            _ => panic!("Bootloader didn't provide address of any ACPI system table."),
        };

        println!("Using {} system table at address 0x{:x}.", sdt_type, sdt_addr);

        // Get all subtables in the system table.
        parse_system_table(PhysAddr(sdt_addr), sdt_type)
    };

    let mut table_map = BTreeMap::new();
    let mut printed   = 0;

    for &(header, payload, payload_size) in &tables {
        // Dump this ACPI table.
        if let Ok(signature) = core::str::from_utf8(&header.signature) {
            if printed % 3 == 0 && printed > 0 {
                println!();
            }

            print!("  {}: 0x{:x}  ", signature, payload.0);

            printed += 1;
        }

        // Add table to global ACPI table list.
        table_map.entry(header.signature)
            .or_insert_with(Vec::new)
            .push((payload, payload_size));
    }

    println!();
    println!();

    // Set global ACPI table map.
    ACPI_TABLES = Some(table_map);
}
