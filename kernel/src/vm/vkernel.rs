use super::svm::{Vm, Register, TableRegister, SegmentRegister, DescriptorTable,
                 Segment, VmExit, Intercept, Exception, Interrupt};
use super::svm::npt::{Npt, GuestAddr, PageType};

use page_table::{PageTable, PhysMem, PhysAddr, VirtAddr};

use alloc::vec::Vec;
use alloc::vec;
use core::alloc::Layout;
use core::convert::TryInto;

struct GuestMemory<'a>(&'a mut Npt, &'a mut u64);

impl PhysMem for GuestMemory<'_> {
    fn alloc_phys(&mut self, layout: Layout) -> Option<PhysAddr> {
        // We can only do 4K aligned allocations.
        if layout.align() > 4096 {
            return None;
        }

        let address = *self.1;
        let size    = layout.size() as u64;

        // Increase physcial address for the next allocation.
        *self.1 += size;

        // Map region to guest physical memory as readable and writable.
        self.0.map(GuestAddr(address), PageType::Page4K, size, true, true);

        Some(PhysAddr(address))
    }

    unsafe fn translate(&mut self, phys_addr: PhysAddr, size: usize) -> Option<*mut u8> {
        let next_page   = (phys_addr.0 + 0x1000) & !0xfff;
        let to_page_end = next_page - phys_addr.0;

        // Make sure that region to translate fits in one page.
        if size as u64 > to_page_end {
            return None;
        }

        // Translate guest physical address to host physical address.
        let host_addr = self.0.guest_to_host(GuestAddr(phys_addr.0))?;

        // Translate host physical address to guest virtual address.
        crate::mm::PhysicalMemory.translate(host_addr, size)
    }
}

struct VKernelImage {
    base:        u64,
    entrypoint:  u64,
    image:       Vec<u8>,
    permissions: Vec<u8>,
    rsp:         u64,
}

pub struct VKernel {
    vm:         Vm,
    address:    u64,
    page_table: PageTable,
    image:      VKernelImage,
}

impl VKernel {
    pub fn new() -> Self {
        let image = get_vkernel_image();

        assert!(image.base & 0xfff == 0, "Non page aligned kernel base address.");

        let mut vm           = Vm::new().unwrap();
        let mut address      = 0;
        let mut guest_memory = GuestMemory(vm.npt_mut(), &mut address);
        let page_table       = PageTable::new(&mut guest_memory).unwrap();

        let mut vkernel = Self {
            vm,
            address,
            page_table,
            image,
        };

        vkernel.initialize();

        vkernel
    }

    fn initialize(&mut self) {
        // Setup null IDT and GDT as we rely on descriptor caches.
        self.vm.set_table_reg(TableRegister::Idt, DescriptorTable::null());
        self.vm.set_table_reg(TableRegister::Gdt, DescriptorTable::null());

        // Present data segment with selector 0x10.
        let data_segment = Segment {
            selector: 0x10,
            limit:    0,
            base:     0,
            attrib:   (1 << 7) | (0b10 << 3),
        };

        // Present, 64 bit mode code segment with selector 0x8.
        let code_segment = Segment {
            selector: 0x8,
            limit:    0,
            base:     0,
            attrib:   (1 << 7) | (1 << 9) | (0b11 << 3),
        };

        let null_segment = Segment::null(0);

        self.vm.set_segment_reg(SegmentRegister::Cs,  code_segment);
        self.vm.set_segment_reg(SegmentRegister::Ss,  data_segment);
        self.vm.set_segment_reg(SegmentRegister::Ds,  data_segment);
        self.vm.set_segment_reg(SegmentRegister::Es,  data_segment);
        self.vm.set_segment_reg(SegmentRegister::Gs,  data_segment);
        self.vm.set_segment_reg(SegmentRegister::Fs,  data_segment);
        self.vm.set_segment_reg(SegmentRegister::Tr,  null_segment);
        self.vm.set_segment_reg(SegmentRegister::Ldt, null_segment);

        // LME, LMA, NXE, SVME.
        self.vm.set_reg(Register::Efer, (1 << 8) | (1 << 10) | (1 << 11) | (1 << 12));

        // PE, MP, WP, PG.
        self.vm.set_reg(Register::Cr0, (1 << 0) | (1 << 1) | (1 << 16) | (1 << 31));

        // PAE, OSFXSR, OSXMMEXCPT, OSXSAVE.
        self.vm.set_reg(Register::Cr4, (1 << 5) | (1 << 9) | (1 << 10) | (1 << 18));

        // Everything WB.
        self.vm.set_reg(Register::Pat, 0x0606_0606_0606_0606);

        self.vm.set_reg(Register::Cr3,    self.page_table.table().0);
        self.vm.set_reg(Register::Rip,    self.image.entrypoint);
        self.vm.set_reg(Register::Rsp,    self.image.rsp);
        self.vm.set_reg(Register::Rflags, (1 << 9) | 2);

        self.vm.intercept_all_msrs(true, true);
        self.vm.intercept_all_ports(true);

        self.vm.intercept(&[
            // Intercept relevant exceptions.
            Intercept::Pf,

            // Intercept relevant SVM instructions.
            Intercept::Vmmcall, Intercept::Vmload, Intercept::Vmsave, Intercept::Stgi,
            Intercept::Clgi, Intercept::Skinit, Intercept::Invlpga,

            // Intercept other instructions.
            Intercept::Xsetbv, Intercept::Hlt, Intercept::Invlpgb,

            // Intercept reads and writed of relevant CRs.
            Intercept::Cr0Read, Intercept::Cr0Write,
            Intercept::Cr3Read, Intercept::Cr3Write,
            Intercept::Cr4Read, Intercept::Cr4Write,

            // Intercept all interrupts on the system.
            Intercept::Intr,
            Intercept::Nmi,
            Intercept::Smi,
            Intercept::Init,

            // Intercept other stuff.
            Intercept::FerrFreeze,
        ], true);
    }

    pub fn run(&mut self) -> VmExit {
        loop {
            let (exit, delivery) = unsafe { self.vm.run() };

            if let Some(delivery) = delivery {
                panic!("Intercepted delivery of {:?}.", delivery);
            }

            if let VmExit::Interrupt(Interrupt::Intr) = exit {
                continue;
            }

            if let VmExit::Exception(Exception::Pf { address, error_code }) = exit {
                // We can only handle page faults due to missing page. Other page faults
                // may be caused by for example code writing to read only memory.
                assert!(error_code & (1 << 0) == 0, "Page fault not due to missing page.");

                // Get the address of the beginning of the page.
                let address = VirtAddr(address.0 & !0xfff);

                println!("Page fault. Mapping in 0x{:x}.", address.0);

                let mut guest_memory = GuestMemory(self.vm.npt_mut(), &mut self.address);

                // Get page aligned range of the kernel in virtual memory.
                let kernel_start = self.image.base;
                let kernel_end   = self.image.base +
                    ((self.image.image.len() as u64 + 0xfff) & !0xfff);

                // Check if accessed memory was part of the kernel.
                if address.0 >= kernel_start && address.0 < kernel_end {
                    // Get accessed address page offset from the start of the kernel.
                    let offset = address.0 - kernel_start;

                    // If offset is not page aligned than invalid parameters were supplied.
                    assert!(offset % 4096 == 0, "Non page aligned offset.");

                    // Get permissions for this kernel page. Bitmap contains 2 bits for
                    // every kernel page. LSB corresponds to write bit, MSB corresponds to
                    // execute bit.
                    let page_index = offset / 4096;
                    let perm_index = ((page_index * 2) / 8) as usize;
                    let perm_bit   = ((page_index * 2) % 8) as usize;
                    let perms      = self.image.permissions[perm_index];
                    let write      = perms & (1 << (perm_bit + 0)) != 0;
                    let exec       = perms & (1 << (perm_bit + 1)) != 0;

                    // Get desired contents of this page.
                    let buffer = &self.image.image[offset as usize..];

                    // Map kernel page with appropriate permissions and contents.
                    self.page_table.map_init(&mut guest_memory, address, PageType::Page4K,
                                             4096, write, exec, false, Some(
                        |offset| buffer.get(offset as usize).copied().unwrap_or(0),
                    )).expect("Failed to map kernel memory.");
                } else {
                    // This memory is not part of the kernel. Map it as zeroed, writable and
                    // non-executable.
                    self.page_table.map(&mut guest_memory, address, PageType::Page4K, 4096,
                                        true, false, false)
                        .expect("Failed to map zeroed memory.");
                }
            } else {
                break exit;
            }
        }
    }
}

fn get_vkernel_image() -> VKernelImage {
    const IMAGE: &[u8] = include_bytes!(
        concat!(env!("OUT_DIR"), "/vkernel.bin")
    );

    let base       = u64::from_le_bytes(IMAGE[0.. 8].try_into().unwrap());
    let rsp        = u64::from_le_bytes(IMAGE[8..16].try_into().unwrap());
    let entrypoint = base + 16;

    let image_size  = (IMAGE.len() + 0xfff) & !0xfff;
    let permissions = vec![0b11; (image_size + 3) / 4];

    VKernelImage {
        base,
        entrypoint,
        permissions,
        rsp,
        image: IMAGE.to_vec(),
    }
}
