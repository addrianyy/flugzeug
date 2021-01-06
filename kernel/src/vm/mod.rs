mod svm;

use svm::{Vm, Register, TableRegister, SegmentRegister, DescriptorTable,
          Segment, VmExit, Intercept, Exception};
use svm::npt::{self, Npt, GuestAddr, PageType};

use page_table::{PageTable, PhysMem, PhysAddr, VirtAddr};

use alloc::vec::Vec;
use core::alloc::Layout;

fn load_segment(vm: &mut Vm, register: SegmentRegister, selector: u16) {
    // Make sure that selector's table bit is 0.
    assert!(selector & 0b100 == 0, "Segments in LDT are not supported.");

    // Mask off RPL from the selector to get the segment offset.
    let offset = selector & !0b11;

    let segment = if offset == 0 {
        Segment::null(selector)
    } else {
        let gdtr     = cpu::get_gdt();
        let gdt_base = gdtr.base as *mut u64;

        assert!(offset < gdtr.limit, "Selector offset is outside the GDT.");

        unsafe {
            // Get the GDT entry for this selector.
            let ptr   = gdt_base.add(offset as usize / 8);
            let entry = *ptr;

            let mut attribs = 0;
            let mut limit   = 0;
            let mut base    = 0;

            // Extract segment attributes.
            attribs |= ((entry >> 40) & 0xff) << 0;
            attribs |= ((entry >> 52) & 0x0f) << 8;

            // Extract segment limit.
            limit |= ((entry >>  0) & 0xffff) << 0;
            limit |= ((entry >> 48) & 0x000f) << 16;

            // Extract segment base address.
            base |= ((entry >> 16) & 0xff_ffff) << 0;
            base |= ((entry >> 56) & 0x00_00ff) << 24;

            if entry & (1 << 44) == 0 {
                // System segments have 64 bit base address.
                base |= (*ptr.add(1) & 0xffff_ffff) << 32;
            }

            // Load base address from MSRs for FS and GS.
            match register {
                SegmentRegister::Fs => base = cpu::rdmsr(0xc000_0100),
                SegmentRegister::Gs => base = cpu::rdmsr(0xc000_0101),
                _                   => (),
            }

            Segment {
                selector,
                base,
                limit:  limit as u32,
                attrib: attribs as u16,
            }
        }
    };

    vm.set_segment_reg(register, segment);
}

pub unsafe fn initialize() {
    println!("Starting VM...");

    let mut vm = Vm::new()
        .expect("Failed to create virtual machine");

    // Allocate stack for the guest.
    let mut stack = alloc::vec![0u8; 1024 * 1024];
    let rsp       = (stack.as_mut_ptr() as u64) + (stack.len() as u64);
    let rsp       = (rsp & !0xf) - 0x100;

    // Use null TR and LDT.
    load_segment(&mut vm, SegmentRegister::Tr,  0);
    load_segment(&mut vm, SegmentRegister::Ldt, 0);

    // Copy user segments from the host.
    load_segment(&mut vm, SegmentRegister::Cs, cpu::get_cs());
    load_segment(&mut vm, SegmentRegister::Ss, cpu::get_ss());
    load_segment(&mut vm, SegmentRegister::Ds, cpu::get_ds());
    load_segment(&mut vm, SegmentRegister::Es, cpu::get_es());
    load_segment(&mut vm, SegmentRegister::Gs, cpu::get_gs());
    load_segment(&mut vm, SegmentRegister::Fs, cpu::get_fs());

    // Use null IDT.
    vm.set_table_reg(TableRegister::Idt, DescriptorTable::null());

    // Copy GDT from the host.
    vm.set_table_reg(TableRegister::Gdt, {
        let gdt = cpu::get_gdt();

        DescriptorTable {
            base:  gdt.base as u64,
            limit: gdt.limit,
        }
    });

    // Setup register state for the VM.
    vm.set_reg(Register::Efer,         cpu::rdmsr(0xc000_0080));
    vm.set_reg(Register::Cr0,          cpu::get_cr0() as u64);
    vm.set_reg(Register::Cr2,          0);
    vm.set_reg(Register::Cr3,          cpu::get_cr3() as u64);
    vm.set_reg(Register::Cr4,          cpu::get_cr4() as u64);
    vm.set_reg(Register::Star,         cpu::rdmsr(0xc000_0081));
    vm.set_reg(Register::Lstar,        cpu::rdmsr(0xc000_0082));
    vm.set_reg(Register::Cstar,        cpu::rdmsr(0xc000_0083));
    vm.set_reg(Register::Sfmask,       cpu::rdmsr(0xc000_0084));
    vm.set_reg(Register::KernelGsBase, cpu::rdmsr(0xc000_0102));
    vm.set_reg(Register::SysenterCs,   cpu::rdmsr(0x174));
    vm.set_reg(Register::SysenterEsp,  cpu::rdmsr(0x175));
    vm.set_reg(Register::SysenterEip,  cpu::rdmsr(0x176));
    vm.set_reg(Register::Pat,          cpu::rdmsr(0x277));
    vm.set_reg(Register::Dr6,          0xffff_0ff0);
    vm.set_reg(Register::Dr7,          0x0000_0400);
    vm.set_reg(Register::Rip,          guest_entrypoint as *const () as u64);
    vm.set_reg(Register::Rsp,          rsp);
    vm.set_reg(Register::Rflags,       2);

    vm.intercept(&[
        // Intercept relevant SVM instructions.
        Intercept::Vmmcall, Intercept::Stgi, Intercept::Clgi, Intercept::Skinit,
        Intercept::Invlpga,

        // Intercept other instructions.
        Intercept::Xsetbv, Intercept::Hlt,

        // Intercept all interrupts on the system.
        Intercept::Intr,
        Intercept::Nmi,
        Intercept::Smi,
        Intercept::Init,

        // Intercept other stuff.
        Intercept::FerrFreeze,
    ]);

    let mut mapped_pages = 0;

    'run: loop {
        let exit = vm.run();

        match exit {
            VmExit::NestedPageFault { address, .. } => {
                let phys_addr = address.0 & !0xfff;
                let raw       = phys_addr | npt::NPT_PRESENT | npt::NPT_WRITE;

                vm.npt_mut().map_raw(GuestAddr(phys_addr), PageType::Page4K,
                                     raw, true, false);

                mapped_pages += 1;

                continue 'run;
            }
            VmExit::Vmmcall => {
                println!("vmmcall: {:#x}.", vm.reg(Register::Rax));

                vm.set_reg(Register::Rip, vm.next_rip());

                continue 'run;
            }
            VmExit::Hlt => {}
            _           => panic!("Unhandled VM exit {:x?}.", exit),
        }

        break 'run;
    }

    println!("Done! Mapped in {} pages.", mapped_pages);

    let mut vkernel = VKernel::new();

    let exit = vkernel.run();

    println!("RBX: {:x}", vkernel.vm.reg(Register::Rbx));
    println!("{:x?}", exit);
}

unsafe fn guest_entrypoint() -> ! {
    println!("Running in the VM!");

    asm!(r#"
        mov eax, 0x1337
        vmmcall
    "#);

    cpu::halt();
}

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
}

struct VKernel {
    vm:         Vm,
    address:    u64,
    page_table: PageTable,
    image:      VKernelImage,
}

impl VKernel {
    fn new() -> Self {
        let image = VKernelImage {
            base:       0x13_3700_0000,
            entrypoint: 0x13_3700_0000,
            image:      alloc::vec![
                0x49, 0xBF, 0x8F, 0x67, 0x45, 0x23, 0xF1, 0xDE, 0xBC, 0x0A, 0x48, 0xC7, 0xC4,
                0x00, 0x00, 0x00, 0x0C, 0x5B, 0x48, 0xFF, 0xC3, 0x0F, 0x01, 0xD9,
            ],
            permissions: alloc::vec![0b10],
        };

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
            attrib:   1 << 7,
        };

        // Present, 64 bit mode code segment with selector 0x8.
        let code_segment = Segment {
            selector: 0x8,
            limit:    0,
            base:     0,
            attrib:   (1 << 7) | (1 << 9),
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
        self.vm.set_reg(Register::Rsp,    0);
        self.vm.set_reg(Register::Rflags, 2);

        self.vm.intercept(&[
            // Intercept relevant exceptions.
            Intercept::Pf,

            // Intercept relevant SVM instructions.
            Intercept::Vmmcall, Intercept::Stgi, Intercept::Clgi, Intercept::Skinit,
            Intercept::Invlpga,

            // Intercept other instructions.
            Intercept::Xsetbv, Intercept::Hlt,

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
        ]);
    }

    fn run(&mut self) -> VmExit {
        loop {
            let exit = unsafe { self.vm.run() };

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
