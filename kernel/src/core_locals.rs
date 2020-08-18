use core::sync::atomic::{AtomicU64, Ordering};
use core::alloc::Layout;

use crate::mm::{self, FreeList};
use boot_block::BootBlock;
use page_table::PhysAddr;
use crate::apic::Apic;
use lock::Lock;

static NEXT_FREE_CORE_ID: AtomicU64 = AtomicU64::new(0);

#[macro_export]
macro_rules! core {
    () => { $crate::core_locals::get_core_locals() }
}

#[inline]
pub fn get_core_locals() -> &'static CoreLocals {
    unsafe {
        let core_locals: usize;

        asm!("mov {}, gs:[0]", out(reg) core_locals);

        &*(core_locals as *const CoreLocals)
    }
}

#[repr(C)]
pub struct CoreLocals {
    /// Must be always the first field in the structure.
    self_address: usize,

    /// Unique identifier for this CPU. 0 is BSP.
    pub id: u64,

    /// Data shared between bootloader and kernel.
    pub boot_block: &'static BootBlock,

    /// Local APIC for this core.
    pub apic: Lock<Option<Apic>>,

    /// Free lists for each power-of-two size.
    /// The free list size is `(1 << (index + 3))`.
    free_lists: [Lock<FreeList>; 61],
}

impl CoreLocals {
    pub unsafe fn free_list(&self, layout: Layout) -> &Lock<FreeList> {
        // Free lists start at 8 bytes, round it up if needed.
        let size = core::cmp::max(layout.size(), 8);

        // Round up size to the nearest power of two and get the log2 of it
        // to determine the index into the free lists.
        let index = 64 - (size - 1).leading_zeros();

        // Compute the alignment of the free list associated with this memory.
        // Free lists are naturally aligned until 4096 byte sizes, at which
        // point they remain only 4096 byte aligned.
        let free_list_align = 1 << core::cmp::min(index, 12);

        assert!(free_list_align >= layout.align(),
            "Cannot satisfy alignment requirement from the free list.");

        // Get the free list corresponding to this size.
        &self.free_lists[index as usize - 3]
    }
}

// Make sure that `CoreLocals` is Sync.
trait SyncGuard: Sync + Sized {}
impl SyncGuard for CoreLocals {}

pub unsafe fn initialize(boot_block: PhysAddr) {
    const IA32_GS_BASE: u32 = 0xc0000101;

    // Make sure that core locals haven't been initialized yet.
    assert!(cpu::rdmsr(IA32_GS_BASE) == 0, "Core locals were already initialized.");

    // Get a unique identifier for this core.
    let core_id = NEXT_FREE_CORE_ID.fetch_add(1, Ordering::SeqCst);

    assert!(boot_block.0 != 0, "Boot block is null.");

    let boot_block = mm::phys_ref::<BootBlock>(boot_block).unwrap();

    // Make sure that structure size is the same in 32 bit and 64 bit mode.
    assert!(boot_block.size == core::mem::size_of::<BootBlock>() as u64,
            "Boot block size mismatch.");

    let core_locals_ptr = {
        let mut free_memory = boot_block.free_memory.lock();
        let free_memory     = free_memory.as_mut().unwrap();

        // Allocate core locals using physical allocator, at this stage it is the only
        // allocator available.
        let core_locals_phys = free_memory.allocate(
            core::mem::size_of::<CoreLocals>()  as u64,
            core::mem::align_of::<CoreLocals>() as u64,
        ).expect("Failed to allocate core locals.") as u64;

        mm::phys_ref::<CoreLocals>(PhysAddr(core_locals_phys)).unwrap() as *const _ as usize
    };

    let core_locals = CoreLocals {
        self_address: core_locals_ptr,
        id:           core_id,
        apic:         Lock::new(None),
        boot_block,
        free_lists: [
            Lock::new(FreeList::new(0x0000000000000008)),
            Lock::new(FreeList::new(0x0000000000000010)),
            Lock::new(FreeList::new(0x0000000000000020)),
            Lock::new(FreeList::new(0x0000000000000040)),
            Lock::new(FreeList::new(0x0000000000000080)),
            Lock::new(FreeList::new(0x0000000000000100)),
            Lock::new(FreeList::new(0x0000000000000200)),
            Lock::new(FreeList::new(0x0000000000000400)),
            Lock::new(FreeList::new(0x0000000000000800)),
            Lock::new(FreeList::new(0x0000000000001000)),
            Lock::new(FreeList::new(0x0000000000002000)),
            Lock::new(FreeList::new(0x0000000000004000)),
            Lock::new(FreeList::new(0x0000000000008000)),
            Lock::new(FreeList::new(0x0000000000010000)),
            Lock::new(FreeList::new(0x0000000000020000)),
            Lock::new(FreeList::new(0x0000000000040000)),
            Lock::new(FreeList::new(0x0000000000080000)),
            Lock::new(FreeList::new(0x0000000000100000)),
            Lock::new(FreeList::new(0x0000000000200000)),
            Lock::new(FreeList::new(0x0000000000400000)),
            Lock::new(FreeList::new(0x0000000000800000)),
            Lock::new(FreeList::new(0x0000000001000000)),
            Lock::new(FreeList::new(0x0000000002000000)),
            Lock::new(FreeList::new(0x0000000004000000)),
            Lock::new(FreeList::new(0x0000000008000000)),
            Lock::new(FreeList::new(0x0000000010000000)),
            Lock::new(FreeList::new(0x0000000020000000)),
            Lock::new(FreeList::new(0x0000000040000000)),
            Lock::new(FreeList::new(0x0000000080000000)),
            Lock::new(FreeList::new(0x0000000100000000)),
            Lock::new(FreeList::new(0x0000000200000000)),
            Lock::new(FreeList::new(0x0000000400000000)),
            Lock::new(FreeList::new(0x0000000800000000)),
            Lock::new(FreeList::new(0x0000001000000000)),
            Lock::new(FreeList::new(0x0000002000000000)),
            Lock::new(FreeList::new(0x0000004000000000)),
            Lock::new(FreeList::new(0x0000008000000000)),
            Lock::new(FreeList::new(0x0000010000000000)),
            Lock::new(FreeList::new(0x0000020000000000)),
            Lock::new(FreeList::new(0x0000040000000000)),
            Lock::new(FreeList::new(0x0000080000000000)),
            Lock::new(FreeList::new(0x0000100000000000)),
            Lock::new(FreeList::new(0x0000200000000000)),
            Lock::new(FreeList::new(0x0000400000000000)),
            Lock::new(FreeList::new(0x0000800000000000)),
            Lock::new(FreeList::new(0x0001000000000000)),
            Lock::new(FreeList::new(0x0002000000000000)),
            Lock::new(FreeList::new(0x0004000000000000)),
            Lock::new(FreeList::new(0x0008000000000000)),
            Lock::new(FreeList::new(0x0010000000000000)),
            Lock::new(FreeList::new(0x0020000000000000)),
            Lock::new(FreeList::new(0x0040000000000000)),
            Lock::new(FreeList::new(0x0080000000000000)),
            Lock::new(FreeList::new(0x0100000000000000)),
            Lock::new(FreeList::new(0x0200000000000000)),
            Lock::new(FreeList::new(0x0400000000000000)),
            Lock::new(FreeList::new(0x0800000000000000)),
            Lock::new(FreeList::new(0x1000000000000000)),
            Lock::new(FreeList::new(0x2000000000000000)),
            Lock::new(FreeList::new(0x4000000000000000)),
            Lock::new(FreeList::new(0x8000000000000000)),
        ],
    };

    core::ptr::write(core_locals_ptr as *mut CoreLocals, core_locals);

    cpu::wrmsr(IA32_GS_BASE, core_locals_ptr as u64);
}
