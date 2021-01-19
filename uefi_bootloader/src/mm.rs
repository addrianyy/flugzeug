use core::convert::TryInto;
use core::alloc::Layout;

use crate::lock::Lock;
use rangeset::Range;
use page_table::{PhysMem, PhysAddr};

use crate::{BOOT_BLOCK, efi};
use efi::EfiGuid;

// When handling APs we will have only first 4GB mapped in.
pub const MAX_ADDRESS: usize = 0xffff_ffff;

#[derive(Clone)]
pub struct Image {
    pub base: usize,
    pub size: usize,
}

static BOOTLOADER_IMAGE: Lock<Option<Image>> = Lock::new(None);

pub struct PhysicalMemory;

impl PhysMem for PhysicalMemory {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        // We don't use paging in the bootloader so physcial address == virtual address.
        // Just make sure that address fits in first 4GB and region doesn't overflow.
        
        // WARNING: This code will work only if MAX_ADDRESS == 4GB - 1.

        let phys_addr: u32 = phys_addr.0.try_into().ok()?;
        let phys_size: u32 = size.try_into().ok()?;
        let _phys_end: u32 = phys_addr.checked_add(phys_size.checked_sub(1)?)?;

        Some(phys_addr as *mut u8)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        let mut free_memory = BOOT_BLOCK.free_memory.lock();

        // Make sure we never allocate > 4GB memory.
        free_memory.allocate_limited(layout.size() as u64, layout.align() as u64,
                                     Some(MAX_ADDRESS as u64))
            .map(|addr| PhysAddr(addr as u64))
    }
}

pub struct BootPhysicalMemory;

impl PhysMem for BootPhysicalMemory {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        PhysicalMemory.translate(phys_addr, size)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        let phys_addr = PhysicalMemory.alloc_phys(layout)?;

        // Mark this memory as usable after booting.
        mark_boot_memory(phys_addr.0, layout.size() as u64);

        Some(phys_addr)
    }
}

fn mark_boot_memory(start: u64, size: u64) {
    assert!(size > 0, "Cannot mark zero-sized block as boot memory.");

    let end = start + size - 1;

    // Mark this memory as usable after booting.
    BOOT_BLOCK.boot_memory
        .lock()
        .insert(Range { start, end });
}

fn allocate_boot_memory_internal(size: u64, align: u64, max_address: u64) -> Option<*mut u8> {
    assert!(max_address <= MAX_ADDRESS as u64, "Max address is higher than max supported one.");
    assert!(size > 0, "Cannot allocate 0 bytes of boot memory.");

    let pointer = BOOT_BLOCK.free_memory
        .lock()
        .allocate_limited(size, align, Some(max_address))?;

    // Mark this memory as usable after booting.
    mark_boot_memory(pointer as u64, size);

    Some(pointer as *mut u8)
}

pub fn allocate_low_boot_memory(size: u64, align: u64) -> Option<*mut u8> {
    // Low memory is witin first megabyte of physical memory.
    allocate_boot_memory_internal(size, align, 1024 * 1024 - 1)
}

pub fn allocate_boot_memory(size: u64, align: u64) -> Option<*mut u8> {
    allocate_boot_memory_internal(size, align, MAX_ADDRESS as u64)
}

fn locate_bootloader_image(image_handle: usize, boot_services: &mut efi::EfiBootServices) {
    const OPEN_IMAGE_PROTOCOL_GUID: EfiGuid =
        EfiGuid(0x5B1B31A1, 0x9562, 0x11d2, [0x8E, 0x3F, 0x00, 0xA0, 0xC9, 0x69, 0x72, 0x3B]);

    let mut interface = 0;

    // Use EFI_OPEN_PROTOCOL_GET_PROTOCOL. In this case the caller is not required
    // to call CloseProtocol afterwards.
    let status = unsafe {
        (boot_services.open_protocol)(image_handle, &OPEN_IMAGE_PROTOCOL_GUID, &mut interface,
                                      image_handle, 0, 2)
    };

    assert!(status == 0, "Failed to get information about loaded bootloader \
            image with status {:x}.", status);

    let image = unsafe { &*(interface as *const efi::EfiLoadedImageProtocol) };

    println!("Bootloader image: base 0x{:x}, size 0x{:x}.", image.image_base, image.image_size);

    assert!(image.image_base > 0 && image.image_size > 0, "Image is null.");
    assert!(image.image_base & 0xfff == 0 && image.image_size & 0xfff == 0,
            "Bootloader image is not page aligned.");

    *BOOTLOADER_IMAGE.lock() = Some(Image {
        base: image.image_base,
        size: image.image_size,
    });
}

pub fn bootloader_image() -> Image {
    BOOTLOADER_IMAGE.lock().as_ref().expect("Bootloader wasn't located yet.").clone()
}

pub unsafe fn initialize_and_exit_boot_services(image_handle: usize,
                                                system_table: *mut efi::EfiSystemTable) {
    // Max address needs to be always 4GB - 1.
    assert!(MAX_ADDRESS + 1 == 1024 * 1024 * 1024 * 4, "Unexpected MM max address.");

    let mut free_memory = BOOT_BLOCK.free_memory.lock();
    let mut boot_memory = BOOT_BLOCK.boot_memory.lock();

    assert!(free_memory.entries().is_empty() && boot_memory.entries().is_empty(),
            "Memory lists are already initialized.");

    let boot_services = &mut *((*system_table).boot_services);

    locate_bootloader_image(image_handle, boot_services);

    for try_count in 1.. {
        assert!(try_count < 10, "Too many failed tries to get memory \
                list and exit boot services.");

        // Clear memory lists.
        free_memory.clear();
        boot_memory.clear();

        let mut map_size     = 0;
        let mut map_key      = 0;
        let mut desc_size    = 0;
        let mut desc_version = 0;

        // Get size of memory map.
        let status = (boot_services.get_memory_map)(&mut map_size, core::ptr::null_mut(),
                                                    &mut map_key, &mut desc_size,
                                                    &mut desc_version);

        assert_eq!(status, 0x8000000000000005, "Status is not BUFFER_TOO_SMALL.");

        // Adjust map size to make sure that memory map will fit.
        map_size *= 2;

        // Allocate pool for memory map.
        let mut pool = core::ptr::null_mut();
        let status   = (boot_services.allocate_pool)(efi::EFI_LOADER_DATA, map_size,
                                                     &mut pool);

        assert_eq!(status, 0, "Allocating memory pool failed.");

        // Get the memory map.
        let status = (boot_services.get_memory_map)(&mut map_size, pool as *mut _,
                                                    &mut map_key, &mut desc_size,
                                                    &mut desc_version);

        // If we have failed to get memory map free the pool and try next time.
        if status != 0 {
            assert_eq!((boot_services.free_pool)(pool), 0, "Freeing pool failed.");

            continue;
        }

        // Make sure that we have got valid results.
        assert!(desc_size >= core::mem::size_of::<efi::EfiMemoryDescriptor>(),
                "Descriptor size is lower than our struct size.");
        assert!(map_size % desc_size == 0,
                "Map size is not divisible by descriptor size.");

        let entries = map_size / desc_size;

        for index in 0..entries {
            let desc = &*((pool as usize + index * desc_size) as *const efi::EfiMemoryDescriptor);

            // Ignore non-writeback memory.
            if desc.attribute & 8 == 0 {
                continue;
            }

            let target_list = match desc.typ {
                efi::EFI_CONVENTIONAL_MEMORY => Some(&mut free_memory),
                efi::EFI_LOADER_CODE        |
                efi::EFI_LOADER_DATA        |
                efi::EFI_BOOT_SERVICES_CODE |
                efi::EFI_BOOT_SERVICES_DATA => {
                    // Memory which can be freed after we have finished boot process.
                    Some(&mut boot_memory)
                }
                _ => None,
            };

            if let Some(target_list) = target_list {
                assert!(desc.pages > 0, "Invalid, empty entry.");

                let range = Range {
                    start: desc.physical_start,
                    end:   (desc.physical_start + desc.pages * 4096) - 1,
                };

                target_list.insert(range);
            }
        }

        // `boot_memory` now contains our bootloader. After finishing boot process it is not
        // needed. Boot block will be moved by the kernel so it can be freed here too.

        // Even if we will fail to exit we won't be able to use EFI print services anymore.
        crate::print::on_exited_boot_services();

        // If we have failed to exit boot services free the pool and try next time.
        let status = (boot_services.exit_boot_services)(image_handle, map_key);
        if  status != 0 {
            assert_eq!((boot_services.free_pool)(pool), 0, "Freeing pool failed.");

            continue;
        }

        break;
    }
}
