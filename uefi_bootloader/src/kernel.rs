use core::convert::TryInto;

use crate::{BOOT_BLOCK, ap_entrypoint, binaries, mm};
use mm::{BootPhysicalMemory, PhysicalMemory};
use ap_entrypoint::APEntrypoint;

use boot_block::{KERNEL_PHYSICAL_REGION_BASE, KERNEL_PHYSICAL_REGION_SIZE,
                 KERNEL_STACK_BASE, KERNEL_STACK_SIZE, KERNEL_STACK_PADDING};

use page_table::{PageTable, PageType, VirtAddr, PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE};
use elfparse::{Elf, Bitness, SegmentType, Machine};
use lock::Lock;

#[derive(Copy, Clone)]
struct KernelEntryData {
    entrypoint:     u64,
    kernel_cr3:     u64,
    trampoline_cr3: u64,
    trampoline_rsp: u64,
    gdt:            u64,
}

/// Data required to enter the kernel. If it is `None` then kernel wasn't loaded yet.
static KERNEL_ENTRY_DATA: Lock<Option<KernelEntryData>> = Lock::new(None);

/// Address of the next stack used to enter the kernel. Each CPU takes address from here
/// and advances the value.
static NEXT_STACK_ADDRESS: Lock<u64> = Lock::new(KERNEL_STACK_BASE);

/// We map 4GB of physical memory for trampoline page table. This means that bootloader can
/// only touch first 4GB of memory when launching APs.
const TRAMPOLINE_PHYSICAL_REGION_SIZE: u64 = 4 * 1024 * 1024 * 1024;

/// Creates a unique kernel stack required to enter the kernel. Returns stack end, not stack base.
fn create_kernel_stack() -> u64 {
    // It is possible that the kernel uses free memory memory list or page tables too.
    // This is fine as everything is locked.

    let mut page_table = BOOT_BLOCK.page_table.lock();
    let page_table     = page_table.as_mut().unwrap();

    let mut next_stack_address = NEXT_STACK_ADDRESS.lock();

    // Get a unique stack address.
    let stack = VirtAddr(*next_stack_address);

    // Map the stack to the kernel address space.
    // It is possible that this page table contains some physical addresses > 4GB that we cannot
    // access. This is fine, as this area of virtual memory is used only by the bootloader
    // so it won't contain these addesses.
    page_table.map(&mut PhysicalMemory, stack, PageType::Page4K, KERNEL_STACK_SIZE,
                   true, false, false)
        .expect("Failed to map kernel stack.");

    // Update stack address which will be used by the next AP.
    *next_stack_address += KERNEL_STACK_SIZE + KERNEL_STACK_PADDING;

    stack.0 + KERNEL_STACK_SIZE
}

fn create_trampoline_page_table(page_type: PageType) -> PageTable {
    /// Map `phys_addr` at virtual address `phys_addr` and virtual address
    /// `phys_addr` + `KERNEL_PHYSICAL_REGION_BASE`. This mapping will be both
    /// writable and executable.
    fn identity_map(page_table: &mut PageTable, phys_addr: u64, page_type: PageType) {
        let page_mask = page_type as u64 - 1;

        assert!(phys_addr & page_mask == 0, "Cannot map unaligned \
                physical address {:x}.", phys_addr);

        for &virt_addr in &[VirtAddr(phys_addr),
                            VirtAddr(phys_addr + KERNEL_PHYSICAL_REGION_BASE)] {
            assert!(virt_addr.0 & page_mask == 0, "Cannot map unaligned \
                    virtual address address {:x}.", virt_addr.0);

            let mut backing = phys_addr | PAGE_WRITE | PAGE_PRESENT;

            // Set PAGE_SIZE bit if we aren't using standard 4K pages.
            if page_type != PageType::Page4K {
                backing |= PAGE_SIZE;
            }

            unsafe {
                page_table.map_raw(&mut BootPhysicalMemory, virt_addr, page_type, backing,
                                   true, false)
                    .expect("Failed to map memory to the trampoline page table.");
            }
        }
    }

    let page_size = page_type as u64;
    let page_mask = page_size - 1;

    // Make sure that trampoline physcial region size is properly aligned.
    assert!(TRAMPOLINE_PHYSICAL_REGION_SIZE & page_mask == 0,
            "TRAMPOLINE_PHYSICAL_REGION_SIZE is not aligned.");

    // Make sure that we agree with memory manager.
    assert!(mm::MAX_ADDRESS + 1 == TRAMPOLINE_PHYSICAL_REGION_SIZE as usize,
            "Trampoline PT size doesn't match with MM max address.");

    assert!(KERNEL_PHYSICAL_REGION_SIZE >= TRAMPOLINE_PHYSICAL_REGION_SIZE,
            "Trampoline PR doesn't fit in kernel PR.");

    // Allocate a page table that will be used when transitioning to the kernel.
    // We should use `BootPhysicalMemory` with it because it won't be needed after
    // finishing boot process.
    let mut trampoline_page_table = PageTable::new(&mut BootPhysicalMemory)
        .expect("Failed to allocate trampoline page table.");

    // Identity map first `TRAMPOLINE_PHYSICAL_REGION_SIZE` bytes of physical memory.
    for phys_addr in (0..TRAMPOLINE_PHYSICAL_REGION_SIZE).step_by(page_size as usize) {
        identity_map(&mut trampoline_page_table, phys_addr, page_type);
    }

    let bootloader = mm::bootloader_image();

    // Map in bootloader image to the trampoline page table. (As it can be in > 4GB memory.)
    for offset in (0..bootloader.size).step_by(4096) {
        let phys_addr   = (bootloader.base + offset) as u64;
        let max_address = mm::MAX_ADDRESS as u64;

        // Make sure that bootloader will be mapped in the kernel physical region.
        assert!(phys_addr + 4095 < KERNEL_PHYSICAL_REGION_SIZE,
                "Bootloader won't be mapped in the kernel physical region.");

        // If this page is < 4GB it is already mapped in.
        if phys_addr <= max_address {
            continue;
        }

        // Map this page using small 4K pages.
        identity_map(&mut trampoline_page_table, phys_addr, PageType::Page4K);
    }

    trampoline_page_table
}

fn create_kernel_page_table(kernel: &Elf, page_type: PageType) -> PageTable {
    // Allocate a page table that will be used by the kernel.
    let mut kernel_page_table = PageTable::new(&mut PhysicalMemory)
        .expect("Failed to allocate kernel page table.");

    // Map kernel to the virtual memory.
    kernel.segments(|segment| {
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
        kernel_page_table.map_init(
            &mut PhysicalMemory,
            virt_addr,
            PageType::Page4K,
            virt_size,
            segment.write,
            segment.execute,
            false,
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
            })
        ).expect("Failed to map kernel segment.");
    });

    // Create linear physical memory map used by the kernel.
    let page_size = page_type as u64;
    let page_mask = page_size - 1;

    // Make sure physical region address and size are properly aligned for used page type.
    assert!(KERNEL_PHYSICAL_REGION_BASE & page_mask == 0,
            "KERNEL_PHYSICAL_REGION_BASE is not aligned.");
    assert!(KERNEL_PHYSICAL_REGION_SIZE & page_mask == 0,
            "KERNEL_PHYSICAL_REGION_SIZE is not aligned.");

    // Setup kernel physical memory map.
    for phys_addr in (0..KERNEL_PHYSICAL_REGION_SIZE).step_by(page_size as usize) {
        // Map current `phys_addr` at virtual address  `phys_addr` + `KERNEL_PHYSICAL_REGION_BASE`.
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
            kernel_page_table.map_raw(&mut PhysicalMemory, virt_addr, page_type, raw, true, false)
                .expect("Failed to map physical region in the kernel page table.");
        }
    }

    // Inform kernel about what page size we used for physical memory map.
    *BOOT_BLOCK.physical_map_page_size.lock() = Some(page_size.try_into().unwrap());

    kernel_page_table
}

fn load_kernel() -> KernelEntryData {
    // To avoid memory fragmentation:
    // Allocate all boot memory first, then allocate kernel memory.

    // AP entrypoint needs to be in low memory so we must create it before doing any other
    // allocations.
    let mut ap_entrypoint = unsafe { APEntrypoint::new() };

    // Parse the kernel ELF file and make sure that it is 64 bit.
    let kernel = Elf::parse(&binaries::KERNEL).expect("Failed to parse kernel ELF file.");
    assert!(kernel.bitness() == Bitness::Bits64, "Loaded kernel is not 64 bit.");
    assert!(kernel.machine() == Machine::Amd64, "Loaded kernel is AMD64 binary.");

    let features = cpu::get_features();

    // Determine max supported page size.
    let max_page_type = if features.page1g {
        PageType::Page1G
    } else if features.page2m {
        PageType::Page2M
    } else {
        panic!("System doesn't support 2M or 1G pages.");
    };

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

    // Create a page table that will be used when transitioning to the kernel.
    let mut trampoline_page_table = create_trampoline_page_table(max_page_type);

    let gdt = mm::allocate_boot_memory(4096, 8)
        .expect("Failed to allocate GDT.");

    // It is possible that current stack is not mapped in trampoline page tables.
    // To hande this we need to create a small trampoline stack.
    let trampoline_stack = mm::allocate_boot_memory(4096, 8)
        .expect("Failed to allocate trampoline stack.");
    let trampoline_rsp = trampoline_stack as u64 + 4096;

    // All boot memory is allocated, now allocate kernel memory.

    // Create a page table that will be used by the kernel. It will already contain a
    // mapped in kernel.
    let mut kernel_page_table = create_kernel_page_table(&kernel, max_page_type);

    // Get bases of both page tables.
    let kernel_cr3     = kernel_page_table.table().0;
    let trampoline_cr3 = trampoline_page_table.table().0;

    unsafe {
        ap_entrypoint.finalize_and_register(trampoline_cr3);
    }

    println!("Kernel base is 0x{:x}.", kernel.base_address());
    println!("Kernel entrypoint is 0x{:x}.", kernel.entrypoint());

    let entry_data = KernelEntryData {
        entrypoint: kernel.entrypoint(),
        gdt:        gdt as u64,
        kernel_cr3,
        trampoline_cr3,
        trampoline_rsp,
    };

    // Cache entry data so APs can use them later to enter the kernel.
    *KERNEL_ENTRY_DATA.lock() = Some(entry_data);

    // Cache page tables which will be used by all APs.
    *BOOT_BLOCK.page_table.lock() = Some(kernel_page_table);

    entry_data
}

pub unsafe fn enter(boot_tsc: u64) -> ! {
    let entry_data: Option<KernelEntryData> = *KERNEL_ENTRY_DATA.lock();
    let entry_data = entry_data.unwrap_or_else(|| {
        load_kernel()
    });

    let rsp = create_kernel_stack();

    extern "C" {
        fn enter_kernel(entrypoint: u64, rsp: u64, boot_block: u64, kernel_cr3: u64,
                        trampoline_cr3: u64, physical_region: u64, gdt: u64,
                        trampoline_rsp: u64, boot_tsc: u64) -> !;
    }
    
    enter_kernel(entry_data.entrypoint, rsp, &BOOT_BLOCK as *const _ as u64,
                 entry_data.kernel_cr3, entry_data.trampoline_cr3,
                 KERNEL_PHYSICAL_REGION_BASE, entry_data.gdt,
                 entry_data.trampoline_rsp, boot_tsc);
}
