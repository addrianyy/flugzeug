use crate::{efi, BOOT_BLOCK};
use efi::EfiGuid;

pub unsafe fn locate(system_table: *mut efi::EfiSystemTable) {
    const EFI_ACPI_TABLE_GUID: EfiGuid =
        EfiGuid(0xeb9d2d30, 0x2d88, 0x11d3, [0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);

    const EFI_ACPI_20_TABLE_GUID: EfiGuid =
        EfiGuid(0x8868e871, 0xe4f1, 0x11d3, [0xbc, 0x22, 0x0, 0x80, 0xc7, 0x3c, 0x88, 0x81]);

    let mut acpi_tables = BOOT_BLOCK.acpi_tables.lock();

    assert!(acpi_tables.rsdt.is_none() && acpi_tables.xsdt.is_none(),
            "ACPI tables were already initialized.");

    let configuration_table = {
        let entry_count = (*system_table).table_entries;
        let table       = (*system_table).configuration_table;

        core::slice::from_raw_parts(table, entry_count)
    };

    for entry in configuration_table {
        match entry.guid {
            EFI_ACPI_TABLE_GUID => {
                let table = core::ptr::read_unaligned(entry.table as *const acpi::Rsdp);

                acpi_tables.rsdt = Some(table.rsdt_addr as u64);
            }
            EFI_ACPI_20_TABLE_GUID => {
                let table = core::ptr::read_unaligned(entry.table as *const acpi::RsdpExtended);

                acpi_tables.rsdt = Some(table.rsdp.rsdt_addr as u64);
                acpi_tables.xsdt = Some(table.xsdt_addr);

                break;
            }
            _ => (),
        }
    }
}
