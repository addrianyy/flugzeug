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

    let svm_cr = unsafe { cpu::rdmsr(VM_CR_MSR) };
    if  svm_cr & (1 << 4) != 0 {
        return Err("SVM is disabled by the BIOS.");
    }

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

    let mut vmcb = Vmcb::new();
    
    vmcb.control.intercept_cr_reads = 0x1337;

    println!("{:x}", crate::mm::read_phys::<u16>(vmcb.phys_addr()));

    todo!()
}
