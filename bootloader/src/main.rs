#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler)]

extern crate alloc;

#[macro_use] mod serial;
mod panic;
mod bios;
mod mm;

use boot_block::{BootBlock, KERNEL_PHYSICAL_REGION_BASE, KERNEL_PHYSICAL_REGION_SIZE,
                 KERNEL_STACK_BASE, KERNEL_STACK_SIZE};

use page_table::{PageTable, PageType, VirtAddr, PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE};
use bdd::{BootDiskDescriptor, BootDiskData};
use elfparse::{Elf, Bitness};
use bios::RegisterState;

use core::convert::TryInto;
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

        // Setup proper register state to perform the read.

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
            // We have successfuly read 1 sector from disk. Now copy it to the actual destination.
            buffer.copy_from_slice(&temp_buffer);

            return;
        }

        println!("Retrying disk read...");
    }

    panic!("Failed to read sector from disk at LBA {}.", lba);
}

fn read_kernel(boot_disk_data: *const BootDiskData,
               boot_disk_descriptor: *const BootDiskDescriptor) -> Vec<u8> {
    let (boot_disk_descriptor, boot_disk_data) = unsafe {
        (&*boot_disk_descriptor, &*boot_disk_data)
    };

    // Verify that BDD is valid and get information about kernel from it.

    assert!(boot_disk_descriptor.signature == bdd::SIGNATURE, "BDD signature does not match.");

    let kernel_lba      = boot_disk_descriptor.kernel_lba;
    let kernel_sectors  = boot_disk_descriptor.kernel_sectors;
    let kernel_checksum = boot_disk_descriptor.kernel_checksum;

    // Allocate buffer that will hold kernel ELF image.
    let mut kernel = alloc::vec![0; (kernel_sectors as usize) * 512];

    // Read the kernel.
    for sector in 0..kernel_sectors {
        let buffer = &mut kernel[(sector as usize) * 512..][..512];

        // Read one sector of the kernel to the destination kernel buffer.
        read_sector(boot_disk_data, kernel_lba + sector, buffer);
    }

    // Make sure that we have actually loaded a thing that we expected.
    assert!(bdd::checksum(&kernel) == kernel_checksum,
            "Loaded kernel has invalid checksum.");

    kernel
}

fn prepare_kernel(kernel: Vec<u8>) -> (u64, u64, u32, u32) {
    // Parse the kernel ELF file and make sure that it is 64 bit.
    let elf = Elf::parse(&kernel).expect("Failed to parse kernel ELF file.");

    assert!(elf.bitness() == Bitness::Bits64, "Loaded kernel is not 64 bit.");

    // WARNING: We can't use any normal allocation routines from here because free memory list
    // is locked and we would cause a deadlock.
    let mut phys_mem = BOOT_BLOCK.free_memory.lock();
    let mut phys_mem = mm::PhysicalMemory(phys_mem.as_mut().unwrap());

    let mut kernel_page_table = PageTable::new(&mut phys_mem)
        .expect("Failed to allocate kernel page table.");

    let mut trampoline_page_table = PageTable::new(&mut phys_mem)
        .expect("Failed to allocate trampoline page table.");

    elf.for_each_segment(|segment| {
        // Skip non-loadable segments.
        if !segment.load {
            return;
        }

        // Page table `map_init` function requires both address and size to be page aligned.
        // Segments in ELF file are often unaligned.

        // Align virtual address down.
        let virt_addr = VirtAddr(segment.virt_addr & !0xfff);

        // Align virtual size up.
        let virt_size = (segment.virt_size + 0xfff) & !0xfff;

        let offset_add = virt_addr.0 - segment.virt_addr;

        // Map the segment with correct permissions using standard 4K pages.
        // If some segments overlap, this routine will return an error.
        kernel_page_table.map_init(&mut phys_mem, virt_addr, PageType::Page4K, virt_size,
                                   segment.write, segment.execute,
                                   Some(|offset| {
                                       segment.bytes.get((offset + offset_add) as usize)
                                           .copied().unwrap_or(0)
                                   }))
            .expect("Failed to map kernel segment.");
    });

    for phys_addr in (0..1024 * 1024).step_by(4096) {
        unsafe {
            for &virt_addr in &[VirtAddr(phys_addr),
                                VirtAddr(phys_addr + KERNEL_PHYSICAL_REGION_BASE)] {
                trampoline_page_table.map_raw(&mut phys_mem, virt_addr, PageType::Page4K,
                                              phys_addr | PAGE_WRITE | PAGE_PRESENT, true, false)
                    .expect("Failed to map physical region in the trampoline page table.");
            }
        }
    }

    {
        let page_type = PageType::Page1G;
        let page_size = page_type as u64;
        let page_mask = page_size - 1;
        let large     = page_type != PageType::Page4K;

        assert!(KERNEL_PHYSICAL_REGION_BASE & page_mask == 0,
                "KERNEL_PHYSICAL_REGION_BASE is not aligned.");

        assert!(KERNEL_PHYSICAL_REGION_SIZE & page_mask == 0,
                "KERNEL_PHYSICAL_REGION_SIZE is not aligned.");

        for phys_addr in (0..KERNEL_PHYSICAL_REGION_SIZE).step_by(page_size as usize) {
            let virt_addr = VirtAddr(KERNEL_PHYSICAL_REGION_BASE + phys_addr);

            // We can't set NX here because we will execute this memory in enter_kernel.
            let mut raw = phys_addr | PAGE_PRESENT | PAGE_WRITE;

            if large {
                raw |= PAGE_SIZE;
            }

            unsafe {
                kernel_page_table.map_raw(&mut phys_mem, virt_addr, page_type, raw, true, false)
                    .expect("Failed to map physical region in the kernel page table.");
            }
        }
    }

    let stack = KERNEL_STACK_BASE;

    kernel_page_table.map(&mut phys_mem, VirtAddr(stack), PageType::Page4K, KERNEL_STACK_SIZE,
                          true, false)
        .expect("Failed to map kernel stack.");

    let kernel_cr3:     u32 = kernel_page_table.table().0.try_into().unwrap();
    let trampoline_cr3: u32 = trampoline_page_table.table().0.try_into().unwrap();

    println!("Kernel base is {:x}", elf.base_address());
    println!("Kernel entrypoint is {:x}", elf.entrypoint());
    println!("Kernel stack base is {:x}", stack);

    (elf.entrypoint(), stack + KERNEL_STACK_SIZE, kernel_cr3, trampoline_cr3)
}

#[no_mangle]
extern "C" fn _start(boot_disk_data: *const BootDiskData,
                     boot_disk_descriptor: *const BootDiskDescriptor) -> ! {
    // Initialize crucial bootloader components.
    unsafe {
        serial::initialize();
        mm::initialize();
    }

    // Read the kernel from disk and prepare it for execution.
    let kernel = read_kernel(boot_disk_data, boot_disk_descriptor);
    let (entrypoint, stack, kernel_cr3, trampoline_cr3) = prepare_kernel(kernel);

    extern "C" {
        fn enter_kernel(entrypoint: u64, stack: u64, boot_block: u64, kernel_cr3: u32,
                        trampoline_cr3: u32, physical_region: u64) -> !;
    }

    // Switch to the 64 bit kernel!

    println!("Entering kernel!");

    unsafe {
        enter_kernel(entrypoint, stack, &BOOT_BLOCK as *const _ as u64, kernel_cr3,
                     trampoline_cr3, KERNEL_PHYSICAL_REGION_BASE);
    }
}
