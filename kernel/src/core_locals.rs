use core::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use core::alloc::Layout;
use core::alloc::GlobalAlloc;

use boot_block::BootBlock;
use page_table::PhysAddr;
use lock::Lock;

use crate::interrupts::Interrupts;
use crate::apic::{Apic, ApicMode};
use crate::mm::{self, FreeList, PhysicalPage};

static NEXT_FREE_CORE_ID: AtomicU64 = AtomicU64::new(0);

#[macro_export]
macro_rules! core {
    () => { $crate::core_locals::get_core_locals() }
}

#[inline]
pub fn get_raw_core_locals() -> usize {
    let core_locals: usize;

    unsafe {
        asm!("mov {}, gs:[0]", out(reg) core_locals);
    }

    core_locals
}

#[inline]
pub fn get_core_locals() -> &'static CoreLocals {
    unsafe {
        &*(crate::core_locals::get_raw_core_locals() as *const CoreLocals)
    }
}

#[repr(C)]
pub struct CoreLocals {
    /// Must be always the first field in the structure.
    /// Required offset: 0.
    /// DON'T CHANGE!
    self_address: usize,

    /// Must be always the second field in the structure.
    /// Required offset: 8.
    /// DON'T CHANGE!
    xsave_area: usize,

    /// Required size of the XSAVE area.
    xsave_size: AtomicUsize,

    /// Unique identifier for this CPU. 0 is BSP.
    pub id: u64,

    /// Data shared between bootloader and kernel.
    pub boot_block: &'static BootBlock,

    /// Local APIC for this core.
    pub apic: Lock<Option<Apic>>,

    /// Interrupt handlers for this core.
    pub interrupts: Lock<Option<Interrupts>>,

    /// TSC when this core entered bootloader.
    pub boot_tsc: u64,

    /// 4KB of space for SVM to save host state on `vmrun`.
    pub host_save_area: Lock<Option<PhysicalPage<[u8; 4096]>>>,

    /// APIC ID for this core. !0 if not cached yet.
    apic_id: AtomicU32,

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

    pub unsafe fn set_apic_id(&self, apic_id: u32) {
        self.apic_id.store(apic_id, Ordering::SeqCst);
    }

    pub fn apic_id(&self) -> Option<u32> {
        // Return `None` if the APIC ID isn't cached yet.
        match self.apic_id.load(Ordering::SeqCst) {
            0xffff_ffff => None,
            x           => Some(x),
        }
    }

    pub fn apic_mode(&self) -> ApicMode {
        core!().apic.lock()
            .as_ref()
            .expect("Cannot get APIC mode before initializing APIC.")
            .mode()
    }

    #[allow(unused)]
    pub fn xsave_size(&self) -> usize {
        let xsave_size = self.xsave_size.load(Ordering::Relaxed);

        assert!(xsave_size > 0, "XSAVE size not calculated yet.");

        xsave_size
    }
}

// Make sure that `CoreLocals` is Sync.
trait SyncGuard: Sync + Sized {}
impl  SyncGuard for CoreLocals {}

pub unsafe fn initialize(boot_block: PhysAddr, boot_tsc: u64) {
    const IA32_GS_BASE: u32 = 0xc0000101;

    // Make sure that core locals haven't been initialized yet.
    assert!(cpu::rdmsr(IA32_GS_BASE) == 0, "Core locals were already initialized.");

    // Make sure that we got valid boot block from the bootloader.
    assert!(boot_block.0 != 0, "Boot block is null.");

    let core_id    = NEXT_FREE_CORE_ID.fetch_add(1, Ordering::SeqCst);
    let boot_block = mm::phys_ref::<BootBlock>(boot_block).unwrap();

    // Make sure that structure size is the same in 32 bit and 64 bit mode.
    assert!(boot_block.size == core::mem::size_of::<BootBlock>() as u64,
            "Boot block size mismatch.");

    let core_locals_ptr = {
        let size  = core::mem::size_of::<CoreLocals>()  as u64;
        let align = core::mem::align_of::<CoreLocals>() as u64;

        // Validate `CoreLocals` constrains.
        assert!(align <= 4096, "`CoreLocals` must have alignment <= 4096 bytes.");
        assert!(size  > 0, "`CoreLocals` size must be > 0.");

        // Align the size and reserve virtual region for `CoreLocals`.
        let aligned_size = (size + 0xfff) & !0xfff;
        let virt_addr    = mm::reserve_virt_addr(aligned_size as usize);

        let mut page_table = boot_block.page_table.lock();
        let page_table     = page_table.as_mut().unwrap();

        // Normal `PhysicalMemory` uses `CoreLocals` so we cannot use it.
        struct EarlyPhysicalMemory(&'static BootBlock);

        impl page_table::PhysMem for EarlyPhysicalMemory {
            unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
                mm::translate(phys_addr, size)
            }

            fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
                unsafe {
                    mm::alloc_phys(self.0, layout)
                }
            }
        }

        // Allocate `CoreLocals` and map it to virtual memory.
        page_table.map(&mut EarlyPhysicalMemory(boot_block), virt_addr,
                       page_table::PageType::Page4K, aligned_size, true, false, false)
            .expect("Failed to map `CoreLocals`.");

        virt_addr.0 as usize
    };

    let core_locals = CoreLocals {
        boot_tsc,
        self_address:   core_locals_ptr,
        xsave_area:     0,
        xsave_size:     AtomicUsize::new(0),
        id:             core_id,
        apic:           Lock::new(None),
        apic_id:        AtomicU32::new(!0),
        interrupts:     Lock::new(None),
        host_save_area: Lock::new(None),
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

    initialize_xsave();
}

unsafe fn initialize_xsave() {
    // We start with legacy area size and XSAVE header size.
    let mut xsave_size = 512 + 64;

    // IA32_XSS is not used.
    let xcr0 = cpu::get_xcr0();

    for component in 2..64 {
        if xcr0 & (1 << component) != 0 {
            let cpuid  = cpu::cpuid(0x0d, component);
            let offset = cpuid.ebx;
            let size   = cpuid.eax;

            xsave_size = xsave_size.max(offset + size);
        }
    }

    // Allocate the XSAVE area.
    let xsave_size   = xsave_size as usize;
    let xsave_layout = Layout::from_size_align(xsave_size, 64)
        .expect("Failed to create XSAVE layout.");
    let xsave_area   = mm::GLOBAL_ALLOCATOR.alloc(xsave_layout);

    assert!(!xsave_area.is_null(), "Failed to allocate XSAVE area.");

    // Zero out XSAVE area as required by the architecture.
    core::ptr::write_bytes(xsave_area, 0, xsave_size);

    // Manually get core locals so we get a mutable reference.
    let core_locals = get_raw_core_locals() as *mut CoreLocals;

    // Save address of XSAVE area.
    (*core_locals).xsave_area = xsave_area as usize;

    // Save size of XSAVE area.
    (*core_locals).xsave_size.store(xsave_size, Ordering::Relaxed);
}
