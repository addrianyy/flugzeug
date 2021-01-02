mod svm_vm;

use svm_vm::{Vm, Register, TableRegister, SegmentRegister, DescriptorTable, Segment};

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
    let mut vm = Vm::new()
        .expect("Failed to create virtual machine.");

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

    vm.set_reg(Register::Rip,    guest_entrypoint as *const () as u64);
    vm.set_reg(Register::Rsp,    rsp);
    vm.set_reg(Register::Rflags, 2);

    //let mut page_table = PageTable::new();

    let vmcb = vm.vmcb_mut();
    vmcb.control.intercept_misc_2 = 1 | 2;
    vmcb.control.intercept_misc_1 = 1 << 31;
    vmcb.control.intercept_exceptions = 0b11111111111;
    //vmcb.control.np_control = 1;
    vmcb.control.guest_asid = 1;

    vm.run();

    println!("Exit reason: 0x{:x}.", vm.vmcb().control.exitcode);
    println!("Running at CPL {}", vm.cpl());
}

unsafe fn guest_entrypoint() -> ! {
    println!("Running in the VM!");

    asm!(r#"
        mov eax, 0x1337
        vmmcall
    "#);

    cpu::halt();
}
