use core::alloc::{GlobalAlloc, Layout};
use rangeset::{RangeSet, Range};
use lock::Lock;
use crate::bios;

struct PhysicalMemory(RangeSet);

static PHYS_MEM: Lock<Option<PhysicalMemory>> = Lock::new(None);

pub struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        PHYS_MEM.lock().as_mut().and_then(|physmem| {
            physmem.0.allocate(layout.size() as u64, layout.align() as u64)
        }).unwrap_or(0) as *mut u8
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        PHYS_MEM.lock().as_mut().and_then(|physmem| {
            let start = ptr as u64;
            let end   = start.checked_add(layout.size().checked_sub(1)? as u64)?;

            physmem.0.insert(Range { start, end });

            Some(())
        }).expect("Failed to free memory.");
    }
}

#[global_allocator]
static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator;

#[alloc_error_handler]
fn alloc_error_handler(_layout: core::alloc::Layout) -> ! {
    panic!("Allocation failure!");
}

fn get_memory_map() -> RangeSet {
    let mut memory = RangeSet::new();

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

            unsafe { bios::interrupt(0x15, &mut regs); }

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

    memory
}

pub unsafe fn initialize() {
    let mut physmem = PHYS_MEM.lock();

    assert!(physmem.is_none(), "Bootloader memory manager was already initialized.");

    *physmem = Some(PhysicalMemory(get_memory_map()));
}
