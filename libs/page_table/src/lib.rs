#![no_std]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use core::alloc::Layout;

pub const PAGE_PRESENT: u64 = 1 << 0;
pub const PAGE_WRITE:   u64 = 1 << 1;
pub const PAGE_USER:    u64 = 1 << 2;
pub const PAGE_NX:      u64 = 1 << 63;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[repr(C)]
pub struct PhysAddr(pub u64);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[repr(C)]
pub struct VirtAddr(pub u64);

pub trait PhysMem {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8>;

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr>;

    fn alloc_phys_zeroed(&mut self, layout: Layout) -> Option<PhysAddr> {
        let phys_addr = self.alloc_phys(layout)?;

        unsafe {
            let virt_addr = self.translate(phys_addr, layout.size())?;
            core::ptr::write_bytes(virt_addr, 0, layout.size())
        }

        Some(phys_addr)
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u64)]
pub enum PageType {
    Page4K = 4096,
    Page2M = 2 * 1024 * 1024,
    Page1G = 1 * 1024 * 1024 * 1024,
}

pub struct PageTable {
    table: PhysAddr,
}

impl PageTable {
    pub fn new(phys_mem: &mut impl PhysMem) -> Option<Self> {
        let table = phys_mem.alloc_phys_zeroed(
            Layout::from_size_align(4096, 4096).ok()?)?;

        Some(Self {
            table,
        })
    }

    pub unsafe fn from_table(table: PhysAddr) -> Self {
        Self {
            table: PhysAddr(table.0 & 0xffffffffff000),
        }
    }

    pub unsafe fn map_raw(&mut self, phys_mem: &mut impl PhysMem, virt_addr: VirtAddr,
                          page_type: PageType, raw: u64, add: bool, update: bool) -> Option<()> {
        const U64_SIZE: u64 = core::mem::size_of::<u64>() as u64;

        let page_size = page_type as u64;
        let page_mask = page_size - 1;

        // Make sure that the `virt_addr` is properly aligned.
        if virt_addr.0 & page_mask != 0 {
            return None;
        }

        let mut indices = [0u64; 4];

        // Calculate virtual address indices for given page type.
        let indices = match page_type {
            PageType::Page4K => {
                indices[0] = (virt_addr.0 >> 39) & 0x1ff;
                indices[1] = (virt_addr.0 >> 30) & 0x1ff;
                indices[2] = (virt_addr.0 >> 21) & 0x1ff;
                indices[3] = (virt_addr.0 >> 12) & 0x1ff;

                &indices[..4]
            }
            PageType::Page2M => {
                indices[0] = (virt_addr.0 >> 39) & 0x1ff;
                indices[1] = (virt_addr.0 >> 30) & 0x1ff;
                indices[2] = (virt_addr.0 >> 21) & 0x1ff;

                &indices[..3]
            }
            PageType::Page1G => {
                indices[0] = (virt_addr.0 >> 39) & 0x1ff;
                indices[1] = (virt_addr.0 >> 30) & 0x1ff;

                &indices[..2]
            }
        };

        let mut table = self.table.0;

        for (depth, &index) in indices.iter().enumerate() {
            // Get the physical address of current entry.
            let entry_ptr = PhysAddr(table + index * U64_SIZE);

            // Get the virtual address of current entry.
            let entry_ptr = phys_mem.translate(entry_ptr, U64_SIZE as usize)? as *mut u64;

            let entry = *entry_ptr;

            // If it's not the the last lavel entry and it's non-present then we can
            // allocate it and continue traversing
            if depth != indices.len() - 1 && entry & PAGE_PRESENT == 0 {
                // Check if we are allowed to create new entries.
                if !add {
                    return None;
                }

                let new_table = phys_mem.alloc_phys_zeroed(
                    Layout::from_size_align(4096, 4096).ok()?)?;

                // Create new entry with max permissions and mark it as present.
                *entry_ptr = new_table.0 | PAGE_PRESENT | PAGE_USER | PAGE_WRITE;
            }

            // Check if we are at the final level.
            if depth == indices.len() - 1 {
                // `update` needs to be set if we are going to change already present entry.
                if entry & PAGE_PRESENT == 0 || update {
                    *entry_ptr = raw;

                    // Check if we can access this virtual address in current processor mode.
                    let accessible = (virt_addr.0 as u64) < (core::mem::size_of::<usize>() as u64);

                    // If the entry was already present and virtual address is accessible then
                    // we need to flush TLB.
                    if entry & PAGE_PRESENT != 0 && accessible {
                        cpu::invlpg(virt_addr.0 as usize);
                    }

                    return Some(());
                } else {
                    return None;
                }
            }

            // Go to the next level in paging hierarchy.
            table = *entry_ptr & 0xffffffffff000;
        }

        unreachable!()
    }
}
