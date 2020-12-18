#![no_std]
#![no_main]
#![feature(abi_efiapi, panic_info_message, asm)]

extern crate libc_routines;

#[macro_use] mod serial;
mod panic;
mod efi;
mod mm;

use core::convert::TryInto;

use boot_block::{BootBlock, KERNEL_PHYSICAL_REGION_BASE, KERNEL_PHYSICAL_REGION_SIZE,
                 KERNEL_STACK_BASE, KERNEL_STACK_SIZE, KERNEL_STACK_PADDING};

use page_table::{PageTable, PageType, VirtAddr, PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE};
use elfparse::{Elf, Bitness, SegmentType, Machine};
use mm::{PhysicalMemory, PhysicalMemory32};
use lock::Lock;

// Bootloader is not thread safe. There can be only one instance of it running at a time.
// Kernel launches cores one by one to make sure that this is indeed what happens.

/// Boot block is a shared data structure between kernel and bootloader. It must have
/// exactly the same shape in 32 bit and 64 bit mode. It allows for concurrent memory
/// allocation and modification and serial port interface.
pub static BOOT_BLOCK: BootBlock = BootBlock::new();

/// ELF image of the kernel.
const KERNEL: &[u8] = include_bytes!(env!("FLUGZEUG_KERNEL_PATH"));

/// Realmode AP entrypoint.
const AP_ENTRYPOINT: &[u8] = include_bytes!(env!("FLUGZEUG_AP_ENTRYPOINT_PATH"));

/// Data required to enter the kernel. If it is `None` then kernel wasn't loaded
/// from disk yet.
static KERNEL_ENTRY_DATA: Lock<Option<KernelEntryData>> = Lock::new(None);

/// Address of the next stack used to enter the kernel. Each CPU takes address from here
/// and advances the value. There is no 64 bit atomic value in 32 bit mode so `Lock` is used.
static NEXT_STACK_ADDRESS: Lock<u64> = Lock::new(KERNEL_STACK_BASE);

#[derive(Copy, Clone)]
struct KernelEntryData {
    entrypoint:     u64,
    kernel_cr3:     u64,
    trampoline_cr3: u64,
    gdt:            u64,
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

struct APEntry {
    code_address: usize,
    code_buffer:  &'static mut [u8],
}

impl APEntry {
    unsafe fn new() -> Self {
        if false {
            // Reserve memory to test segment handling in AP entrypoint.
            // Testing only.
            BOOT_BLOCK.free_memory
                .lock()
                .as_mut()
                .unwrap()
                .allocate_limited(0x11000, 0x1000, Some(0x100000 - 1))
                .expect("Failed to allocate testing area.");
        }

        // 16KB of stack. Bootloader needs a lot of stack because it uses `RangeSet`.
        const STACK_SIZE: usize = 16 * 1024;

        let entrypoint_code = AP_ENTRYPOINT;
        let code_size       = (entrypoint_code.len() + 0xfff) & !0xfff;
        let area_size       = code_size + STACK_SIZE;

        assert!(area_size & 0xfff == 0, "AP entrypoint area size is not page aligned.");

        // Allocate AP area in low memory that is accesible by 16 bit code.
        let area_address = BOOT_BLOCK.free_memory
            .lock()
            .as_mut()
            .unwrap()
            .allocate_limited(area_size as u64, 0x1000, Some(0x100000 - 1))
            .expect("Failed to allocate AP entrypoint.");

        let code_address = (area_address as usize) + STACK_SIZE;
        let code_buffer  = core::slice::from_raw_parts_mut(code_address as *mut u8,
                                                           entrypoint_code.len());

        code_buffer.copy_from_slice(entrypoint_code);

        Self {
            code_address,
            code_buffer,
        }
    }

    unsafe fn finalize_and_register(&mut self, trampoline_cr3: u64) {
        let code_buffer = &mut self.code_buffer;

        let code_address:   u32 = self.code_address.try_into().expect("AP entrypoint > 4GB");
        let trampoline_cr3: u32 = trampoline_cr3.try_into().expect("Trampoline CR3 > 4GB");

        // Make sure that AP entrypoint starts with:
        //   mov eax, 0xaabbccdd
        //   jmp skip
        //
        // AP entrypoint will expect us to change 0xaabbccdd to its own address.
        assert!(&code_buffer[..6 + 1] == &[0x66, 0xb8, 0xdd, 0xcc, 0xbb, 0xaa, 0xeb]);

        // Replace imm in `mov` to code base address.
        code_buffer[2..6].copy_from_slice(&code_address.to_le_bytes());

        // Calculate jump target relative to `code_address`.
        let jmp_target_offset = (code_buffer[6 + 1] + 6 + 2) as usize;

        // 6 bytes for mov and 2 bytes for jmp.
        let mut current_offset = 6 + 2;
        let mut current_buffer = &mut code_buffer[current_offset..];

        macro_rules! write {
            ($value: expr) => {{
                let value: u64 = $value;
                let bytes      = value.to_le_bytes();

                current_buffer[..bytes.len()].copy_from_slice(&bytes);
                current_offset += bytes.len();

                #[allow(unused)]
                {
                    current_buffer = &mut current_buffer[bytes.len()..];
                }
            }}
        }

        // trampoline_cr3:        dq 0
        write!(trampoline_cr3 as u64);

        // bootloader_entrypoint: dq 0
        write!(efi_main as *const () as u64);

        // Make sure we have written expected amount of bytes.
        assert_eq!(current_offset, jmp_target_offset,
                   "Data area in AP entrypoint was corrupted.");

        // Set AP entrypoint address so it will be used by the kernel.
        *BOOT_BLOCK.ap_entrypoint.lock() = Some(code_address as u64);
    }
}

/// Allocates a unique stack and gets all data required to enter the kernel.
/// If kernel isn't already in memory, it will be read from disk and mapped.
fn setup_kernel() -> (KernelEntryData, u64) {
    if let Some(entry_data) = *KERNEL_ENTRY_DATA.lock() {
        // We are currently launching AP and the kernel has been already loaded and mapped.
        // We just need a new stack to enter the kernel.

        // Create a unique stack for this core.
        let rsp = create_kernel_stack() + KERNEL_STACK_SIZE;

        return (entry_data, rsp);
    }

    // AP entrypoint needs to be in low memory so we must create it before doing any other
    // allocations.
    let mut ap_entrypoint = unsafe { APEntry::new() };

    // Parse the kernel ELF file and make sure that it is 64 bit.
    let elf = Elf::parse(&KERNEL).expect("Failed to parse kernel ELF file.");
    assert!(elf.bitness() == Bitness::Bits64, "Loaded kernel is not 64 bit.");
    assert!(elf.machine() == Machine::Amd64, "Loaded kernel is AMD64 binary.");

    // Allocate a page table that will be used when transitioning to the kernel.
    // It will be also used by AP entrypoint so it needs to be in address < 4GB.
    let mut trampoline_page_table = PageTable::new(&mut PhysicalMemory32)
        .expect("Failed to allocate trampoline page table.");

    // Allocate a page table that will be used by the kernel.
    let mut kernel_page_table = PageTable::new(&mut PhysicalMemory)
        .expect("Failed to allocate kernel page table.");

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

    // We hope that 4GB mapping is enough. TODO: Fix this.
    const TRAMPOLINE_PHYSICAL_REGION_SIZE: u64 = 4 * 1024 * 1024 * 1024;

    let features = cpu::get_features();

    assert!(KERNEL_PHYSICAL_REGION_SIZE >= TRAMPOLINE_PHYSICAL_REGION_SIZE);
    assert!(features.page2m, "CPU needs to support at least 2M pages.");

    // Setup trampoline page table.
    for phys_addr in (0..TRAMPOLINE_PHYSICAL_REGION_SIZE).step_by(2 * 1024 * 1024) {
        // Map current `phys_addr` at virtual address `phys_addr` and virtual address
        // `phys_addr` + `KERNEL_PHYSICAL_REGION_BASE`. All this memory will be both
        // writable and executable.
        for &virt_addr in &[VirtAddr(phys_addr),
                            VirtAddr(phys_addr + KERNEL_PHYSICAL_REGION_BASE)] {
            unsafe {
                trampoline_page_table.map_raw(&mut PhysicalMemory, virt_addr, PageType::Page2M,
                                              phys_addr | PAGE_WRITE | PAGE_PRESENT | PAGE_SIZE,
                                              true, false)
                    .expect("Failed to map physical region in the trampoline page table.");
            }
        }
    }

    // Create linear physical memory map used by kernel at address.
    {
        // We will map a lot of memory so use the largest possible page type.
        let page_type = if features.page1g {
            PageType::Page1G
        } else {
            println!("WARNING: CPU doesn't support 1G pages, mapping physical \
                     region may take a while.");

            PageType::Page2M
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

    let kernel_cr3     = kernel_page_table.table().0;
    let trampoline_cr3 = trampoline_page_table.table().0;
    let gdt            = BOOT_BLOCK.free_memory.lock().as_mut()
        .unwrap()
        .allocate(4096, 8)
        .expect("Failed to allocate GDT.");

    // Cache page tables which will be used by all APs.
    *BOOT_BLOCK.page_table.lock() = Some(kernel_page_table);

    unsafe {
        ap_entrypoint.finalize_and_register(trampoline_cr3);
    }

    println!("Kernel base is {:x}.", elf.base_address());
    println!("Kernel entrypoint is {:x}.", elf.entrypoint());

    let entry_data = KernelEntryData {
        entrypoint: elf.entrypoint(),
        gdt:        gdt as u64,
        kernel_cr3,
        trampoline_cr3,
    };

    // Cache entry data so APs can use them later to enter the kernel.
    *KERNEL_ENTRY_DATA.lock() = Some(entry_data);

    // Create a unique stack for this core.
    let rsp = create_kernel_stack() + KERNEL_STACK_SIZE;

    (entry_data, rsp)
}

unsafe fn locate_acpi(system_table: *mut efi::EfiSystemTable) {
    use efi::EfiGuid;

    const EFI_ACPI_TABLE_GUID: EfiGuid = EfiGuid(0xeb9d2d30, 0x2d88, 0x11d3,
                                                 [0x9a, 0x16, 0x0, 0x90,
                                                  0x27, 0x3f, 0xc1, 0x4d]);

    const EFI_ACPI_20_TABLE_GUID: EfiGuid = EfiGuid(0x8868e871, 0xe4f1, 0x11d3,
                                                    [0xbc, 0x22, 0x0, 0x80,
                                                     0xc7, 0x3c, 0x88, 0x81]);

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

                acpi.rsdt = Some(table.descriptor.rsdt_addr as u64);
                acpi.xsdt = Some(table.xsdt_addr);

                break;
            }
            _ => (),
        }
    }

    *BOOT_BLOCK.acpi_tables.lock() = acpi;
}

unsafe fn initialize_framebuffer(system_table: *mut efi::EfiSystemTable) {
    use efi::EfiGuid;

    const EFI_GOP_GUID: EfiGuid = EfiGuid(0x9042a9de, 0x23dc, 0x4a38, [0x96, 0xfb, 0x7a, 0xde,
                                                                       0xd0, 0x80, 0x51, 0x6a]);

    fn is_pixel_format_usable(format: efi::EfiGraphicsPixelFormat) -> bool {
        matches!(format, efi::PIXEL_RGB | efi::PIXEL_BGR | efi::PIXEL_BITMASK)
    }

    let mut protocol = 0;
    let status = ((*(*system_table).boot_services).locate_protocol)(&EFI_GOP_GUID,
                                                                    core::ptr::null_mut(),
                                                                    &mut protocol);
    if status != 0 {
        println!("WARNING: Getting EFI graphic output protocol failed with status {:x}.",
                 status);
        return;
    }

    let protocol = &mut *(protocol as *mut efi::EfiGraphicsOutputProtocol);

    /*
    let max_mode = (*protocol.mode).max_mode;

    for mode in 0..max_mode {
        let mut info         = core::ptr::null_mut();
        let mut size_of_info = 0;

        let status = (protocol.query_mode)(protocol, mode, &mut size_of_info, &mut info);

        assert!(status == 0);
        assert!(size_of_info >= core::mem::size_of::<efi::EfiGraphicsOutputModeInformation>());

        let info = &*info;

        println!("{}x{} {} {}", info.horizontal_res, info.vertical_res, info.pixel_format,
                 info.pixels_per_scanline);
    }
    */

    let mode      = &(*protocol.mode);
    let mode_info = &(*mode.info);

    if !is_pixel_format_usable(mode_info.pixel_format) {
        println!("WARNING: Selected EFI output mode is not usable as a framebuffer.");
        return;
    }
    
    let mut format = boot_block::PixelFormat {
        red:   0,
        green: 0,
        blue:  0,
    };

    match mode_info.pixel_format {
        efi::PIXEL_RGB => {
            format.red   = 0x0000ff;
            format.green = 0x00ff00;
            format.blue  = 0xff0000;
        }
        efi::PIXEL_BGR => {
            format.red   = 0xff0000;
            format.green = 0x00ff00;
            format.blue  = 0x0000ff;
        }
        efi::PIXEL_BITMASK => {
            format.red   = mode_info.pixel_info.red;
            format.green = mode_info.pixel_info.green;
            format.blue  = mode_info.pixel_info.blue;
        }
        _ => unreachable!(),
    }

    let framebuffer_info = boot_block::FramebufferInfo {
        width:               mode_info.horizontal_res,
        height:              mode_info.vertical_res,
        pixel_format:        format,
        pixels_per_scanline: mode_info.pixels_per_scanline,
        fb_base:             mode.fb_base as u64,
        fb_size:             mode.fb_size as u64,
    };

    *BOOT_BLOCK.framebuffer.lock() = Some(framebuffer_info);
}

#[no_mangle]
extern fn efi_main(image_handle: usize, system_table: *mut efi::EfiSystemTable) -> ! {
    if KERNEL_ENTRY_DATA.lock().is_none() {
        // We are executing for the first time and we have EFI services available.

        unsafe {
            serial::initialize();

            // Get addresses of ACPI tables.
            locate_acpi(system_table);

            // Try to initialize framebuffer device.
            initialize_framebuffer(system_table);

            mm::initialize_and_exit_boot_services(image_handle, system_table);
        }
    } else {
        // AP entrypoint should pass zeroes here because EFI is unavailable.
        assert!(image_handle == 0 && system_table == core::ptr::null_mut(),
                "Invalid arguments passed to the bootloader.");
    }

    // Load and map kernel if required. Also allocate a unique stack for this core.
    let (entry_data, rsp) = setup_kernel();

    extern "C" {
        fn enter_kernel(entrypoint: u64, rsp: u64, boot_block: u64, kernel_cr3: u64,
                        trampoline_cr3: u64, physical_region: u64, gdt: u64) -> !;
    }

    // Enter the 64 bit kernel!
    unsafe {
        enter_kernel(entry_data.entrypoint, rsp, &BOOT_BLOCK as *const _ as u64,
                     entry_data.kernel_cr3, entry_data.trampoline_cr3,
                     KERNEL_PHYSICAL_REGION_BASE, entry_data.gdt);
    }
}
