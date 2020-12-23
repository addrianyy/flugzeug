use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use alloc::collections::btree_set::BTreeSet;
use alloc::vec::Vec;

use page_table::PhysAddr;
use acpi::Header;
use crate::mm;

/// Maximum number of cores allowed on the system.
pub const MAX_CORES: usize = 1024;

/// Total number of cores available on the system.
static TOTAL_CORES: AtomicU32 = AtomicU32::new(0);

/// State of all cores on the system.
static CORE_STATES: [AtomicU8; MAX_CORES] = [AtomicU8::new(CoreState::None as u8); MAX_CORES];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum CoreState {
    /// The core has checked in with the kernel and is actively running.
    Online = 1,

    /// The core has been launched by the kernel, but has not yet registered with the kernel.
    Launched = 2,

    /// This APIC ID does not exist.
    None = 3,
}

impl From<u8> for CoreState {
    /// Convert a raw `u8` into an `CoreState`.
    fn from(value: u8) -> CoreState {
        match value {
            1 => CoreState::Online,
            2 => CoreState::Launched,
            3 => CoreState::None,
            _ => panic!("Invalid CoreState from `u8`."),
        }
    }
}

unsafe fn set_core_state(apic_id: u32, state: CoreState) {
    CORE_STATES[apic_id as usize].store(state as u8, Ordering::SeqCst);
}

fn core_state(apic_id: u32) -> CoreState {
    CORE_STATES[apic_id as usize].load(Ordering::SeqCst).into()
}

pub fn total_cores() -> u32 {
    let total_cores = TOTAL_CORES.load(Ordering::SeqCst);

    // Make sure that the ACPI initalization routine has filled in total number of cores.
    assert!(total_cores > 0, "Cannot get total number of cores before ACPI initialization.");

    total_cores
}

/// Notify that this core is online and wait for other cores to become online.
pub unsafe fn notify_core_online() {
    /// Number of cores which have notified that they are online.
    static CORES_ONLINE: AtomicU32 = AtomicU32::new(0);

    // Transition the core from the launched state to the online state.
    let old_state = CORE_STATES[core!().apic_id().unwrap() as usize]
        .compare_and_swap(CoreState::Launched as u8,
                          CoreState::Online   as u8,
                          Ordering::SeqCst);

    if core!().id == 0 {
        // BSP should be already marked as online (in acpi::initialize).
        assert!(old_state == CoreState::Online as u8, "BSP was not marked as online.");
    } else {
        // Make sure that we have transitioned from the launched state to the online state.
        assert!(old_state == CoreState::Launched as u8,
                "AP became online but it wasn't in launching state before.");
    }

    // This core is now online.
    CORES_ONLINE.fetch_add(1, Ordering::SeqCst);

    // Wait for all cores to become online.
    while CORES_ONLINE.load(Ordering::SeqCst) != total_cores() {
        core::sync::atomic::spin_loop_hint();
    }
}

pub unsafe fn initialize() {
    // Make sure that the ACPI hasn't been initialized yet.
    assert!(TOTAL_CORES.load(Ordering::SeqCst) == 0, "ACPI was already initialized.");

    let tables = {
        // Get the addreses of ACPI system tables.
        let system_tables = core!().boot_block.acpi_tables.lock().clone();

        // Get the preferred ACPI system table.
        let (sdt_addr, sdt_type) = match (system_tables.rsdt, system_tables.xsdt) {
            // XSDT takes priority.
            (_, Some(xsdt)) => (xsdt, SdtType::Xsdt),

            // If there is no XSDT then fallback to RSDT.
            (Some(rsdt), _) => (rsdt, SdtType::Rsdt),

            _ => panic!("Bootloader didn't provide address of any ACPI system table."),
        };

        println!("Using {:?} system table at address 0x{:x}.", sdt_type, sdt_addr);

        // Get all subtables in the system table.
        parse_system_table(PhysAddr(sdt_addr), sdt_type)
    };

    let mut apics = None;

    for (header, payload, payload_size) in tables {
        if &header.signature == b"APIC" {
            apics = Some(parse_madt(payload, payload_size));
        }

        if true {
            if let Ok(signature) = core::str::from_utf8(&header.signature) {
                println!("  {}: 0x{:x}", signature, payload.0);
            }
        }
    }

    let current_apic_id            = core!().apic_id().unwrap();
    let ap_entrypoint: Option<u64> = core!().boot_block.ap_entrypoint.lock().clone();

    if let Some(ap_entrypoint) = ap_entrypoint {
        if let Some(apics) = &apics {
            println!("Launching {} APs. Bootloader AP entrypoint: 0x{:x}.",
                     apics.len() - 1, ap_entrypoint);
        }
    } else {
        println!("WARNING: Bootloader hasn't provivided realmode AP \
                 entrypoint so APs won't be laucnhed.");

        if let Some(apics) = &apics {
            println!("Found {} APICs on the system.", apics.len());
        }

        apics = None;
    }

    let apics = apics.unwrap_or_else(|| {
        if ap_entrypoint.is_some() {
            println!("WARNING: No APIC table was found on the system.");
        }

        // If we haven't found APIC table then just report our APIC ID.

        let mut apics = BTreeSet::new();

        apics.insert(current_apic_id);

        apics
    });

    let core_count = apics.len();

    // Make sure that the total core count doesn't exceed maximum supported value.
    assert!(core_count <= MAX_CORES, "Too many cores on the system.");

    // Save the total number of cores available on the system.
    TOTAL_CORES.store(core_count as u32, Ordering::SeqCst);

    let mut apic = core!().apic.lock();
    let apic     = apic.as_mut().unwrap();

    // Mark our core (BSP) as online.
    set_core_state(current_apic_id, CoreState::Online);

    // Launch all available cores one by one.
    for &apic_id in &apics {
        // Don't IPI ourselves.
        if apic_id == current_apic_id {
            continue;
        }

        let ap_entrypoint = ap_entrypoint.expect("No AP entrypoint.");

        // Calculate the SIPI vector which will cause APs to start
        // execution at the `ap_entrypoint`.
        let sipi_vector = (ap_entrypoint / 0x1000) & 0xff;

        // Make sure that the `ap_entrypoint` is encodable in SIPI vector.
        assert!(sipi_vector * 0x1000 == ap_entrypoint, "AP entrypoint {:x} cannot be encoded.",
                ap_entrypoint);

        let sipi_vector = sipi_vector as u32;

        // Mark the core as launched.
        set_core_state(apic_id, CoreState::Launched);

        // Launch the core by sending INIT-SIPI-SIPI sequence to to it. 
        // Bootloader will perform normal initialization sequence on the launched core
        // and transfer execution to the kernel entrypoint.
        apic.ipi(apic_id, 0x4500);
        apic.ipi(apic_id, 0x4600 | sipi_vector);
        apic.ipi(apic_id, 0x4600 | sipi_vector);

        // Wait for the core to become online. Bootloader is not thread safe so there can
        // be only one launching AP at a time.
        while core_state(apic_id) != CoreState::Online {
            core::sync::atomic::spin_loop_hint();
        }
    }
}

#[derive(Debug)]
enum SdtType {
    Rsdt,
    Xsdt,
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
        .unwrap_or_else(|| panic!("{:?} checksum is invalid.", sdt_type));

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
    assert!(sdt_size % entry_size == 0, "{:?} size is not divisible by entry size.", sdt_type);

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

unsafe fn parse_madt(payload: PhysAddr, payload_size: usize) -> BTreeSet<u32> {
    const APIC_ENABLED:        u32 = 1 << 0;
    const APIC_ONLINE_CAPABLE: u32 = 1 << 1;

    // Get the address of Interrupt Controller Structure. We need to skip
    // local interrupt controller address (4 bytes) and flags (4 bytes).
    let mut ics = PhysAddr(payload.0 + 4 + 4);
    let end     = payload.0 + payload_size as u64;

    let mut apics = BTreeSet::new();

    // Go through every ICS in the MADT.
    loop {
        // Make sure that there is enough space for ICS type and size.
        if ics.0 + 2 > end {
            break;
        }

        let ics_type: u8 = mm::read_phys(PhysAddr(ics.0 + 0));
        let ics_size: u8 = mm::read_phys(PhysAddr(ics.0 + 1));

        // Make sure that the ICS size is valid.
        assert!(ics_size >= 2, "ICS size is invalid.");

        // Make sure that there is enough space for the whole ICS entry.
        if ics.0 + ics_size as u64 > end {
            break;
        }

        // Try to extract APIC information from the ICS.
        let apic = match ics_type {
            0 => {
                // Processor Local APIC

                // Make sure that the size that we expect is correct.
                assert!(ics_size == 8, "Invalid Local APIC entry size.");

                let apic_id: u8  = mm::read_phys_unaligned(PhysAddr(ics.0 + 3));
                let flags:   u32 = mm::read_phys_unaligned(PhysAddr(ics.0 + 4));

                Some((apic_id as u32, flags))
            }
            9 => {
                // Processor Local x2APIC

                // Make sure that the size that we expect is correct.
                assert!(ics_size == 16, "Invalid Local x2APIC entry size.");

                let apic_id: u32 = mm::read_phys_unaligned(PhysAddr(ics.0 + 4));
                let flags:   u32 = mm::read_phys_unaligned(PhysAddr(ics.0 + 8));

                Some((apic_id, flags))
            }
            _ => None,
        };

        if let Some((apic_id, flags)) = apic {
            // We only care about APICs which are either enabled or can be enabled by us.
            if flags & APIC_ENABLED != 0 || flags & APIC_ONLINE_CAPABLE != 0 {
                // Make sure that this APIC reported by ICS is unique.
                assert!(apics.insert(apic_id), "Multiple ICSes reported the same APIC ID.");
            }
        }

        // Go to the next ICS entry.
        ics = PhysAddr(ics.0 + ics_size as u64);
    }

    apics
}
