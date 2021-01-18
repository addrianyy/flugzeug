mod svm;
mod vkernel;

use svm::{Vm, Register, TableRegister, SegmentRegister, DescriptorTable,
          Segment, VmExit, Intercept, Interrupt};
use svm::npt::{self, GuestAddr, PageType};

use vkernel::VKernel;

unsafe fn guest_entrypoint() -> ! {
    // If printing in interrupts is enabled we will mess up guest and host interrupt state.
    if !crate::interrupts::PRINT_IN_INTERRUPTS {
        println!("Running in the VM! Uptime: {:.2}s.", crate::time::uptime());
    }

    asm!(r#"
        mov eax, 0x1337
        vmmcall
    "#);

    cpu::halt();
}

fn load_segment(vm: &mut Vm, register: SegmentRegister, selector: u16) {
    // Make sure that selector's table bit is 0.
    assert!(selector & 0b100 == 0, "Segments in LDT are not supported.");

    // Mask off RPL from the selector to get the segment offset.
    let offset = selector & !0b11;

    let mut segment = if offset == 0 {
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

            Segment {
                selector,
                base,
                limit:  limit   as u32,
                attrib: attribs as u16,
            }
        }
    };

    unsafe {
        // Load base address from MSRs for FS and GS.
        match register {
            SegmentRegister::Fs => segment.base = cpu::rdmsr(0xc000_0100),
            SegmentRegister::Gs => segment.base = cpu::rdmsr(0xc000_0101),
            _                   => (),
        }
    }

    vm.set_segment_reg(register, segment);
}

fn run_kernel_in_vm() {
    println!();
    println!("Running the kernel in the VM...");

    let mut vm = Vm::new()
        .expect("Failed to create virtual machine");

    // Allocate stack for the guest.
    let mut stack = alloc::vec![0u8; 1024 * 1024];
    let rsp       = (stack.as_mut_ptr() as u64) + (stack.len() as u64);
    let rsp       = (rsp & !0xf) - 0x48;

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

    unsafe {
        // Setup register state for the VM.
        vm.set_reg(Register::Efer,         cpu::rdmsr(0xc000_0080));
        vm.set_reg(Register::Cr0,          cpu::get_cr0() as u64);
        vm.set_reg(Register::Cr2,          0);
        vm.set_reg(Register::Cr3,          cpu::get_cr3() as u64);
        vm.set_reg(Register::Cr4,          cpu::get_cr4() as u64);
        vm.set_reg(Register::Xcr0,         cpu::get_xcr0());
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
    }

    vm.intercept(&[Intercept::Vmmcall, Intercept::Hlt, Intercept::FerrFreeze], true);

    let mut mapped_pages = 0;
    let mut interrupts   = 0;

    'run: loop {
        let (exit, delivery) = unsafe { vm.run() };
        let mut unhandled    = false;

        match exit {
            VmExit::NestedPageFault { address, .. } => {
                let phys_addr = address.0 & !0xfff;
                let raw       = phys_addr | npt::NPT_PRESENT | npt::NPT_WRITE;

                unsafe {
                    vm.npt_mut().map_raw(GuestAddr(phys_addr), PageType::Page4K,
                                         raw, true, false);
                }

                mapped_pages += 1;
            }
            VmExit::Vmmcall => {
                println!("vmmcall: {:#x}.", vm.reg(Register::Rax));

                vm.set_reg(Register::Rip, vm.next_rip());
            }
            VmExit::Hlt      => break 'run,
            VmExit::Shutdown => {
                panic!("VM shutdown because of event: {:?}", delivery.unwrap());
            }
            VmExit::Interrupt(Interrupt::Intr) => {
                interrupts += 1;
                continue 'run;
            }
            _ => unhandled = true,
        }

        if unhandled {
            println!("Unhandled VM exit {:x?}.", exit)
        }

        if let Some(delivery) = delivery {
            println!("Intercepted delivery of {:x?}.", delivery);
        }

        if unhandled || delivery.is_some() {
            panic!("VM failure.");
        }
    }

    println!("Done! Mapped in {} pages. Interrupted {} times.", mapped_pages, interrupts);
}

fn run_vkernel_in_vm() {
    println!();
    println!("Running the VKernel in the VM...");

    let mut vkernel = VKernel::new();
    let exit        = vkernel.run();

    println!("VKernel exited: {:x?}.", exit);

    assert!(vkernel.vm.reg(Register::Xcr0) == 3);
}

pub unsafe fn initialize() {
    run_kernel_in_vm();
    run_vkernel_in_vm();
}
