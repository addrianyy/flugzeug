use core::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use alloc::collections::BTreeSet;

use page_table::PhysAddr;
use crate::{mm, panic};

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

    /// This core is halted forever.
    Halted = 4,
}

impl From<u8> for CoreState {
    /// Convert a raw `u8` into an `CoreState`.
    fn from(value: u8) -> CoreState {
        match value {
            1 => CoreState::Online,
            2 => CoreState::Launched,
            3 => CoreState::None,
            4 => CoreState::Halted,
            _ => panic!("Invalid CoreState from `u8`."),
        }
    }
}

pub unsafe fn set_core_state(apic_id: u32, state: CoreState) {
    CORE_STATES[apic_id as usize].store(state as u8, Ordering::SeqCst);
}

pub fn core_state(apic_id: u32) -> CoreState {
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

    let apic_id = core!().apic_id().unwrap();

    // Transition the core from the launched state to the online state.
    let old_state = CORE_STATES[apic_id as usize]
        .compare_exchange(CoreState::Launched as u8,
                          CoreState::Online   as u8,
                          Ordering::SeqCst,
                          Ordering::SeqCst);

    if core!().id == 0 {
        // BSP should be already marked as online (in acpi::initialize).
        assert!(old_state == Err(CoreState::Online as u8), "BSP was not marked as online.");
    } else {
        // Make sure that we have transitioned from the launched state to the online state.
        assert!(old_state == Ok(CoreState::Launched as u8),
                "AP became online but it wasn't in launching state before.");
    }

    // If we were launching we may have missed an NMI. Halt the execution if kernel is panicking.
    if panic::is_panicking() {
        panic::halt();
    }

    // This core is now online.
    CORES_ONLINE.fetch_add(1, Ordering::SeqCst);

    // Wait for all cores to become online.
    while CORES_ONLINE.load(Ordering::SeqCst) != total_cores() {
        core::sync::atomic::spin_loop_hint();
    }
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

pub unsafe fn initialize() {
    let mut apics = crate::acpi::get_only_acpi_table("APIC").map(|(payload, payload_size)| {
        parse_madt(payload, payload_size)
    });

    let current_apic_id            = core!().apic_id().unwrap();
    let ap_entrypoint: Option<u64> = *core!().boot_block.ap_entrypoint.lock();

    if let Some(ap_entrypoint) = ap_entrypoint {
        if let Some(apics) = &apics {
            println!("Launching {} APs. Bootloader AP entrypoint: 0x{:x}.",
                     apics.len() - 1, ap_entrypoint);
        }
    } else {
        color_println!(0xffff00, "WARNING: Bootloader hasn't provivided realmode AP \
                                  entrypoint so APs won't be laucnhed.");

        if let Some(apics) = &apics {
            println!("Found {} APICs on the system.", apics.len());
        }

        apics = None;
    }

    let apics = apics.unwrap_or_else(|| {
        if ap_entrypoint.is_some() {
            color_println!(0xffff00, "WARNING: No APIC table was found on the system.");
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
        apic.ipi(apic_id, (1 << 14) | (0b101 << 8));
        apic.ipi(apic_id, (1 << 14) | (0b110 << 8) | sipi_vector);
        apic.ipi(apic_id, (1 << 14) | (0b110 << 8) | sipi_vector);

        // Wait for the core to become online. Bootloader is not thread safe so there can
        // be only one launching AP at a time.
        //
        // If core panics while executing in kernel it will print panic message and we will spin
        // here forever.
        while core_state(apic_id) != CoreState::Online {
            core::sync::atomic::spin_loop_hint();
        }
    }
}
