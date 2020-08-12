use page_table::PhysAddr;
use boot_block::{KERNEL_PHYSICAL_REGION_BASE, KERNEL_PHYSICAL_REGION_SIZE};

pub unsafe fn translate(phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
    // Calculate end of region and make sure it doesn't overflow.
    let size = size as u64;
    let end  = size.checked_sub(1).and_then(|size| size.checked_add(phys_addr.0))?;

    // Make sure that we can access this physical region.
    if end >= KERNEL_PHYSICAL_REGION_SIZE {
        return None;
    }

    Some((KERNEL_PHYSICAL_REGION_BASE + phys_addr.0) as *mut u8)
}

pub unsafe fn phys_ref<T>(phys_addr: PhysAddr) -> Option<&'static T> {
    let align = core::mem::align_of::<T>() as u64;

    // Make sure that `phys_addr` is properly aligned.
    if phys_addr.0 & (align - 1) != 0 {
        return None;
    }

    let virt_addr = translate(phys_addr, core::mem::size_of::<T>())?;

    Some(&*(virt_addr as *const T))
}
