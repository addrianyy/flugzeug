use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicU64, Ordering};

use page_table::{VirtAddr, PhysAddr, PhysMem, PageType, PAGE_PRESENT, PAGE_WRITE, PAGE_SIZE, PAGE_NX};
use boot_block::{KERNEL_PHYSICAL_REGION_BASE, KERNEL_PHYSICAL_REGION_SIZE,
                 KERNEL_HEAP_BASE, KERNEL_HEAP_PADDING};

pub struct PhysicalMemory;

impl PhysMem for PhysicalMemory {
    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        translate(phys_addr, size)
    }

    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        let mut free_memory = core!().boot_block.free_memory.lock();
        let free_memory     = free_memory.as_mut().unwrap();

        free_memory.allocate(layout.size() as u64, layout.align() as u64)
            .map(|addr| PhysAddr(addr as u64))
    }
}

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

/// Read `T` from an aligned physical address `phys_addr`.
pub unsafe fn read_phys<T>(phys_addr: PhysAddr) -> T {
    let align = core::mem::align_of::<T>() as u64;

    // Make sure that `phys_addr` is properly aligned.
    assert!(phys_addr.0 & (align - 1) == 0, "Physical address {:x} has invalid alignment.",
            phys_addr.0);

    let virt_addr = translate(phys_addr, core::mem::size_of::<T>())
        .expect("Failed to translate address for `read_phys`.");

    core::ptr::read_volatile(virt_addr as *const T)
}

/// Write `value` to an aligned physical address `phys_addr`.
#[allow(unused)]
pub unsafe fn write_phys<T>(phys_addr: PhysAddr, value: T) {
    let align = core::mem::align_of::<T>() as u64;

    // Make sure that `phys_addr` is properly aligned.
    assert!(phys_addr.0 & (align - 1) == 0, "Physical address {:x} has invalid alignment.",
            phys_addr.0);

    let virt_addr = translate(phys_addr, core::mem::size_of::<T>())
        .expect("Failed to translate address for `write_phys`.");

    core::ptr::write_volatile(virt_addr as *mut T, value);
}

/// Read `T` from an unaligned physical address `phys_addr`.
pub unsafe fn read_phys_unaligned<T>(phys_addr: PhysAddr) -> T {
    let virt_addr = translate(phys_addr, core::mem::size_of::<T>())
        .expect("Failed to translate address for `read_phys`.");

    core::ptr::read_unaligned(virt_addr as *const T)
}

/// Write `value` to an unaligned physical address `phys_addr`.
#[allow(unused)]
pub unsafe fn write_phys_unaligned<T>(phys_addr: PhysAddr, value: T) {
    let virt_addr = translate(phys_addr, core::mem::size_of::<T>())
        .expect("Failed to translate address for `write_phys`.");

    core::ptr::write_unaligned(virt_addr as *mut T, value);
}

/// Address of the next free virtual address in the kernel heap.
static NEXT_HEAP_ADDRESS: AtomicU64 = AtomicU64::new(KERNEL_HEAP_BASE);

pub fn reserve_virt_addr(size: usize) -> VirtAddr {
    // Make sure that the requested size is valid.
    assert!(size > 0 && size % 4096 == 0, "Size to reserve is invalid.");

    // Calculate actual size for the region we are reserving.
    let reserve = KERNEL_HEAP_PADDING + size as u64;

    // Reserve the region.
    let address = NEXT_HEAP_ADDRESS.fetch_add(reserve, Ordering::SeqCst);

    // Make sure that we haven't overflowed the heap region.
    address.checked_add(reserve).expect("Heap virtual address overflowed.");

    VirtAddr(address)
}

/// Amount of memory used by the stack metadata in `FreeListNode`.
const STACK_HEADER_SIZE: usize = core::mem::size_of::<usize>() * 2;

#[repr(C)]
struct FreeListNode {
    /// Address of the next `FreeListNode`. 0 if this is the last entry.
    next: usize,

    /// Number of free slots available on the stack. This is only valid if owning free list
    /// uses stack. If it's valid, slots are placed just after end of this structure.
    free_slots: usize,
}

pub struct FreeList {
    /// Pointer to `FreeListNode`. 0 if this free list is empty.
    head: usize,

    /// Allocation size for this free list. Must be >= pointer size and must be power of two.
    size: usize,
}

impl FreeList {
    fn use_stack(&self) -> bool {
        // If allocation size is less than STACK_HEADER_SIZE we won't be able to store required
        // metadata. If allocation size is equal to STACK_HEADER_SIZE we won't get any benefit
        // from using stack. In these cases just use simple linked list.
        // `self.size` is power of two, so the number of bytes left is always divisible
        // by the pointer size.
        self.size > STACK_HEADER_SIZE
    }

    fn slot_ptr(&mut self, node: *mut FreeListNode, slot: usize) -> *mut usize {
        let node_size = core::mem::size_of::<FreeListNode>();

        // Make sure that constant size matches structure size.
        assert!(node_size == STACK_HEADER_SIZE, "Unexpected free list node size.");

        // Make sure that we can actually use stack in this list.
        assert!(self.use_stack(), "Stack cannot be used for this free page list.");

        // Calculate pointer address to the slot. Slot list starts at the end of the node
        // structure. Each slot has a pointer size.
        (node as usize + node_size + slot * core::mem::size_of::<usize>()) as *mut usize
    }

    fn max_slots(&self) -> usize {
        // Make sure that we can actually use stack in this list.
        assert!(self.use_stack(), "Stack cannot be used for this free page list.");

        // Calculate the number of slots that are available in this list.
        // Each slot has a pointer size and we need to subtract the metadata size.
        // `self.size` is power of two, so the number of bytes left is always divisible
        // by the pointer size.
        (self.size - STACK_HEADER_SIZE) / core::mem::size_of::<usize>()
    }

    pub fn new(size: usize) -> Self {
        // Make sure that this free list size is valid.
        assert!(size.count_ones() == 1, "Free list size must be a power of two.");
        assert!(size >= core::mem::size_of::<usize>(),
                "Free list size must be >= pointer size.");

        Self {
            head: 0,
            size,
        }
    }

    /// Put an allocation back to the free memory list.
    pub unsafe fn push(&mut self, virt_addr: *mut u8) {
        if self.use_stack() {
            // We are using a linked list where each node has a stack of free addresses.

            let head_node = self.head as *mut FreeListNode;

            if head_node.is_null() || (*head_node).free_slots == 0 {
                // Either head node is null or head node doesn't have any slots left.
                // We need to create a new node.

                let node = virt_addr as *mut FreeListNode;

                // New nodes start with all slots empty.
                (*node).free_slots = self.max_slots();

                // Insert this node at the beginning of the list.
                (*node).next = self.head;

                self.head = node as usize;
            } else {
                // Head node has enough space to store another virtual address on the stack.

                // Allocate a new slot.
                (*head_node).free_slots -= 1;

                // Store the virtual address in the newly allocated slot.
                *self.slot_ptr(head_node, (*head_node).free_slots) = virt_addr as usize;
            }
        } else {
            // We are using a simple linked list, just insert current virtual address at
            // the beginning of the list.

            let node = virt_addr as *mut FreeListNode;

            (*node).next = self.head;

            self.head = node as usize;
        }
    }

    /// Get an allocation from the free memory list.
    pub unsafe fn pop(&mut self) -> *mut u8 {
        if self.head == 0 {
            // This list is empty, we need to populate it with some memory.

            // Always allocate at least 1 page. `actual_size` will always be page aligned.
            let actual_size = if self.size < 4096 {
                4096
            } else {
                self.size
            };

            // Reserve virtual region.
            let virt_addr = reserve_virt_addr(actual_size);

            let mut page_table = core!().boot_block.page_table.lock();
            let page_table     = page_table.as_mut().unwrap();

            // Map new memory region as readable and writable.
            page_table.map(&mut PhysicalMemory, virt_addr, PageType::Page4K,
                           actual_size as u64, true, false)
                .expect("Failed to map heap memory.");

            if actual_size != self.size {
                // We have overallocated memory and we need to add all regions to the free list,
                // not only one.

                // This should never happen.
                assert!(actual_size % self.size == 0,
                        "Allocated size is not divisible by requested size.");

                // Go through every region and add it to the free list,
                for offset in (0..actual_size).step_by(self.size) {
                    self.push((virt_addr.0 as usize + offset) as *mut u8)
                }
            } else {
                // We have allocated exactly as much memory as requested, we can just
                // return this address.
                return virt_addr.0 as *mut u8;
            }
        }

        assert!(self.head != 0, "Head cannot be empty at this point.");

        let head_node = self.head as *mut FreeListNode;

        if self.use_stack() {
            // We are using a linked list where each node has a stack of free addresses.

            if (*head_node).free_slots == self.max_slots() {
                // Address stack is empty, take the entire node and use is for the allocation.

                // Pop the head node from the list.
                self.head = (*head_node).next;

                head_node as *mut u8
            } else {
                // Take the first free virtual address from the stack.
                let virt_addr = *self.slot_ptr(head_node, (*head_node).free_slots);

                // Update the number of free slots.
                (*head_node).free_slots += 1;

                virt_addr as *mut u8
            }
        } else {
            // We are using a simple linked list, just pop the head node from the list.

            self.head = (*head_node).next;

            head_node as *mut u8
        }
    }
}

pub struct GlobalAllocator;

unsafe impl GlobalAlloc for GlobalAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        core!().free_list(layout).lock().pop()
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        core!().free_list(layout).lock().push(ptr)
    }
}

#[global_allocator]
pub static GLOBAL_ALLOCATOR: GlobalAllocator = GlobalAllocator;

#[alloc_error_handler]
fn alloc_error_handler(layout: Layout) -> ! {
    panic!("Allocation of memory with layout {:?} failed!", layout);
}

unsafe fn enable_nx_on_physical_region() {
    let page_size = core!().boot_block.physical_map_page_size.lock()
        .expect("Bootloader didn't fill in `physical_map_page_size`.");

    const PAGE_2M: u64 = 2 * 1024 * 1024;
    const PAGE_1G: u64 = 1 * 1024 * 1024 * 1024;

    let page_type = match page_size {
        PAGE_2M  => PageType::Page2M,
        PAGE_1G  => PageType::Page1G,
        _        => panic!("Bootloader set invalid physical map page size {:x}.",
                           page_size),
    };

    let mut page_table = core!().boot_block.page_table.lock();
    let page_table     = page_table.as_mut().unwrap();

    // Recreate kernel physical memory map.
    for phys_addr in (0..KERNEL_PHYSICAL_REGION_SIZE).step_by(page_size as usize) {
        // Map current `phys_addr` at virtual address
        // `phys_addr` + `KERNEL_PHYSICAL_REGION_BASE`.
        let virt_addr = VirtAddr(phys_addr + KERNEL_PHYSICAL_REGION_BASE);

        // This physical memory page will be writable. Unlike bootloader, we can now set NX bit.
        let mut raw = phys_addr | PAGE_PRESENT | PAGE_WRITE | PAGE_NX;

        // Set PAGE_SIZE bit if we aren't using standard 4K pages.
        if page_type != PageType::Page4K {
            raw |= PAGE_SIZE;
        }

        // Map the memory. Allow updating existing mappings, disallow creating new tables.
        page_table.map_raw(&mut PhysicalMemory, virt_addr, page_type, raw,
                           false, true)
            .expect("Failed to remap physical region in the kernel page table.");
    }
}

pub unsafe fn on_finished_boot_process() {
    // Make kernel physcial memory map non-executable.
    enable_nx_on_physical_region();

    let mut total_reclaimed = 0;

    // Reclaim boot memory.
    if let Some(boot_memory) = core!().boot_block.boot_memory.lock().take() {
        for &entry in boot_memory.entries() {
            let size = entry.size();

            core!().boot_block.free_memory
                .lock()
                .as_mut()
                .unwrap()
                .insert(entry);

            total_reclaimed += size;
        }
    }

    let mut total_free = 0;

    // Sum up all free memory.
    if let Some(free_memory) = core!().boot_block.free_memory.lock().as_ref() {
        for entry in free_memory.entries() {
            total_free += entry.size();
        }
    }

    println!("Reclaimed {} of boot memory. {} of available memory.",
             Memory(total_reclaimed), Memory(total_free));
}

#[allow(unused)]
pub fn dump_memory_ranges() {
    let entries  = {
        let free_memory = core!().boot_block.free_memory.lock()
            .as_ref()
            .unwrap()
            .clone();

        let mut entries = alloc::vec::Vec::new();

        for entry in free_memory.entries() {
            entries.push((entry.start, entry.end + 1));
        }

        entries.sort_by(|(s1, _), (s2, _)| s1.cmp(&s2));

        entries
    };

    for (start, end) in entries {
        println!("{:010x} - {:010x} ({})", start, end, Memory(end - start));
    }
}

struct Memory(u64);

impl core::fmt::Display for Memory {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        let kb = self.0 as f64 / 1024.0;
        let mb = kb / 1024.0;
        let gb = mb / 1024.0;

        const TRESHOLD: f64 = 0.8;

        macro_rules! write_unit {
            ($amount: expr, $name: expr) => {
                if $amount >= TRESHOLD {
                    let total = $amount as u64 as f64;
                    let diff  = $amount - total;
                    if  diff >= 0.1 {
                        return write!(f, "{:.1} {}", $amount, $name);
                    } else {
                        return write!(f, "{} {}", total, $name);
                    }
                }
            }
        }

        write_unit!(gb, "GB");
        write_unit!(mb, "MB");
        write_unit!(kb, "KB");

        write!(f, "{} B", self.0)
    }
}
