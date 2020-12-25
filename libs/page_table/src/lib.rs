#![no_std]

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
}

impl PageTable {
    /// Create a new, empty page table witout any mapped memory.
    pub fn new(phys_mem: &mut impl PhysMem) -> Option<Self> {
        // Allocate empty root table (PML4) and use it.
        let table = phys_mem.alloc_phys_zeroed(
            Layout::from_size_align(4096, 4096).ok()?)?;

        Some(Self {
            table,
        })
    }

    /// Create a page table from PML4 physical address (eg. CR3).
    pub unsafe fn from_table(table: PhysAddr) -> Self {
        // Mask off VPID and other stuff from CR3.
        Self {
            table: PhysAddr(table.0 & 0xffffffffff000),
        }
    }

    /// Get the physical address of a root table (PML4).
    pub fn table(&mut self) -> PhysAddr {
        self.table
    }

    /// Map region at `virt_addr` with size `size`. Mapped region will contain usable but
    /// uninitialized data.
    pub fn map(
        &mut self,
        phys_mem:  &mut impl PhysMem,
        virt_addr: VirtAddr,
        page_type: PageType,
        size:      u64,
        write:     bool,
        exec:      bool,
    ) -> Option<()> {
        self.map_init(phys_mem, virt_addr, page_type, size,
                      write, exec, None::<fn(u64) -> u8>)
    }

    /// Map region at `virt_addr` with size `size`. `init` function is used to initialize
    /// memory contents of the new region.
    pub fn map_init(
        &mut self,
        phys_mem:  &mut impl PhysMem,
        virt_addr: VirtAddr,
        page_type: PageType,
        size:      u64,
        write:     bool,
        exec:      bool,
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
            let page = phys_mem.alloc_phys(
                Layout::from_size_align(page_size as usize,
                                        page_size as usize).unwrap())?;

            // Calculate value of raw page table entry.
            let raw = page.0 | PAGE_PRESENT |
                if write { PAGE_WRITE } else { 0 } |
                if exec  { 0 }          else { PAGE_NX } |
                if large { PAGE_SIZE }  else { 0 };

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

            // Map current page. Fail it it was already used.
            unsafe {
                self.map_raw(phys_mem, VirtAddr(current_virt_addr), page_type, raw,
                             true, false)?;
            }
        }

        Some(())
    }

    /// Set page table entry value that describes `virt_addr` to `raw`.
    pub unsafe fn map_raw(
        &mut self,
        phys_mem:  &mut impl PhysMem,
        virt_addr: VirtAddr,
        page_type: PageType,
        raw:       u64,
        add:       bool,
        update:    bool,
    ) -> Option<()> {
        const U64_SIZE: u64 = core::mem::size_of::<u64>() as u64;

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
            // Get the physical address of current entry.
            let entry_ptr = PhysAddr(table + index * U64_SIZE);

            // Get the virtual address of current entry.
            let entry_ptr = phys_mem.translate(entry_ptr, U64_SIZE as usize)? as *mut u64;

            let entry = *entry_ptr;

            if depth != indices.len() - 1 {
                if entry & PAGE_PRESENT == 0 {
                    // If it's not the the last lavel entry and it's non-present then we can
                    // allocate it and continue traversing

                    // Check if we are allowed to create new entries.
                    if !add {
                        return None;
                    }

                    let new_table = phys_mem.alloc_phys_zeroed(
                        Layout::from_size_align(4096, 4096).ok()?)?;

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

                    // Check if we can access this virtual address in current processor mode.
                    let accessible = (virt_addr.0 as u64) <= (usize::MAX as u64);

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
