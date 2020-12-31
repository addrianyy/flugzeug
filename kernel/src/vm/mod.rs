mod vmcb;

use vmcb::Vmcb;

const VM_CR_MSR: u32 = 0xc001_0114;
const EFER_MSR:  u32 = 0xc000_0080;

fn enable_svm() -> Result<(), &'static str> {
    {
        let cpuid      = cpu::cpuid(0, 0);
        let mut vendor = [0u8; 3 * 4];

        vendor[0.. 4].copy_from_slice(&cpuid.ebx.to_le_bytes());
        vendor[4.. 8].copy_from_slice(&cpuid.edx.to_le_bytes());
        vendor[8..12].copy_from_slice(&cpuid.ecx.to_le_bytes());

        if &vendor != b"AuthenticAMD" {
            return Err("CPU vendor is not `AuthenticAMD`.");
        }
    }

    if !cpu::get_features().svm {
        return Err("CPUID reported that SVM is not supported.");
    }

    /*
    let svm_cr = unsafe { cpu::rdmsr(VM_CR_MSR) };
    if  svm_cr & (1 << 4) != 0 {
        return Err("SVM is disabled by the BIOS.");
    }
    */

    // SVM is available, enable it.
    unsafe {
        cpu::wrmsr(EFER_MSR, cpu::rdmsr(EFER_MSR) | (1 << 12));
    }

    Ok(())
}

pub unsafe fn initialize() {
    // Try to enable SVM extensions.
    if let Err(error) = enable_svm() {
        color_println!(0xffcc00, "Cannot create VM: {}", error);
        return;
    }

    /*
    let mut vmcb = Vmcb::new();

    vmcb.state.cs = vmcb::VmcbSegmentDescriptor { selector: 8, base: 0, limit: 0xffff, attrib: 0x93 };
    vmcb.state.ss = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0x93 };
    vmcb.state.ds = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0x93 };
    vmcb.state.es = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0x93 };
    vmcb.state.gs = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0x93 };
    vmcb.state.fs = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0x93 };

    vmcb.state.gdtr = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0 };
    vmcb.state.idtr = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0, attrib: 0 };
    vmcb.state.tr = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0 };
    vmcb.state.ldtr = vmcb::VmcbSegmentDescriptor { selector: 0, base: 0, limit: 0xffff, attrib: 0 };

    vmcb.state.rflags = 2;
    vmcb.state.cr0 = 0x0000_0000_6000_0010;
    vmcb.state.efer = 1 << 12;
    vmcb.control.intercept_instructions_2 = 1;
    vmcb.control.intercept_instructions_1 = 1 << 31;
    vmcb.control.guest_asid = 1;

    let save = crate::mm::PhysicalPage::new([0u8; 4096]);
    use page_table::PhysAddr;

    cpu::wrmsr(0xc001_0117, save.phys_addr().0);

    println!("Running! {:x?}", crate::mm::read_phys::<[u8; 4]>(PhysAddr(0)));

    crate::mm::write_phys(PhysAddr(0), 0xccu8);

    asm!("vmrun rax", in("rax") vmcb.phys_addr().0);

    {
        let mut sp =  unsafe { serial_port::SerialPort::new() };
        use core::fmt::Write;
        writeln!(sp, "EXITTTT");
    }

    println!("{:x}", vmcb.control.exitcode);
    */

    todo!()
}
