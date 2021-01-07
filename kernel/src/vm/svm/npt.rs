pub use page_table::{PageTable, PageType, PhysAddr, VirtAddr};

use crate::mm;

pub const NPT_PRESENT: u64 = page_table::PAGE_PRESENT;
pub const NPT_WRITE:   u64 = page_table::PAGE_WRITE;
pub const NPT_NX:      u64 = page_table::PAGE_NX;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[repr(C)]
pub struct GuestAddr(pub u64);

pub struct Npt {
    page_table: PageTable,
}

impl Npt {
    pub(super) fn new() -> Self {
        // Don't invalidate TLB on modifications.
        Self {
            page_table: PageTable::new_advanced(&mut mm::PhysicalMemory, false)
                .expect("Failed to allocate NPT for the VM."),
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

        self.page_table.map_raw(&mut mm::PhysicalMemory, VirtAddr(guest_addr.0), page_type,
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
        self.page_table.map(&mut mm::PhysicalMemory, VirtAddr(guest_addr.0), page_type,
                            size, write, exec, true)
            .expect("Failed to map memory in the NPT.");
    }

    pub unsafe fn guest_to_host(&self, guest_addr: GuestAddr) -> Option<PhysAddr> {
        self.page_table.virt_to_phys(&mut mm::PhysicalMemory, VirtAddr(guest_addr.0))
    }
}

impl Drop for Npt {
    fn drop(&mut self) {
        // `PageTable` doesn't destroy itself automatically so we need to do this here.
        unsafe {
            self.page_table.destroy(&mut mm::PhysicalMemory)
                .expect("Failed to destroy NPT.");
        }
    }
}
