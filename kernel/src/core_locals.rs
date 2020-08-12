use core::sync::atomic::{AtomicU64, Ordering};

use boot_block::BootBlock;
use page_table::PhysAddr;
use crate::mm;

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
    // Must be always first field in the structure.
    self_address: usize,

    pub id:         u64,
    pub boot_block: &'static BootBlock,
}

// Make sure that `CoreLocals` is Sync.
trait SyncGuard: Sync + Sized {}
impl SyncGuard for CoreLocals {}

pub unsafe fn initialize(boot_block: PhysAddr) {
    const IA32_GS_BASE: u32 = 0xc0000101;

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
        boot_block,
    };

    core::ptr::write(core_locals_ptr as *mut CoreLocals, core_locals);

    cpu::wrmsr(IA32_GS_BASE, core_locals_ptr as u64);
}
