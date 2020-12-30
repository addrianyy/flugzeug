mod vmcb;

use vmcb::Vmcb;

pub unsafe fn initialize() {
    let mut vmcb = Vmcb::new();
    
    vmcb.control.intercept_cr_reads = 0x1337;

    println!("{:x}", crate::mm::read_phys::<u16>(vmcb.phys_addr()));

    todo!()
}
