use crate::{efi, BOOT_BLOCK};

pub unsafe fn locate(system_table: *mut efi::EfiSystemTable) {
    use efi::EfiGuid;

    const EFI_ACPI_TABLE_GUID: EfiGuid =
        EfiGuid(0xeb9d2d30, 0x2d88, 0x11d3, [0x9a, 0x16, 0x0, 0x90, 0x27, 0x3f, 0xc1, 0x4d]);

    const EFI_ACPI_20_TABLE_GUID: EfiGuid =
        EfiGuid(0x8868e871, 0xe4f1, 0x11d3, [0xbc, 0x22, 0x0, 0x80, 0xc7, 0x3c, 0x88, 0x81]);

    let configuration_table = {
        let entry_count = (*system_table).table_entries;
        let table       = (*system_table).configuration_table;

        core::slice::from_raw_parts(table, entry_count)
    };

    let mut acpi = boot_block::AcpiTables {
        rsdt: None,
        xsdt: None,
    };

    for entry in configuration_table {
        match entry.guid {
            EFI_ACPI_TABLE_GUID => {
                let table = core::ptr::read_unaligned(entry.table as *const acpi::Rsdp);

                acpi.rsdt = Some(table.rsdt_addr as u64);
            }
            EFI_ACPI_20_TABLE_GUID => {
                let table = core::ptr::read_unaligned(entry.table as *const acpi::RsdpExtended);

                acpi.rsdt = Some(table.rsdp.rsdt_addr as u64);
                acpi.xsdt = Some(table.xsdt_addr);

                break;
            }
            _ => (),
        }
    }

    *BOOT_BLOCK.acpi_tables.lock() = acpi;
}
