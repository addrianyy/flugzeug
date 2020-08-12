#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler)]

extern crate alloc;

#[macro_use] mod serial;
mod panic;
mod bios;
mod mm;

use core::convert::TryInto;

use boot_block::{BootBlock, KERNEL_PHYSICAL_REGION_BASE, KERNEL_PHYSICAL_REGION_SIZE,
                 KERNEL_STACK_BASE, KERNEL_STACK_SIZE};

use page_table::{PageTable, PageType, VirtAddr, PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE};
use bdd::{BootDiskDescriptor, BootDiskData};
use elfparse::{Elf, Bitness};
use bios::RegisterState;

pub static BOOT_BLOCK: BootBlock = BootBlock::new();

struct KernelEntryData {
    entrypoint:     u64,
    stack:          u64,
    kernel_cr3:     u32,
    trampoline_cr3: u32,
}

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

fn setup_kernel(boot_disk_data: &BootDiskData,
                boot_disk_descriptor: &BootDiskDescriptor) -> KernelEntryData {
    // Make sure that the BDD is valid.
    assert!(boot_disk_descriptor.signature == bdd::SIGNATURE, "BDD has invalid signature.");

    // Get information about kernel location on disk from BDD.
    let kernel_lba      = boot_disk_descriptor.kernel_lba;
    let kernel_sectors  = boot_disk_descriptor.kernel_sectors;
    let kernel_checksum = boot_disk_descriptor.kernel_checksum;

    // Allocate a buffer that will hold whole kernel ELF image.
    let mut kernel = alloc::vec![0; (kernel_sectors as usize) * 512];

    // Read the kernel.
    for sector in 0..kernel_sectors {
        let buffer = &mut kernel[(sector as usize) * 512..][..512];

        // Read one sector of the kernel to the destination kernel buffer.
        read_sector(boot_disk_data, kernel_lba + sector, buffer);
    }

    // Make sure that loaded kernel matches our expectations.
    assert!(bdd::checksum(&kernel) == kernel_checksum,
            "Loaded kernel has invalid checksum.");

    // Parse the kernel ELF file and make sure that it is 64 bit.
    let elf = Elf::parse(&kernel).expect("Failed to parse kernel ELF file.");
    assert!(elf.bitness() == Bitness::Bits64, "Loaded kernel is not 64 bit.");

    // WARNING: We can't use any normal allocation routines from here because free memory list
    // is locked and we would cause a deadlock.
    let mut phys_mem = BOOT_BLOCK.free_memory.lock();
    let mut phys_mem = mm::PhysicalMemory(phys_mem.as_mut().unwrap());

    // Allocate page table that will be used by the kernel.
    let mut kernel_page_table = PageTable::new(&mut phys_mem)
        .expect("Failed to allocate kernel page table.");

    // Allocate page table that will be used when transitioning to the kernel.
    let mut trampoline_page_table = PageTable::new(&mut phys_mem)
        .expect("Failed to allocate trampoline page table.");

    // Map kernel to the virtual memory.
    elf.for_each_segment(|segment| {
        // Skip non-loadable segments.
        if !segment.load {
            return;
        }

        // Page table `map_init` function requires both address and size to be page aligned, but
        // segments in ELF files are often unaligned.

        // Align virtual address down.
        let virt_addr = VirtAddr(segment.virt_addr & !0xfff);

        // Align virtual size up.
        let virt_size = (segment.virt_size + 0xfff) & !0xfff;

        let front_padding = segment.virt_addr - virt_addr.0;

        // Map the segment with correct permissions using standard 4K pages.
        // If some segments overlap, this routine will return an error.
        kernel_page_table.map_init(&mut phys_mem, virt_addr, PageType::Page4K, virt_size,
                                   segment.write, segment.execute,
                                   Some(|offset: u64| {
                                       // Get a byte for given segment offset. Because
                                       // we possibly changed segment start address,
                                       // we need to account for that.
                                       // If offset is part of front padding then return 0,
                                       // otherwise get actual offset by subtracting
                                       // `front_padding`.

                                       let offset = match offset.checked_sub(front_padding) {
                                           Some(offset) => offset,
                                           None         => return 0,
                                       };

                                       // Get a byte. If the memory is not initialized then
                                       // initialize it to 0.
                                       segment.bytes.get(offset as usize).copied().unwrap_or(0)
                                   }))
            .expect("Failed to map kernel segment.");
    });

    // Bootloader uses identity physical memory map, but kernel will use linear physical
    // memory map that starts at `KERNEL_PHYSICAL_REGION_BASE`.
    // To be able to transition to the kernel we need to a allocate trampoline page table that
    // will map physical address 0 to virtual address 0 (like in bootloader) and
    // physical address 0 to virtual address `KERNEL_PHYSICAL_REGION_SIZE` (like in kernel).

    // Transition code will work like this:
    // 1. Bootloader executes `enter_kernel`. Enable long mode and setup paging with trampoline
    //    page table.
    // 2. Jump to the next part of `enter_kernel`, but add `KERNEL_PHYSICAL_REGION_BASE`
    //    to RIP in order to use kernel-valid address.
    // 3. Switch to the actual kernel page tables, switch stack and jump to the kernel.

    // We will only execute bootloader code using trampoline page tables. Bootloader
    // has to be loaded in a low, smaller than 1MB address. Therafore we just need to map
    // 1MB of memory.
    const TRAMPOLINE_PHYSICAL_REGION_SIZE: u64 = 1024 * 1024;

    // Setup trampoline page table.
    for phys_addr in (0..TRAMPOLINE_PHYSICAL_REGION_SIZE).step_by(4096) {
        // Map current `phys_addr` at virtual address `phys_addr` and virtual address
        // `phys_addr` + `KERNEL_PHYSICAL_REGION_BASE`. All this memory will be both
        // writable and executable.
        for &virt_addr in &[VirtAddr(phys_addr),
                            VirtAddr(phys_addr + KERNEL_PHYSICAL_REGION_BASE)] {
            unsafe {
                trampoline_page_table.map_raw(&mut phys_mem, virt_addr, PageType::Page4K,
                                              phys_addr | PAGE_WRITE | PAGE_PRESENT, true, false)
                    .expect("Failed to map physical region in the trampoline page table.");
            }
        }
    }

    // Create linear physical memory map used by kernel at address.
    {
        // We will map a lot of memory, so use the largest possible page type.
        // TODO: Don't use 1G pages blindly, check what is the largest page supported by the CPU.
        let page_type = PageType::Page1G;
        let page_size = page_type as u64;
        let page_mask = page_size - 1;

        // Make sure physical region address and size are properly aligned for used page type.
        assert!(KERNEL_PHYSICAL_REGION_BASE & page_mask == 0,
                "KERNEL_PHYSICAL_REGION_BASE is not aligned.");
        assert!(KERNEL_PHYSICAL_REGION_SIZE & page_mask == 0,
                "KERNEL_PHYSICAL_REGION_SIZE is not aligned.");

        // Setup kernel physical memory map.
        for phys_addr in (0..KERNEL_PHYSICAL_REGION_SIZE).step_by(page_size as usize) {
            // Map current `phys_addr` at virtual address
            // `phys_addr` + `KERNEL_PHYSICAL_REGION_BASE`.
            let virt_addr = VirtAddr(KERNEL_PHYSICAL_REGION_BASE + phys_addr);

            // This physical memory page will be both writable and executable. Unfortunately
            // we can't set NX bit because we will execute some code using this mapping
            // when transitioning from bootloader to the kernel. Kernel should later make these
            // mappings NX.
            let mut raw = phys_addr | PAGE_PRESENT | PAGE_WRITE;

            // Set PAGE_SIZE bit if we aren't using standard 4K pages.
            if page_type != PageType::Page4K {
                raw |= PAGE_SIZE;
            }

            unsafe {
                kernel_page_table.map_raw(&mut phys_mem, virt_addr, page_type, raw, true, false)
                    .expect("Failed to map physical region in the kernel page table.");
            }
        }
    }

    let stack = KERNEL_STACK_BASE;

    // Map kernel stack as writable and non-executable.
    kernel_page_table.map(&mut phys_mem, VirtAddr(stack), PageType::Page4K, KERNEL_STACK_SIZE,
                          true, false)
        .expect("Failed to map kernel stack.");

    // Get physical addresses of page tables and make sure they fit in 32 bit integer.
    let kernel_cr3:     u32 = kernel_page_table.table().0.try_into().unwrap();
    let trampoline_cr3: u32 = trampoline_page_table.table().0.try_into().unwrap();

    println!("Kernel base is {:x}", elf.base_address());
    println!("Kernel entrypoint is {:x}", elf.entrypoint());
    println!("Kernel stack base is {:x}", stack);

    KernelEntryData {
        entrypoint: elf.entrypoint(),
        stack:      stack + KERNEL_STACK_SIZE,
        kernel_cr3,
        trampoline_cr3,
    }
}

#[no_mangle]
extern "C" fn _start(boot_disk_data: &BootDiskData,
                     boot_disk_descriptor: &BootDiskDescriptor) -> ! {
    // Initialize crucial bootloader components.
    unsafe {
        serial::initialize();
        mm::initialize();
    }

    // Load kernel, map it to the virtual memory and allocate stack.
    let entry_data = setup_kernel(boot_disk_data, boot_disk_descriptor);

    extern "C" {
        fn enter_kernel(entrypoint: u64, stack: u64, boot_block: u64, kernel_cr3: u32,
                        trampoline_cr3: u32, physical_region: u64) -> !;
    }

    println!("Entering kernel!");

    // Enter the 64 bit kernel!.
    unsafe {
        enter_kernel(entry_data.entrypoint, entry_data.stack, &BOOT_BLOCK as *const _ as u64,
                     entry_data.kernel_cr3, entry_data.trampoline_cr3,
                     KERNEL_PHYSICAL_REGION_BASE);
    }
}
