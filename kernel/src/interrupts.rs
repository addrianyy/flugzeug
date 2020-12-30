use core::alloc::{GlobalAlloc, Layout};
use alloc::{vec, vec::Vec, boxed::Box};

use cpu::TableRegister;

use crate::{mm, panic};

pub struct Interrupts {
    _idt: Box<[IdtGate]>,
    _gdt: Box<[u64]>,
    _tss: Box<Tss>,
}

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

#[repr(C, packed)]
#[derive(Clone, Copy, Default)]
struct Tss {
    reserved1:   u32,
    rsp:         [u64; 3],
    reserved2:   u64,
    ist:         [u64; 7],
    reserved3:   u64,
    reserved4:   u16,
    iopb_offset: u16,
}

#[repr(C, align(16))]
#[derive(Copy, Clone)]
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
    const EMERGENCY_STACK_SIZE: usize = 32 * 1024;

    let mut interrupts = core!().interrupts.lock();

    // Make sure that the interrupts haven't been initialized yet.
    assert!(interrupts.is_none(), "Interrupts were already initialized.");

    // Allocate an emergency stack for certain interrupts.
    let emergency_stack = mm::GLOBAL_ALLOCATOR.alloc(
        Layout::from_size_align(EMERGENCY_STACK_SIZE, 4096).unwrap()) as u64;

    // Create a TSS which will reference our emergency stack.
    let mut tss = Box::new(Tss::default());

    tss.ist[0] = emergency_stack + EMERGENCY_STACK_SIZE as u64;

    let mut gdt = vec![
        0x0000000000000000u64, // Null segment.
        0x00209a0000000000u64, // Code segment - 64 bit.
        0x0000920000000000u64, // Data segment - 64 bit.
    ];

    // Get the selector of the TSS.
    let tss_selector: u16 = core::mem::size_of_val(&gdt[..]) as u16;

    let tss_base  = &*tss as *const Tss as u64;
    let tss_limit = core::mem::size_of::<Tss>() as u64 - 1;

    // Create the TSS entry for the GDT.
    let tss_high = tss_base >> 32;
    let tss_low  = 0x8900_0000_0000 | (((tss_base >> 24) & 0xff) << 56) |
        ((tss_base & 0xffffff) << 16) | tss_limit;

    // Add the TSS entry into the GDT.
    gdt.push(tss_low);
    gdt.push(tss_high);

    // Make sure that the GDT won't get relocated.
    let gdt = gdt.into_boxed_slice();

    // Create a GDTR that will point to the newly created GDT.
    let gdtr = TableRegister {
        base:  gdt.as_ptr() as usize,
        limit: core::mem::size_of_val(&gdt[..]) as u16 - 1,
    };

    // Load new GDT.
    cpu::set_gdt(&gdtr);

    // Load new TSS.
    cpu::set_tr(tss_selector);

    let mut idt = Vec::with_capacity(256);

    // Fill whole interrupt table with gates that point to the `handle_interrupt` wrappers.
    for int in 0..256 {
        let ist = match int {
            2 | 8 | 18 => {
                // Use emergency stack for NMI, #DF, #MC.
                1
            }
            _ => 0,
        };

        let address = crate::interrupts_misc::INTERRUPT_HANDLERS[int] as usize as u64;

        // Add an interrupt gate that will preprocess interrupt and jump to the `handle_interrupt`.
        idt.push(IdtGate::new(true, 0x08, address, 0xe, 0, ist));
    }

    // Make sure that the IDT won't get relocated.
    let idt = idt.into_boxed_slice();

    // Create a IDTR that will point to the newly created IDT.
    let idtr = TableRegister {
        base:  idt.as_ptr() as usize,
        limit: core::mem::size_of_val(&idt[..]) as u16 - 1,
    };

    // Load new IDT.
    cpu::set_idt(&idtr);

    // Reenable NMIs.
    cpu::outb(0x70, cpu::inb(0x70) & 0x7f);

    *interrupts = Some(Interrupts {
        _idt: idt,
        _gdt: gdt,
        _tss: tss,
    });
}

fn panic_on_page_fault(frame: &InterruptFrame, error: u64) -> ! {
    let faulty_address = cpu::get_cr2();

    let p    = error & (1 << 0) != 0;
    let wr   = error & (1 << 1) != 0;
    let rsvd = error & (1 << 3) != 0;
    let id   = error & (1 << 4) != 0;

    let action = if id {
        "execute"
    } else if wr {
        "write to"
    } else {
        "read"
    };

    let reason = if !p {
        "Page was not present."
    } else if rsvd {
        "Reverved bit was set in one of the page table entries."
    } else if wr {
        "Page was not writable."
    } else if id {
        "Page was not executable."
    } else {
        "Unknown reason for page fault."
    };

    if id {
        panic!("Tried to execute invalid memory address {:02x}:{:x}. {}",
               frame.cs, faulty_address, reason);
    } else {
        panic!("Instruction at {:02x}:{:x} tried to {} invalid memory address {:x}. {}",
               frame.cs, frame.rip, action, faulty_address, reason);
    }
}

fn panic_on_interrupt(vector: u8, frame: &InterruptFrame, error: u64, _regs: &RegisterState) -> ! {
    if vector < 32 {
        if vector == 14 {
            panic_on_page_fault(frame, error);
        } else {
            const EXCEPTION_NAMES: [&str; 21] = [
                "#DE",
                "#DB",
                "NMI",
                "#BP",
                "#OF",
                "#BR",
                "#UD",
                "#NM",
                "#DF",
                "Coprocessor Segment Overrun",
                "#TS",
                "#NP",
                "#SS",
                "#GP",
                "#PF",
                "Reserved",
                "#MF",
                "#AC",
                "#MC",
                "#XM",
                "#VE",
            ];

            let vector = vector as usize;
            if  vector < EXCEPTION_NAMES.len() {
                panic!("Cannot handle {}({:x}) exception at {:02x}:{:x}.",
                       EXCEPTION_NAMES[vector], error, frame.cs, frame.rip);
            } else {
                panic!("Unknown exception {} with error code {:x} at {:02x}:{:x}.",
                       vector, error, frame.cs, frame.rip);
            }
        }
    } else {
        panic!("Not recognising hardware IRQ with vector {}.", vector);
    }
}

#[no_mangle]
unsafe extern "C" fn handle_interrupt(vector: u8, frame: &mut InterruptFrame, error: u64,
                                      regs: &mut RegisterState) {
    // On kernel panic NMI is sent to all cores on the system to halt execution.
    if vector == 2 && panic::is_panicking() {
        panic::halt();
    }

    panic_on_interrupt(vector, frame, error, regs);
}
