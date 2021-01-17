pub use page_table::{PageTable, PageType, PhysAddr, VirtAddr};

use core::alloc::Layout;

pub const NPT_PRESENT: u64 = page_table::PAGE_PRESENT;
pub const NPT_WRITE:   u64 = page_table::PAGE_WRITE;
pub const NPT_NX:      u64 = page_table::PAGE_NX;

struct PhysicalMemory<'a> {
    invalidated_tlb: &'a mut bool,
}

impl page_table::PhysMem for PhysicalMemory<'_> {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        crate::mm::PhysicalMemory.translate(phys_addr, size)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        crate::mm::PhysicalMemory.alloc_phys(layout)
    }

    unsafe fn free_phys(&mut self, phys_addr: PhysAddr, size: usize) -> Option<()> {
        crate::mm::PhysicalMemory.free_phys(phys_addr, size)
    }

    unsafe fn invalidate_tlb(&mut self, _virt_addr: VirtAddr) {
        *self.invalidated_tlb = true;
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[repr(C)]
pub struct GuestAddr(pub u64);

pub struct Npt {
    page_table:                 PageTable,
    pub(super) invalidated_tlb: bool,
}

impl Npt {
    pub(super) fn new() -> Self {
        let mut phys_mem = PhysicalMemory {
            invalidated_tlb: &mut false,
        };

        Self {
            page_table: PageTable::new(&mut phys_mem)
                .expect("Failed to allocate NPT for the VM."),
            invalidated_tlb: false,
        }
    }

    pub(super) fn table(&mut self) -> PhysAddr {
        self.page_table.table()
    }

    #[track_caller]
    pub unsafe fn map_raw(
        &mut self,
        guest_addr: GuestAddr,
        page_type:  PageType,
        mut raw:    u64,
        add:        bool,
        update:     bool,
    ) {
        // A page is considered user in the guest only if it is marked as user at the guest level.
        // The page must be marked user in the nested page table to allow any guest access at all.
        raw |= page_table::PAGE_USER;

        let mut phys_mem = PhysicalMemory {
            invalidated_tlb: &mut self.invalidated_tlb,
        };

        self.page_table.map_raw(&mut phys_mem, VirtAddr(guest_addr.0), page_type,
                                raw, add, update)
            .expect("Failed to map memory in the NPT.");
    }

    #[track_caller]
    pub fn map(
        &mut self,
        guest_addr: GuestAddr,
        page_type:  PageType,
        size:       u64,
        write:      bool,
        exec:       bool,
    ) {
        let mut phys_mem = PhysicalMemory {
            invalidated_tlb: &mut self.invalidated_tlb,
        };

        self.page_table.map(&mut phys_mem, VirtAddr(guest_addr.0), page_type,
                            size, write, exec, true)
            .expect("Failed to map memory in the NPT.");
    }

    pub unsafe fn guest_to_host(&self, guest_addr: GuestAddr) -> Option<PhysAddr> {
        let mut phys_mem = PhysicalMemory {
            invalidated_tlb: &mut false,
        };

        self.page_table.virt_to_phys(&mut phys_mem, VirtAddr(guest_addr.0))
    }
}

impl Drop for Npt {
    fn drop(&mut self) {
        let mut phys_mem = PhysicalMemory {
            invalidated_tlb: &mut self.invalidated_tlb,
        };

        // `PageTable` doesn't destroy itself automatically so we need to do this here.
        unsafe {
            self.page_table.destroy(&mut phys_mem)
                .expect("Failed to destroy NPT.");
        }
    }
}
