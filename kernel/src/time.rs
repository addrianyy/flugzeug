use core::convert::TryInto;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::mm;
use crate::hpet::Hpet;
use crate::lock::Lock;

use page_table::PhysAddr;

static HPET:     Lock<Option<Hpet>> = Lock::new(None);
static TSC_KHZ:  AtomicU64          = AtomicU64::new(0);
static BOOT_TSC: AtomicU64          = AtomicU64::new(0);

#[inline(always)]
#[allow(unused)]
pub fn get() -> u64 {
    get_tsc()
}

#[allow(unused)]
pub fn time_difference(from: u64, to: u64) -> f64 {
    assert!(to > from, "`to` ({}) is earlier than `from` ({}).", to, from);

    let khz   = TSC_KHZ.load(Ordering::Relaxed);
    let delta = to - from;

    delta as f64 / khz as f64 / 1_000.0
}

#[allow(unused)]
pub fn local_uptime() -> f64 {
    time_difference(core!().boot_tsc, get_tsc())
}

#[allow(unused)]
pub fn global_uptime() -> f64 {
    time_difference(BOOT_TSC.load(Ordering::Relaxed), get_tsc())
}

#[allow(unused)]
pub fn uptime_with_firmware() -> f64 {
    time_difference(0, get_tsc())
}

#[inline(always)]
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

unsafe fn initialize_hpet() {
    let mut global_hpet = HPET.lock();

    assert!(global_hpet.is_none(), "HPET was already initialized.");

    let (payload, payload_size) = crate::acpi::get_first_acpi_table("HPET")
        .expect("Failed to find HPET on the system.");

    assert!(payload_size >= core::mem::size_of::<acpi::HpetPayload>(),
            "Invalid HPET payload size {}.", payload_size);

    let payload: acpi::HpetPayload = mm::read_phys_unaligned(payload);

    assert!(payload.address.address_space == 0, "HPET is not memory mapped.");

    *global_hpet = Some(Hpet::new(PhysAddr(payload.address.address)));
}

pub unsafe fn initialize() {
    const CALIBRATION_MS:         u128 = 50;
    const FEMTOSECONDS_IN_SECOND: u128 = 1_000_000_000_000_000;

    initialize_hpet();
    
    let mut hpet = HPET.lock();
    let hpet     = hpet.as_mut().unwrap();

    // Check if CPU supports invariant TSC which we rely on. This isn't hard error as some
    // VMs report that it's not supported and we want to test the kernel on them anyways.
    // Timing on these VMs isn't that bad anyways.
    if !cpu::get_features().invariant_tsc {
        println!("WARNING: Timing may be off because CPU doesn't support invariant TSC.");
    }

    // Convert calibration milliseconds to femtoseconds.
    let calibration_fs = CALIBRATION_MS
        .checked_mul(FEMTOSECONDS_IN_SECOND / 1000)
        .expect("Cannot convert calibration milliseconds to femtoseconds.");

    // Get the number of HPET clocks that correspond to `CALIBRATION_MS` milliseconds.
    let calibration_clocks      = calibration_fs / (hpet.period() as u128);
    let calibration_clocks: u64 = calibration_clocks.try_into()
        .expect("Cannot fit calibration clocks in 64 bit integer.");

    {
        let check_clocks = calibration_clocks.checked_mul(2)
            .expect("Calibration would overflow 64 bit counter.");

        if !hpet.is_64bit() {
            let _check_clocks: u32 = check_clocks.try_into()
                .expect("Calibration would overflow 32 bit counter.");
        }
    }

    hpet.enable();

    let start_counter = hpet.counter();
    let start_tsc     = get_tsc_ordered();

    // Run for about `CALIBRATION_MS` milliseconds.
    while hpet.counter() < start_counter + calibration_clocks {}
    
    let end_counter = hpet.counter();
    let end_tsc     = get_tsc_ordered();

    hpet.disable();

    let counter_delta = end_counter - start_counter;
    let tsc_delta     = end_tsc - start_tsc;

    // Calculate the amount of elapsed femtoseconds.
    let elapsed_fs = (counter_delta as u128).checked_mul(hpet.period() as u128)
        .expect("Failed to fit elapsed femtoseconds in 128 bit integer.");

    // Calculate how much femtoseconds every cycle takes.
    let fs_per_cycle = elapsed_fs.checked_div(tsc_delta as u128)
        .expect("Failed to calculate femtoseconds per cycle.");

    // Calculate the TSC frequency.
    let hz       = FEMTOSECONDS_IN_SECOND / fs_per_cycle;
    let khz: u64 = (hz / 1000).try_into()
        .expect("Failed to fit TSC frequency (KHz) in 64 bit integer.");

    println!("Calculated TSC frequency: {}.{:03} MHz.", khz / 1000, khz % 1000);

    TSC_KHZ.store(khz, Ordering::Relaxed);

    // Take the boot TSC from the BSP.
    BOOT_TSC.store(core!().boot_tsc, Ordering::Relaxed);
}
