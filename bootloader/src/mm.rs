use core::alloc::{GlobalAlloc, Layout};
use rangeset::{RangeSet, Range};
use page_table::{PhysMem, PhysAddr};
use core::convert::TryInto;
use crate::BOOT_BLOCK;
use crate::bios;

pub struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        BOOT_BLOCK.free_memory.lock().as_mut().and_then(|memory| {
            memory.allocate(layout.size() as u64, layout.align() as u64)
        }).unwrap_or(0) as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        BOOT_BLOCK.free_memory.lock().as_mut().and_then(|memory| {
            let start = ptr as u64;
            let end   = start.checked_add(layout.size().checked_sub(1)? as u64)?;

            memory.insert(Range { start, end });

            Some(())
        }).expect("Failed to free memory.");
    }
}

pub struct PhysicalMemory<'a>(pub &'a mut RangeSet);

impl PhysMem for PhysicalMemory<'_> {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        let phys_addr: usize = phys_addr.0.try_into().ok()?;
        let _phys_end: usize = phys_addr.checked_add(size.checked_sub(1)?)?;

        Some(phys_addr as *mut u8)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        self.0.allocate(layout.size() as u64, layout.align() as u64)
            .map(|addr| PhysAddr(addr as u64))
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator;

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("Allocation of memory with layout {:?} failed!", layout);
}

pub unsafe fn initialize() {
    let mut free_memory = BOOT_BLOCK.free_memory.lock();
    let mut memory      = RangeSet::new();

    assert!(free_memory.is_none(), "Bootloader memory manager was already initialized.");

    // Do two passes because some BIOSes are broken.
    for &cleanup_pass in &[false, true] {
        let mut sequence = 0;

        loop {
            #[repr(C)]
            #[derive(Default, Debug)]
            struct E820Entry {
                base: u64,
                size: u64,
                typ:  u32,
                acpi: u32,
            }

            // Some BIOSes won't set ACPI field so we need to make it valid in the beginning.
            let mut entry = E820Entry {
                acpi: 1,
                ..Default::default()
            };

            // Make sure that the entry is accessible by BIOS.
            assert!((&entry as *const _ as usize) < 0x10000,
                    "Entry is in high memory, BIOS won't be able to access it.");

            // Make sure that size matches excpected one.
            assert!(core::mem::size_of::<E820Entry>() == 24, "E820 entry has invalid size.");

            // Load all required magic values for this BIOS service.
            let mut regs = bios::RegisterState {
                eax: 0xe820,
                ebx: sequence,
                ecx: core::mem::size_of::<E820Entry>() as u32,
                edx: u32::from_be_bytes(*b"SMAP"),
                edi: &mut entry as *mut _ as u32,
                ..Default::default()
            };

            bios::interrupt(0x15, &mut regs);

            // Update current sequence so BIOS will know which entry to report
            // in the next iteration.
            sequence = regs.ebx;

            // Consider this entry valid only if ACPI bit 0 is set and range is not empty.
            if entry.acpi & 1 != 0 && entry.size > 0 {
                // Create inclusive range required by `RangeSet`.
                let start = entry.base;
                let end   = entry.base.checked_add(entry.size - 1)
                    .expect("E820 region overflowed.");
                let range = Range { start, end };

                let free = entry.typ == 1;

                // First pass will add all free memory to the list.
                // Second pass will remove all non-free memory from the list.
                // Some BIOSes may report that region is free and non-free at the
                // same time, we don't want to use such regions.
                if free && !cleanup_pass {
                    memory.insert(range);
                } else if !free && cleanup_pass {
                    memory.remove(range);
                }
            }

            // CF set indicates error or end of the list. sequence == 0 indicates end of the list.
            if regs.eflags & 1 != 0 || sequence == 0 {
                break;
            }
        }
    }

    // Remove first 1MB of memory, we store some data there which we don't want to overwrite.
    memory.remove(Range { start: 0, end: 1024 * 1024 - 1 });

    *free_memory = Some(memory);
}
