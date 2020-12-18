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
                 KERNEL_STACK_BASE, KERNEL_STACK_SIZE, KERNEL_STACK_PADDING};

use page_table::{PageTable, PageType, VirtAddr, PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE};
use bdd::{BootDiskDescriptor, BootDiskData};
use elfparse::{Elf, Bitness, SegmentType, Machine};
use bios::RegisterState;
use mm::PhysicalMemory;
use lock::Lock;

// Bootloader is not thread safe. There can be only one instance of it running at a time.
// Kernel launches cores one by one to make sure that this is indeed what happens.

/// Boot block is a shared data structure between kernel and bootloader. It must have
/// exactly the same shape in 32 bit and 64 bit mode. It allows for concurrent memory
/// allocation and modification and serial port interface.
pub static BOOT_BLOCK: BootBlock = BootBlock::new();

/// Data required to enter the kernel. If it is `None` then kernel wasn't loaded
/// from disk yet.
static KERNEL_ENTRY_DATA: Lock<Option<KernelEntryData>> = Lock::new(None);

/// Address of the next stack used to enter the kernel. Each CPU takes address from here
/// and advances the value. There is no 64 bit atomic value in 32 bit mode so `Lock` is used.
static NEXT_STACK_ADDRESS: Lock<u64> = Lock::new(KERNEL_STACK_BASE);

#[derive(Copy, Clone)]
struct KernelEntryData {
    entrypoint:     u64,
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

/// Creates a unique kernel stack required for entering the kernel.
fn create_kernel_stack() -> u64 {
    // It is possible that the kernel uses free memory memory list or page tables too.
    // This is fine as everything is locked.

    let mut page_table = BOOT_BLOCK.page_table.lock();
    let page_table     = page_table.as_mut().unwrap();

    let mut next_stack_address = NEXT_STACK_ADDRESS.lock();

    // Get a unique stack address.
    let stack = VirtAddr(*next_stack_address);

    // Map the stack to the kernel address space.
    page_table.map(&mut PhysicalMemory, stack, PageType::Page4K, KERNEL_STACK_SIZE, true, false)
        .expect("Failed to map kernel stack.");

    // Update stack address which will be used by the next AP.
    *next_stack_address += KERNEL_STACK_SIZE + KERNEL_STACK_PADDING;

    stack.0
}

/// Allocates a unique stack and gets all data required to enter the kernel.
/// If kernel isn't already in memory, it will be read from disk and mapped.
fn setup_kernel(boot_disk_data: &BootDiskData,
                boot_disk_descriptor: &BootDiskDescriptor) -> (KernelEntryData, u64) {
    if let Some(entry_data) = *KERNEL_ENTRY_DATA.lock() {
        // We are currently launching AP and the kernel has been already loaded and mapped.
        // We just need a new stack to enter the kernel.

        // Create a unique stack for this core.
        let rsp = create_kernel_stack() + KERNEL_STACK_SIZE;

        return (entry_data, rsp);
    }

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
    assert!(bdd::checksum(&kernel) == kernel_checksum, "Loaded kernel has invalid checksum.");

    // Parse the kernel ELF file and make sure that it is 64 bit.
    let elf = Elf::parse(&kernel).expect("Failed to parse kernel ELF file.");
    assert!(elf.bitness() == Bitness::Bits64, "Loaded kernel is not 64 bit.");
    assert!(elf.machine() == Machine::Amd64, "Loaded kernel is AMD64 binary.");

    // Allocate a page table that will be used by the kernel.
    let mut kernel_page_table = PageTable::new(&mut PhysicalMemory)
        .expect("Failed to allocate kernel page table.");

    // Allocate a page table that will be used when transitioning to the kernel.
    let mut trampoline_page_table = PageTable::new(&mut PhysicalMemory)
        .expect("Failed to allocate trampoline page table.");

    // Map kernel to the virtual memory.
    elf.segments(|segment| {
        // Skip non-loadable segments.
        if segment.seg_type != SegmentType::Load {
            return;
        }

        // Page table `map_init` function requires both address and size to be page aligned, but
        // segments in ELF files are often unaligned.

        // Align virtual address down.
        let virt_addr = VirtAddr(segment.virt_addr & !0xfff);

        // Calculate the number of bytes we have added in front of segment to satisfy alignemnt
        // requirements.
        let front_padding = segment.virt_addr - virt_addr.0;

        // Align virtual size up (accounting for front padding).
        let virt_size = (segment.virt_size + front_padding + 0xfff) & !0xfff;

        // Map the segment with correct permissions using standard 4K pages.
        // If some segments overlap, this routine will return an error.
        kernel_page_table.map_init(&mut PhysicalMemory, virt_addr, PageType::Page4K, virt_size,
                                   segment.write, segment.execute,
                                   Some(|offset: u64| {
                                       // Get a byte for given segment offset. Because we have
                                       // possibly changed segment start address,
                                       // we need to account for that. If offset is part of front
                                       // padding then return 0, otherwise get actual offset by
                                       // subtracting `front_padding`.

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

    assert!(KERNEL_PHYSICAL_REGION_SIZE >= TRAMPOLINE_PHYSICAL_REGION_SIZE);

    // Setup trampoline page table.
    for phys_addr in (0..TRAMPOLINE_PHYSICAL_REGION_SIZE).step_by(4096) {
        // Map current `phys_addr` at virtual address `phys_addr` and virtual address
        // `phys_addr` + `KERNEL_PHYSICAL_REGION_BASE`. All this memory will be both
        // writable and executable.
        for &virt_addr in &[VirtAddr(phys_addr),
                            VirtAddr(phys_addr + KERNEL_PHYSICAL_REGION_BASE)] {
            unsafe {
                trampoline_page_table.map_raw(&mut PhysicalMemory, virt_addr, PageType::Page4K,
                                              phys_addr | PAGE_WRITE | PAGE_PRESENT, true, false)
                    .expect("Failed to map physical region in the trampoline page table.");
            }
        }
    }

    // Create linear physical memory map used by kernel at address.
    {
        let features = cpu::get_features();

        // We will map a lot of memory so use the largest possible page type.
        let page_type = if features.page1g {
            PageType::Page1G
        } else if features.page2m {
            println!("WARNING: CPU doesn't support 1G pages, mapping physical \
                     region may take a while.");

            PageType::Page2M
        } else {
            // Mapping using 4K pages would take too long and would waste too much memory.
            panic!("CPU needs to support at least 2M pages.")
        };

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
            let virt_addr = VirtAddr(phys_addr + KERNEL_PHYSICAL_REGION_BASE);

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
                kernel_page_table.map_raw(&mut PhysicalMemory, virt_addr, page_type, raw,
                                          true, false)
                    .expect("Failed to map physical region in the kernel page table.");
            }
        }
    }

    // Get physical addresses of page tables and make sure they fit in 32 bit integer.
    let kernel_cr3:     u32 = kernel_page_table.table().0.try_into().unwrap();
    let trampoline_cr3: u32 = trampoline_page_table.table().0.try_into().unwrap();

    // Cache page tables which will be used by all APs.
    *BOOT_BLOCK.page_table.lock() = Some(kernel_page_table);

    println!("Kernel base is {:x}", elf.base_address());
    println!("Kernel entrypoint is {:x}", elf.entrypoint());

    let entry_data = KernelEntryData {
        entrypoint: elf.entrypoint(),
        kernel_cr3,
        trampoline_cr3,
    };

    // Cache entry data so APs can use them later to enter the kernel.
    *KERNEL_ENTRY_DATA.lock() = Some(entry_data);

    // Create a unique stack for this core.
    let rsp = create_kernel_stack() + KERNEL_STACK_SIZE;

    (entry_data, rsp)
}

#[no_mangle]
extern "C" fn _start(boot_disk_data: &BootDiskData,
                     boot_disk_descriptor: &BootDiskDescriptor) -> ! {
    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    // Initialize crucial bootloader components. This won't do anything if they
    // were already initialized by other CPU.
    unsafe {
        serial::initialize();
        mm::initialize();
    }

    // Load and map kernel if required. Also allocate a unique stack for this core.
    let (entry_data, rsp) = setup_kernel(boot_disk_data, boot_disk_descriptor);

    extern "C" {
        fn enter_kernel(entrypoint: u64, rsp: u64, boot_block: u64, kernel_cr3: u32,
                        trampoline_cr3: u32, physical_region: u64) -> !;
    }

    // Enter the 64 bit kernel!
    unsafe {
        enter_kernel(entry_data.entrypoint, rsp, &BOOT_BLOCK as *const _ as u64,
                     entry_data.kernel_cr3, entry_data.trampoline_cr3,
                     KERNEL_PHYSICAL_REGION_BASE);
    }
}
