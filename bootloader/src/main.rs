#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler)]

extern crate alloc;

#[macro_use] mod serial;
mod panic;
mod bios;
mod mm;

use bdd::{BootDiskDescriptor, BootDiskData};
use elfparse::{Elf, Bitness};
use boot_block::BootBlock;
use bios::RegisterState;
use alloc::vec::Vec;

pub static BOOT_BLOCK: BootBlock = BootBlock::new();

fn read_sector(boot_disk_data: &BootDiskData, lba: u32, buffer: &mut [u8]) {
    // Make a temporary buffer which is on the stack in low memory address.
    let mut temp_buffer = [0u8; 512];

    // Make sure that the temporary buffer is accessible for BIOS.
    assert!((temp_buffer.as_ptr() as usize).checked_add(temp_buffer.len()).unwrap() < 0x10000,
             "Temporary buffer for reading sectors is inaccesible for BIOS.");

    for tries in 0..5 {
        // If we have failed before, restart boot disk system.
        if tries > 0 {
            let mut regs = RegisterState {
                eax: 0,
                edx: boot_disk_data.disk_number as u32,
                ..Default::default()
            };

            unsafe { bios::interrupt(0x13, &mut regs); }
            
            assert!(regs.eflags & 1 == 0, "Reseting boot disk system failed.");
        }

        // Convert LBA to CHS using drive geometry from BIOS.
        let cylinder = lba / boot_disk_data.sectors_per_cylinder;
        let head     = (lba / boot_disk_data.sectors_per_track) % 
                        boot_disk_data.heads_per_cylinder;
        let sector   = lba % boot_disk_data.sectors_per_track + 1;

        let al: u8 = 1;
        let ah: u8 = 2;

        let cl: u8 = (sector as u8) | ((cylinder >> 2) & 0xc0) as u8;
        let ch: u8 = cylinder as u8;

        let dl: u8 = boot_disk_data.disk_number;
        let dh: u8 = head as u8;

        // Ask BIOS to read one sector.
        let mut regs = RegisterState {
            eax: ((ah as u32) << 8) | ((al as u32) << 0),
            ecx: ((ch as u32) << 8) | ((cl as u32) << 0),
            edx: ((dh as u32) << 8) | ((dl as u32) << 0),
            ebx: temp_buffer.as_mut_ptr() as u32,
            ..Default::default()
        };

        unsafe { bios::interrupt(0x13, &mut regs); }

        if regs.eax & 0xff == 1 && regs.eflags & 1 == 0 {
            // We successfuly read 1 sector from disk. Now copy it to the actual destination.
            buffer.copy_from_slice(&temp_buffer);

            return;
        }

        println!("Retrying...");
    }

    panic!("Failed to read sector from disk at LBA {}.", lba);
}

fn read_kernel(boot_disk_data: *const BootDiskData,
               boot_disk_descriptor: *const BootDiskDescriptor) -> Vec<u8> {
    let (boot_disk_descriptor, boot_disk_data) = unsafe {
        (&*boot_disk_descriptor, &*boot_disk_data)
    };

    assert!(boot_disk_descriptor.signature == bdd::SIGNATURE, "BDD signature does not match.");

    let kernel_lba      = boot_disk_descriptor.kernel_lba;
    let kernel_sectors  = boot_disk_descriptor.kernel_sectors;
    let kernel_checksum = boot_disk_descriptor.kernel_checksum;

    let mut kernel = alloc::vec![0; (kernel_sectors as usize) * 512];

    for sector in 0..kernel_sectors {
        let buffer = &mut kernel[(sector as usize) * 512..][..512];
        read_sector(boot_disk_data, kernel_lba + sector, buffer);
    }

    assert!(bdd::checksum(&kernel) == kernel_checksum,
            "Loaded kernel has invalid checksum.");

    kernel
}

fn load_kernel(kernel: &[u8]) {
    let elf = Elf::parse(kernel).expect("Failed to parse kernel ELF file.");

    assert!(elf.bitness() == Bitness::Bits64, "Loaded kernel is not 64 bit.");

    elf.for_each_segment(|_segment| {
    });
}

#[no_mangle]
extern "C" fn _start(boot_disk_data: *const BootDiskData,
                     boot_disk_descriptor: *const BootDiskDescriptor) -> ! {
    unsafe {
        serial::initialize();
        mm::initialize();
    }

    println!("Bootloader loaded! Reading kernel...");

    let kernel = read_kernel(boot_disk_data, boot_disk_descriptor);

    println!("Kernel successfuly read!");

    load_kernel(&kernel);

    cpu::halt();
}
