#![no_std]
#![no_main]
#![feature(panic_info_message, alloc_error_handler)]

extern crate alloc;

#[macro_use] mod serial;
mod panic;
mod bios;
mod lock;
mod mm;

use core::convert::TryInto;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use boot_block::{BootBlock, KERNEL_PHYSICAL_REGION_BASE, KERNEL_PHYSICAL_REGION_SIZE,
                 KERNEL_STACK_BASE, KERNEL_STACK_SIZE, KERNEL_STACK_PADDING, AcpiTables};

use acpi::{Rsdp, RsdpExtended};
use page_table::{PageTable, PageType, VirtAddr, PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE};
use bdd::{BootDiskDescriptor, BootDiskData};
use elfparse::{Elf, Bitness, SegmentType, Machine};
use bios::RegisterState;
use mm::PhysicalMemory;
use crate::lock::{Lock, EmptyInterrupts};

// Bootloader is not thread safe. There can be only one instance of it running at a time.
// Kernel launches cores one by one to make sure that this is indeed what happens.

/// Boot block is a shared data structure between kernel and bootloader. It must have
/// exactly the same shape in 32 bit and 64 bit mode. It allows for concurrent memory
/// allocation and modification and serial port interface.
/// It will be moved to the kernel after finishing boot process.
pub static BOOT_BLOCK: BootBlock<EmptyInterrupts> = BootBlock::new();

/// Data required to enter the kernel. If it is `None` then kernel wasn't loaded
/// from disk yet.
static KERNEL_ENTRY_DATA: Lock<Option<KernelEntryData>> = Lock::new(None);

/// Address of the next stack used to enter the kernel. Each CPU takes address from here
/// and advances the value. There is no 64 bit atomic value in 32 bit mode so `Lock` is used.
static NEXT_STACK_ADDRESS: Lock<u64> = Lock::new(KERNEL_STACK_BASE);

static INITIALIZED: AtomicBool = AtomicBool::new(false);
static CORE_ID:     AtomicU32  = AtomicU32::new(0);

#[derive(Copy, Clone)]
struct KernelEntryData {
    entrypoint:     u64,
    kernel_cr3:     u32,
    trampoline_cr3: u32,
}

fn extended_reads_supported() -> bool {
    let ax: u16 = 0x4100;
    let bx: u16 = 0x55aa;
    let dx: u16 = 0x0080;

    let mut regs = RegisterState {
        eax: ax as u32,
        ebx: bx as u32,
        edx: dx as u32,
        ..Default::default()
    };

    unsafe { bios::interrupt(0x13, &mut regs); }

    // The carry flag will be set if extensions are not supported.
    regs.eflags & (1 << 0) == 0
}

fn read_sector(boot_disk_data: &BootDiskData, lba: u32, buffer: &mut [u8]) {
    // Make a temporary buffer which is on the stack in low memory address.
    let mut temp_buffer = [0u8; 512];

    // Make sure that the temporary buffer is accessible for BIOS.
    assert!((temp_buffer.as_ptr() as usize).checked_add(512).unwrap() < 0x10000,
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

fn read_sector_extended(boot_disk_data: &BootDiskData, lba: u32, buffer: &mut [u8]) {
    #[repr(C)]
    struct DiskAddressPacket {
        size:    u8,
        zero:    u8,
        sectors: u16,
        offset:  u16,
        segment: u16,
        lo_lba:  u32,
        hi_lba:  u32,
    }

    #[repr(C, align(16))]
    struct ReadBuffer([u8; 512]);

    // Make sure that disk address packet has expected layout.
    assert!(core::mem::size_of::<DiskAddressPacket>() == 16 &&
            core::mem::align_of::<DiskAddressPacket>() >= 4,
            "Invalid shape of disk address packet.");

    // Make a temporary buffer which is aligned and on the stack in low memory address.
    let mut temp_buffer = ReadBuffer([0u8; 512]);
    let buffer_ptr      = temp_buffer.0.as_mut_ptr() as usize;

    // Make sure that the temporary buffer is accessible for BIOS.
    assert!(buffer_ptr.checked_add(512).unwrap() < 0x10000 && buffer_ptr % 16 == 0,
            "Temporary buffer for reading sectors is inaccesible for BIOS.");

    let mut dap = DiskAddressPacket {
        size:    16,
        zero:    0,
        sectors: 1,
        offset:  buffer_ptr as u16,
        segment: 0,
        lo_lba:  lba,
        hi_lba:  0,
    };

    let dap_ptr = &mut dap as *mut _ as usize;

    // Make sure that the DAP is accessible for BIOS.
    assert!(dap_ptr.checked_add(16).unwrap() < 0x10000, "DAP is inaccesible for BIOS.");

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

        // Setup proper register state to perform the extended read and ask BIOS to
        // read one sector.
        let mut regs = RegisterState {
            eax: 0x4200,
            edx: boot_disk_data.disk_number as u32,
            esi: dap_ptr as u32,
            ..Default::default()
        };

        unsafe { bios::interrupt(0x13, &mut regs); }

        if regs.eflags & 1 == 0 {
            // We have successfuly read 1 sector from disk. Now copy it to the actual destination.
            buffer.copy_from_slice(&temp_buffer.0);

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
    page_table.map(&mut PhysicalMemory, stack, PageType::Page4K, KERNEL_STACK_SIZE,
                   true, false, false)
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

    // Check if extended disk services are available.
    let extended_reads_supported = extended_reads_supported();

    // Read the kernel.
    for sector in 0..kernel_sectors {
        let buffer = &mut kernel[(sector as usize) * 512..][..512];

        // Read one sector of the kernel to the destination kernel buffer.
        // Use extended reads if supported.
        if extended_reads_supported {
            read_sector_extended(boot_disk_data, kernel_lba + sector, buffer);
        } else {
            read_sector(boot_disk_data, kernel_lba + sector, buffer);
        }
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
                                   segment.write, segment.execute, false,
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

        *BOOT_BLOCK.physical_map_page_size.lock() = Some(page_size.try_into().unwrap());

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

    println!("Kernel base is 0x{:x}.", elf.base_address());
    println!("Kernel entrypoint is 0x{:x}.", elf.entrypoint());

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

unsafe fn locate_acpi() {
    use core::ptr::read_unaligned;

    let mut tables = AcpiTables {
        rsdt: None,
        xsdt: None,
    };

    // Get the address of the EBDA from the BDA.
    let ebda = read_unaligned(0x40e as *const u16) as usize;

    // Regions that we need to scan for the RSDP.
    let regions = [
        // First 1K of the EBDA.
        (ebda, ebda + 1024),

        // Constant range specified by ACPI specification.
        (0xe0000, 0xfffff),
    ];

    for &(start, end) in &regions {
        // 16 byte align the start address upwards.
        let start = (start + 0xf) & !0xf;

        // 16 byte align the end address downwards.
        let end = end & !0xf;

        // Go through every 16 byte aligned address in the current region.
        for phys_addr in (start..end).step_by(16) {
            // Read the RSDP structure.
            let rsdp: Rsdp = read_unaligned(phys_addr as *const Rsdp);

            // Make sure that the RSDP signature matches.
            if &rsdp.signature != b"RSD PTR " {
                continue;
            }

            // Get the RSDP raw bytes.
            let raw_bytes = read_unaligned(phys_addr as *const [u8; core::mem::size_of::<Rsdp>()]);

            // Make sure that the RSDP checksum is valid.
            let checksum = raw_bytes.iter().fold(0u8, |acc, v| acc.wrapping_add(*v));
            if  checksum != 0 {
                continue;
            }

            if rsdp.revision > 0 {
                type ExtendedArray = [u8; core::mem::size_of::<RsdpExtended>()];

                // Get the extended RSDP raw bytes.
                let raw_bytes = read_unaligned(phys_addr as *const ExtendedArray);
                let extended  = read_unaligned(phys_addr as *const RsdpExtended);

                // Make sure that the extended RSDP checksum is valid.
                let checksum = raw_bytes.iter().fold(0u8, |acc, v| acc.wrapping_add(*v));
                if  checksum != 0 {
                    continue;
                }

                tables.xsdt = Some(extended.xsdt_addr as u64);
            }

            tables.rsdt = Some(rsdp.rsdt_addr as u64);

            // All checks succedded, we have found the ACPI tables.

            *BOOT_BLOCK.acpi_tables.lock() = tables;

            return;
        }
    }

    panic!("Failed to find ACPI tables on the system.");
}

#[no_mangle]
extern "C" fn _start(boot_disk_data: &BootDiskData,
                     boot_disk_descriptor: &BootDiskDescriptor) -> ! {
    let boot_tsc = unsafe { core::arch::x86::_rdtsc() };

    // Make sure that LLVM data layout isn't broken.
    assert!(core::mem::size_of::<u64>() == 8 && core::mem::align_of::<u64>() == 8,
            "U64 has invalid size/alignment.");

    if !INITIALIZED.load(Ordering::Relaxed) {
        // Initialize crucial bootloader components.
        unsafe {
            serial::initialize();
            bootlib::verify_cpu();
            mm::initialize();

            locate_acpi();
        }

        INITIALIZED.store(true, Ordering::Relaxed);
    } else {
        // If we are running for the second time (or later), increase core ID.
        CORE_ID.fetch_add(1, Ordering::Relaxed);

        bootlib::verify_cpu();
    }

    // Set AP entrypoint so kernel will be able to launch other processors.
    let mut ap_entrypoint = BOOT_BLOCK.ap_entrypoint.lock();
    if ap_entrypoint.is_none() {
        *ap_entrypoint = Some(0x8000);
    }

    drop(ap_entrypoint);

    // Load and map kernel if required. Also allocate a unique stack for this core.
    let (entry_data, rsp) = setup_kernel(boot_disk_data, boot_disk_descriptor);

    extern "C" {
        fn enter_kernel(entrypoint: u64, rsp: u64, boot_block: u64, kernel_cr3: u32,
                        trampoline_cr3: u32, physical_region: u64, boot_tsc: u64) -> !;
    }

    // Enter the 64 bit kernel!
    unsafe {
        enter_kernel(entry_data.entrypoint, rsp, &BOOT_BLOCK as *const _ as u64,
                     entry_data.kernel_cr3, entry_data.trampoline_cr3,
                     KERNEL_PHYSICAL_REGION_BASE, boot_tsc);
    }
}
