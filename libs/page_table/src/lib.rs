#![no_std]
#![allow(clippy::identity_op, clippy::missing_safety_doc, clippy::too_many_arguments)]

// Everything here must be exactly the same in 32 bit mode and 64 bit mode.

use core::alloc::Layout;

/// Page properties used by 64 bit x86 paging.
pub const PAGE_PRESENT:         u64 = 1 << 0;
pub const PAGE_WRITE:           u64 = 1 << 1;
pub const PAGE_USER:            u64 = 1 << 2;
pub const PAGE_PWT:             u64 = 1 << 3;
pub const PAGE_CACHE_DISABLE:   u64 = 1 << 4;
pub const PAGE_SIZE:            u64 = 1 << 7;
pub const PAGE_PAT:             u64 = 1 << 7;
pub const PAGE_NX:              u64 = 1 << 63;

/// Internal flag that can be only present on last level entries. It signifies that
/// backing page should be freed when destroying page table. This bit is ignored by the
/// architecture.
const DEALLOCATE_FLAG: u64 = 1 << 9;
const U64_SIZE:        u64 = core::mem::size_of::<u64>() as u64;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[repr(C)]
pub struct PhysAddr(pub u64);

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[repr(C)]
pub struct VirtAddr(pub u64);

impl VirtAddr {
    /// Check if the address is canonical. If it's not then it cannot be mapped and every access
    /// to it will cause #GP.
    pub fn is_canonical(&self) -> bool {
        let mut addr = self.0 as i64;

        // Sign extend last 12 bits of the address.
        addr <<= 12;
        addr >>= 12;

        // Check if sign extended address is the same as original one.
        addr as u64 == self.0
    }
}

/// Trait that allows manipulation of physical memory in various environments.
pub trait PhysMem {
    /// Translate a physical address `phys_addr` to a virtual address. Returned pointer
    /// is valid only for `size` bytes.
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8>;

    /// Allocate a physical region with `layout`.
    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr>;

    /// Allocate a zeroed physical region with `layout`.
    fn alloc_phys_zeroed(&mut self, layout: Layout) -> Option<PhysAddr> {
        // Allocate normal, non-zeroed physical region.
        let phys_addr = self.alloc_phys(layout)?;

        unsafe {
            // Translate physical address to virtual one and zero out the memory.
            let virt_addr = self.translate(phys_addr, layout.size())?;
            core::ptr::write_bytes(virt_addr, 0, layout.size())
        }

        Some(phys_addr)
    }

    unsafe fn free_phys(&mut self, _phys_addr: PhysAddr, _size: usize) -> Option<()> {
        panic!("Freeing is not supported.")
    }
}

/// x86 page type. CPU may not support all these page types.
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
#[repr(u64)]
pub enum PageType {
    Page4K = 4096,
    Page2M = 2 * 1024 * 1024,
    Page1G = 1 * 1024 * 1024 * 1024,
}

/// Wrapper that allows manipulating x86 page tables. It doesn't own any page tables so
/// they won't get freed on `Drop`.
pub struct PageTable {
    /// Physical address of a root table (PML4), Must be always valid.
    table: PhysAddr,

    /// Set to true if modifications to page table require TLB invalidation.
    invalidate_tlb: bool,
}

impl PageTable {
    /// Create a new, empty page table witout any mapped memory.
    pub fn new_advanced(phys_mem: &mut impl PhysMem, invalidate_tlb: bool) -> Option<Self> {
        // Allocate empty root table (PML4).
        let table = phys_mem.alloc_phys_zeroed(
            Layout::from_size_align(4096, 4096).ok()?,
        )?;

        Some(Self {
            table,
            invalidate_tlb,
        })
    }

    /// Create a new, empty page table witout any mapped memory.
    pub fn new(phys_mem: &mut impl PhysMem) -> Option<Self> {
        Self::new_advanced(phys_mem, true)
    }

    /// Create a page table from PML4 physical address (eg. CR3).
    pub unsafe fn from_table(table: PhysAddr, invalidate_tlb: bool) -> Self {
        // Mask off VPID and other stuff from CR3.
        Self {
            table: PhysAddr(table.0 & 0xffffffffff000),
            invalidate_tlb,
        }
    }

    /// Get the physical address of a root table (PML4).
    pub fn table(&mut self) -> PhysAddr {
        self.table
    }

    /// Map region at `virt_addr` with size `size`. Mapped region will be zeroed.
    #[must_use]
    pub fn map(
        &mut self,
        phys_mem:  &mut impl PhysMem,
        virt_addr: VirtAddr,
        page_type: PageType,
        size:      u64,
        write:     bool,
        exec:      bool,
        user:      bool,
    ) -> Option<()> {
        self.map_init(phys_mem, virt_addr, page_type, size, write, exec, user,
                      None::<fn(u64) -> u8>)
    }

    /// Map region at `virt_addr` with size `size`. `init` function is used to initialize
    /// memory contents of the new region.
    #[must_use]
    pub fn map_init(
        &mut self,
        phys_mem:  &mut impl PhysMem,
        virt_addr: VirtAddr,
        page_type: PageType,
        size:      u64,
        write:     bool,
        exec:      bool,
        user:      bool,
        init:      Option<impl Fn(u64) -> u8>,
    ) -> Option<()> {
        let page_size = page_type as u64;
        let page_mask = page_size - 1;

        // Make sure both virtual address and size are correctly aligned.
        if size == 0 || size & page_mask != 0 || virt_addr.0 & page_mask != 0 {
            return None;
        }

        // If page size is not standard 4K we will need to use PAGE_SIZE bit.
        let large = page_type != PageType::Page4K;

        // Calculate inclusive end of virtual region and make sure it doesn't overflow.
        let virt_end = virt_addr.0.checked_add(size - 1)?;

        // Go through each page in virtual region.
        for current_virt_addr in (virt_addr.0..=virt_end).step_by(page_size as usize) {
            // Allocate backing physical page.
            let page = phys_mem.alloc_phys_zeroed(
                Layout::from_size_align(page_size as usize, page_size as usize).unwrap(),
            )?;

            // Calculate value of raw page table entry.
            let raw = page.0 | PAGE_PRESENT |
                if write { PAGE_WRITE } else { 0       } |
                if exec  { 0          } else { PAGE_NX } |
                if user  { PAGE_USER  } else { 0       } |
                if large { PAGE_SIZE  } else { 0       };

            if let Some(init) = &init {
                let bytes = unsafe {
                    let bytes = phys_mem.translate(page, page_size as usize)?;
                    core::slice::from_raw_parts_mut(bytes, page_size as usize)
                };

                // Ask `init` routine to initialize this region.
                for (byte_offset, byte) in bytes.iter_mut().enumerate() {
                    let region_offset = current_virt_addr - virt_addr.0;

                    *byte = init(region_offset + byte_offset as u64);
                }
            }

            // Map current page. Fail it it was already used. Set `deallocate` flag so
            // the memory will be freed when destroying page table.
            unsafe {
                self.map_raw_internal(phys_mem, VirtAddr(current_virt_addr), page_type, raw,
                                      true, false, true)?;
            }
        }

        Some(())
    }

    /// Set page table entry value that describes `virt_addr` to `raw`.
    #[must_use]
    pub unsafe fn map_raw(
        &mut self,
        phys_mem:  &mut impl PhysMem,
        virt_addr: VirtAddr,
        page_type: PageType,
        raw:       u64,
        add:       bool,
        update:    bool,
    ) -> Option<()> {
        self.map_raw_internal(phys_mem, virt_addr, page_type, raw, add, update, false)
    }

    /// Set page table entry value that describes `virt_addr` to `raw`. If `deallocate` flag
    /// is set then backing memory will be deallocated when destroying page table.
    #[must_use]
    unsafe fn map_raw_internal(
        &mut self,
        phys_mem:   &mut impl PhysMem,
        virt_addr:  VirtAddr,
        page_type:  PageType,
        mut raw:    u64,
        add:        bool,
        update:     bool,
        deallocate: bool,
    ) -> Option<()> {
        // Make sure that nobody set the deallocate flag.
        assert!(raw & DEALLOCATE_FLAG == 0, "Internal flag was set in the page table entry.");

        if deallocate {
            raw |= DEALLOCATE_FLAG;
        }

        // If we are using large pages we need to set `PAGE_SIZE` bit in the page table entry.
        if page_type != PageType::Page4K {
            raw |= PAGE_SIZE;
        }

        if !virt_addr.is_canonical() {
            return None;
        }

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
            let entry_ptr = PhysAddr(table + index * U64_SIZE);
            let entry_ptr = phys_mem.translate(entry_ptr, U64_SIZE as usize)? as *mut u64;
            let entry     = *entry_ptr;

            if depth != indices.len() - 1 {
                if entry & PAGE_PRESENT == 0 {
                    // If it's not the the last lavel entry and it's non-present then we can
                    // allocate it and continue traversing

                    // Check if we are allowed to create new entries.
                    if !add {
                        return None;
                    }

                    let new_table = phys_mem.alloc_phys_zeroed(
                        Layout::from_size_align(4096, 4096).ok()?
                    )?;

                    // Create new entry with max permissions and mark it as present.
                    *entry_ptr = new_table.0 | PAGE_PRESENT | PAGE_USER | PAGE_WRITE;
                } else if entry & PAGE_SIZE != 0 {
                    // Mapped page type is different than what was specified in `page_type`.
                    return None;
                }
            } else {
                // We are at the final level of page table.

                // `update` needs to be set if we are going to change already present entry.
                if entry & PAGE_PRESENT == 0 || update {
                    *entry_ptr = raw;

                    if self.invalidate_tlb {
                        // Check if we can access this virtual address in current processor mode.
                        let accessible = (virt_addr.0 as u64) <= (usize::MAX as u64);

                        // If the entry was already present and virtual address is accessible then
                        // we need to flush TLB.
                        if entry & PAGE_PRESENT != 0 && accessible {
                            cpu::invlpg(virt_addr.0 as usize);
                        }
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

    #[must_use]
    pub fn virt_to_phys(
        &self,
        phys_mem:  &mut impl PhysMem,
        virt_addr: VirtAddr,
    ) -> Option<PhysAddr> {
        if !virt_addr.is_canonical() {
            return None;
        }

        let indices = [
            (virt_addr.0 >> 39) & 0x1ff,
            (virt_addr.0 >> 30) & 0x1ff,
            (virt_addr.0 >> 21) & 0x1ff,
            (virt_addr.0 >> 12) & 0x1ff,
        ];

        let mut table = self.table.0;

        for (depth, &index) in indices.iter().enumerate() {
            let entry = unsafe {
                let entry_ptr = PhysAddr(table + index * U64_SIZE);
                let entry_ptr = phys_mem.translate(entry_ptr, U64_SIZE as usize)? as *mut u64;

                *entry_ptr
            };

            // Given `virt_addr` is not mapped in.
            if (entry & PAGE_PRESENT) == 0 {
                return None;
            }

            let page_mask = if depth == indices.len() - 1 {
                Some(0xfff)
            } else if (entry & PAGE_SIZE) != 0 {
                match depth {
                    1 => Some(0x3fffffff), // 1G page.
                    2 => Some(0x1fffff  ), // 2M page.
                    _ => return None,      // Invalid page table entry.
                }
            } else {
                None
            };

            if let Some(page_mask) = page_mask {
                return Some(PhysAddr((entry & 0xffffffffff000) + (virt_addr.0 & page_mask)));
            }

            // Go to the next level in paging hierarchy.
            table = entry & 0xffffffffff000;
        }

        unreachable!()
    }

    #[must_use]
    pub unsafe fn destroy(&mut self, phys_mem: &mut impl PhysMem) -> Option<()> {
        Self::destroy_level(phys_mem, 0, self.table.0 & 0xffffffffff000)
    }

    #[must_use]
    unsafe fn destroy_level(phys_mem: &mut impl PhysMem, depth: usize, table: u64) -> Option<()> {
        let table_phys = PhysAddr(table);
        let table      = phys_mem.translate(table_phys, 4096)? as *mut u64;

        for index in 0..512 {
            let entry_ptr = table.add(index);
            let entry     = *entry_ptr;

            if (entry & PAGE_PRESENT) == 0 {
                continue;
            }

            let page_size = if depth == 3 {
                Some(4096)
            } else if (entry & PAGE_SIZE) != 0 {
                match depth {
                    1 => Some(1024 * 1024 * 1024), // 1G page.
                    2 => Some(2    * 1024 * 1024), // 2M page.
                    _ => return None,              // Invalid page table entry.
                }
            } else {
                None
            };

            let backing = entry & 0xffffffffff000;

            if let Some(page_size) = page_size {
                if (entry & DEALLOCATE_FLAG) != 0 {
                    phys_mem.free_phys(PhysAddr(backing), page_size)?;
                }
            } else {
                Self::destroy_level(phys_mem, depth + 1, backing)?;
            }

            *entry_ptr = 0;
        }

        phys_mem.free_phys(table_phys, 4096)?;

        Some(())
    }
}
