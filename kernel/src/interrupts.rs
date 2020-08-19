use alloc::{vec, vec::Vec, boxed::Box};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptFrame {
    pub rip:    u64,
    pub cs:     u64,
    pub rflags: u64,
    pub rsp:    u64,
    pub ss:     u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct RegisterState {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9:  u64,
    pub r8:  u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
}

pub struct Interrupts {
    _idt: Box<[IdtGate]>,
    _gdt: Box<[u64]>,
}

#[derive(Copy, Clone)]
#[repr(C, packed)]
struct TableRegister {
    limit: u16,
    base:  u64,
}

#[derive(Copy, Clone)]
#[repr(C, align(16))]
struct IdtGate {
    data: [u32; 4],
}

impl IdtGate {
    pub fn new(present: bool, cs: u16, offset: u64, gate_type: u32, dpl: u32, ist: u32) -> Self {
        assert!(dpl       <= 3,  "Invalid DPL.");
        assert!(ist       <= 7,  "Invalid IST.");
        assert!(gate_type <= 31, "Invalid gate type.");

        let mut data = [0u32; 4];

        data[0] |= (offset as u32) & 0xffff;
        data[0] |= (cs     as u32) << 16;

        data[1] |= offset as u32 & 0xffff0000;
        data[1] |= ist;
        data[1] |= gate_type << 8;
        data[1] |= dpl << 13;
        data[1] |= (present as u32) << 15;

        data[2] |= (offset >> 32) as u32;

        Self {
            data
        }
    }
}

pub unsafe fn initialize() {
    let mut interrupts = core!().interrupts.lock();

    // Make sure that the interrupts haven't been initialized yet.
    assert!(interrupts.is_none(), "Interrupts were already initialized.");

    let gdt = vec![
        0x0000000000000000u64, // Null segment.
        0x00209a0000000000u64, // Code segment - 64 bit.
        0x0000920000000000u64, // Data segment - 64 bit.
    ].into_boxed_slice();

    // Create a GDTR that will point to the newly created GDT.
    let gdtr = TableRegister {
        base:  gdt.as_ptr() as u64,
        limit: core::mem::size_of_val(&gdt[..]) as u16 - 1,
    };

    // Load new GDT.
    asm!("lgdt [{}]", in(reg) &gdtr);

    let mut idt = Vec::with_capacity(256);

    // Fill whole interrupt table with gates that point to the `handle_interrupt` wrappers.
    for int in 0..256 {
        // Add an interrupt gate that will preprocess interrupt and jump to the `handle_interrupt`.
        idt.push(IdtGate::new(true, 0x08, crate::interrupts_misc::INTERRUPT_HANDLERS[int] as u64,
                              0xe, 0, 0));
    }

    let idt = idt.into_boxed_slice();

    // Create a IDTR that will point to the newly created IDT.
    let idtr = TableRegister {
        base:  idt.as_ptr() as u64,
        limit: core::mem::size_of_val(&idt[..]) as u16 - 1,
    };

    // Load new IDT.
    asm!("lidt [{}]", in(reg) &idtr);

    *interrupts = Some(Interrupts {
        _idt: idt,
        _gdt: gdt,
    });
}

#[no_mangle]
extern "C" fn handle_interrupt(int: u8, frame: &mut InterruptFrame, _error: u64,
                               regs: &mut RegisterState) {
    println!("Unexpected interrupt {}.", int);
    println!("Interrupt frame: {:#x?}", frame);
    println!("Register state: {:#x?}", regs);

    cpu::halt();
}
