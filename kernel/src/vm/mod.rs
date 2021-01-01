mod svm_vm;

use svm_vm::{Vm, Register, VmcbSegmentDescriptor};

fn load_segment(selector: u16, segment: &mut VmcbSegmentDescriptor) {
    segment.selector = selector;

    // Make sure that selector's table bit is 0.
    assert!(selector & 0b100 == 0, "Segments in LDT are not supported.");

    // Mask off RPL from the selector to get the segment offset.
    let offset = selector & !0b11;
    if  offset == 0 {
        // For NULL segments, set all attribute bits to zero. Other fields should be zeroed too.
        segment.base     = 0;
        segment.limit    = 0;
        segment.attrib   = 0;

        return;
    }

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

        segment.base   = base;
        segment.limit  = limit as u32;
        segment.attrib = attribs as u16;
    }
}

unsafe fn run_guest() {
    println!("Running in the VM!");

    asm!("mov eax, 0x1337\nvmmcall");

    cpu::halt();
}

#[repr(C, align(16))]
#[derive(Copy, Clone)]
struct StackElement([u8; 16]);

pub unsafe fn initialize() {
    //let mut vmcb = Vmcb::new();

    let mut vm = Vm::new().unwrap();
    let vmcb = vm.vmcb_mut();

    // Copy GDTR from the host.
    {
        let gdt = cpu::get_gdt();

        vmcb.state.gdtr.base  = gdt.base  as u64;
        vmcb.state.gdtr.limit = gdt.limit as u32;
    }

    // Use null TR, LDT and IDT.
    load_segment(0, &mut vmcb.state.tr);
    load_segment(0, &mut vmcb.state.ldtr);
    load_segment(0, &mut vmcb.state.idtr);

    // Copy user segments from the host.
    load_segment(cpu::get_cs(), &mut vmcb.state.cs);
    load_segment(cpu::get_ss(), &mut vmcb.state.ss);
    load_segment(cpu::get_ds(), &mut vmcb.state.ds);
    load_segment(cpu::get_es(), &mut vmcb.state.es);
    load_segment(cpu::get_gs(), &mut vmcb.state.gs);
    load_segment(cpu::get_fs(), &mut vmcb.state.fs);

    // Copy GS and FS base from the MSRs.
    vmcb.state.gs.base = cpu::rdmsr(0xc000_0101);
    vmcb.state.fs.base = cpu::rdmsr(0xc000_0100);

    // Copy control registers from the host.
    vmcb.state.cpl            = 0;
    vmcb.state.efer           = cpu::rdmsr(0xc000_0080);
    vmcb.state.cr4            = cpu::get_cr4() as u64;
    vmcb.state.cr3            = cpu::get_cr3() as u64;
    vmcb.state.cr0            = cpu::get_cr0() as u64;
    vmcb.state.star           = cpu::rdmsr(0xc000_0081);
    vmcb.state.lstar          = cpu::rdmsr(0xc000_0082);
    vmcb.state.lstar          = cpu::rdmsr(0xc000_0083);
    vmcb.state.sfmask         = cpu::rdmsr(0xc000_0084);
    vmcb.state.kernel_gs_base = cpu::rdmsr(0xc000_0102);
    vmcb.state.sysenter_cs    = cpu::rdmsr(0x174);
    vmcb.state.sysenter_esp   = cpu::rdmsr(0x175);
    vmcb.state.sysenter_eip   = cpu::rdmsr(0x176);
    vmcb.state.g_pat          = cpu::rdmsr(0x277);

    // Set some registers to initial processor state.
    vmcb.state.dr6    = 0xffff_0ff0;
    vmcb.state.dr7    = 0x0000_0400;
    vmcb.state.cr2    = 0;

    let mut stack = alloc::vec![StackElement([0; 16]); 1024 * 128];
    let rsp       = (stack.as_mut_ptr() as usize + (stack.len() * 16) - 16) as u64;

    vmcb.control.intercept_misc_2 = 1 | 2;
    vmcb.control.intercept_misc_1 = 1 << 31;
    vmcb.control.guest_asid = 1;

    vm.set_reg(Register::Rip, run_guest as *const () as u64);
    vm.set_reg(Register::Rsp, rsp);
    vm.set_reg(Register::Rflags, 2);

    vm.run();

    println!("{:x}", vm.vmcb().control.exitcode);
}
