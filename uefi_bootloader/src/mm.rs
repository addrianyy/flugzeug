use core::convert::TryInto;
use core::alloc::Layout;

use rangeset::{RangeSet, Range};
use page_table::{PhysMem, PhysAddr};
use crate::{BOOT_BLOCK, efi};

unsafe fn translate(phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
    // We don't use paging in the bootloader so physcial address == virtual address.
    // Just make sure that address fits in pointer and region doesn't overflow.
    
    let phys_addr: usize = phys_addr.0.try_into().ok()?;
    let _phys_end: usize = phys_addr.checked_add(size.checked_sub(1)?)?;

    Some(phys_addr as *mut u8)
}

fn alloc_phys(layout: Layout, max_address: Option<usize>) -> Option<PhysAddr> {
    let mut free_memory = BOOT_BLOCK.free_memory.lock();
    let free_memory     = free_memory.as_mut().unwrap();

    free_memory.allocate_limited(layout.size() as u64, layout.align() as u64,
                                 max_address.map(|m| m as u64))
        .map(|addr| PhysAddr(addr as u64))
}

pub struct PhysicalMemory;

impl PhysMem for PhysicalMemory {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        translate(phys_addr, size)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        alloc_phys(layout, None)
    }
}

pub struct PhysicalMemory32;

impl PhysMem for PhysicalMemory32 {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        translate(phys_addr, size)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        // Make sure we never allocate > 4GB memory.
        alloc_phys(layout, Some(0xffff_ffff))
    }
}

pub unsafe fn initialize_and_exit_boot_services(image_handle: usize,
                                                system_table: *mut efi::EfiSystemTable) {
    let mut free_memory = BOOT_BLOCK.free_memory.lock();
    let mut memory      = RangeSet::new();

    assert!(free_memory.is_none(), "Free memory list was already initialized.");

    let boot_services = &mut *((*system_table).boot_services);
    let mut tries     = 0;

    let map_key = loop {
        let mut map_size     = 0;
        let mut map_key      = 0;
        let mut desc_size    = 0;
        let mut desc_version = 0;
        let status = (boot_services.get_memory_map)(&mut map_size, core::ptr::null_mut(),
                                                    &mut map_key, &mut desc_size,
                                                    &mut desc_version);

        assert_eq!(status, 0x8000000000000005, "Status is not BUFFER_TOO_SMALL.");

        map_size *= 2;

        let mut pool = core::ptr::null_mut();
        let status   = (boot_services.allocate_pool)(efi::EFI_LOADER_DATA, map_size,
                                                        &mut pool);

        assert_eq!(status, 0, "Allocating memory pool failed.");

        let status = (boot_services.get_memory_map)(&mut map_size, pool as *mut _,
                                                    &mut map_key, &mut desc_size,
                                                    &mut desc_version);

        if status != 0 {
            assert!(tries < 5, "Too many failed tries to get memory pool.");

            tries += 1;

            assert_eq!((boot_services.free_pool)(pool), 0, "Freeing pool failed.");
        } else {
            assert!(desc_size >= core::mem::size_of::<efi::EfiMemoryDescriptor>(),
                    "Descriptor size is lower than our struct size.");
            assert!(map_size % desc_size == 0,
                    "Map size is not divisible by descriptor size.");

            let entries = map_size / desc_size;

            for index in 0..entries {
                let desc_ptr = (pool as usize + index * desc_size)
                    as *const efi::EfiMemoryDescriptor;

                let desc   = &*desc_ptr;
                let usable = matches!(desc.typ,
                                        efi::EFI_LOADER_CODE |
                                        efi::EFI_LOADER_DATA |
                                        efi::EFI_BOOT_SERVICES_CODE |
                                        efi::EFI_BOOT_SERVICES_DATA |
                                        efi::EFI_CONVENTIONAL_MEMORY);

                if usable {
                    assert!(desc.pages > 0, "Invalid empty entry.");

                    let range = Range {
                        start: desc.physical_start,
                        end:   (desc.physical_start + desc.pages * 4096) - 1,
                    };

                    memory.insert(range);
                }
            }

            break map_key;
        }
    };

    let status = (boot_services.exit_boot_services)(image_handle, map_key);

    assert_eq!(status, 0, "Failed to exit boot services.");

    *free_memory = Some(memory);
}
