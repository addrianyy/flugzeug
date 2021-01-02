use super::{Vm, Register, TableRegister, DescriptorTable, SegmentRegister, Segment};

impl Vm {
    pub fn reg(&self, register: Register) -> u64 {
        let index = register as usize;

        if index < self.guest_registers.len() {
            self.guest_registers[index]
        } else {
            use Register::*;

            macro_rules! create_match {
                ($($register: pat, $field: ident),*) => {
                    match register {
                        $(
                            $register => self.vmcb().state.$field,
                        )*
                        _ => unreachable!(),
                    }
                }
            }

            create_match!(
                Efer,         efer,
                Cr0,          cr0,
                Cr2,          cr2,
                Cr3,          cr3,
                Cr4,          cr4,
                Dr6,          dr6,
                Dr7,          dr7,
                Star,         star,
                Lstar,        lstar,
                Cstar,        cstar,
                Sfmask,       sfmask,
                KernelGsBase, kernel_gs_base,
                SysenterCs,   sysenter_cs,
                SysenterEsp,  sysenter_esp,
                SysenterEip,  sysenter_eip,
                Pat,          g_pat
            )
        }
    }

    pub fn set_reg(&mut self, register: Register, value: u64) {
        let index = register as usize;

        if index < self.guest_registers.len() {
            self.guest_registers[index] = value;
        } else {
            use Register::*;

            macro_rules! create_match {
                ($($register: pat, $field: ident),*) => {
                    match register {
                        $(
                            $register => self.vmcb_mut().state.$field = value,
                        )*
                        _ => unreachable!(),
                    }
                }
            }

            create_match!(
                Efer,         efer,
                Cr0,          cr0,
                Cr2,          cr2,
                Cr3,          cr3,
                Cr4,          cr4,
                Dr6,          dr6,
                Dr7,          dr7,
                Star,         star,
                Lstar,        lstar,
                Cstar,        cstar,
                Sfmask,       sfmask,
                KernelGsBase, kernel_gs_base,
                SysenterCs,   sysenter_cs,
                SysenterEsp,  sysenter_esp,
                SysenterEip,  sysenter_eip,
                Pat,          g_pat
            );
        }
    }

    #[allow(unused)]
    pub fn segment_reg(&self, register: SegmentRegister) -> Segment {
        use SegmentRegister::*;

        let state   = &self.vmcb().state;
        let segment = match register {
            Es  => &state.es,
            Cs  => &state.cs,
            Ss  => &state.ss,
            Ds  => &state.ds,
            Fs  => &state.fs,
            Gs  => &state.gs,
            Ldt => &state.ldtr,
            Tr  => &state.tr,
        };

        Segment {
            base:     segment.base,
            limit:    segment.limit,
            attrib:   segment.attrib,
            selector: segment.selector,
        }
    }

    pub fn set_segment_reg(&mut self, register: SegmentRegister, segment: Segment) {
        use SegmentRegister::*;

        if register == SegmentRegister::Cs {
            // Update the CPL when changing CS.
            let rpl = ((segment.selector >> 0) & 3) as u8;
            let dpl = ((segment.attrib   >> 5) & 3) as u8;

            self.vmcb_mut().state.cpl = u8::max(rpl, dpl);
        }

        let state = &mut self.vmcb_mut().state;
        let state = match register {
            Es  => &mut state.es,
            Cs  => &mut state.cs,
            Ss  => &mut state.ss,
            Ds  => &mut state.ds,
            Fs  => &mut state.fs,
            Gs  => &mut state.gs,
            Ldt => &mut state.ldtr,
            Tr  => &mut state.tr,
        };

        state.base     = segment.base;
        state.limit    = segment.limit;
        state.attrib   = segment.attrib;
        state.selector = segment.selector;
    }

    #[allow(unused)]
    pub fn table_reg(&mut self, register: TableRegister) -> DescriptorTable {
        let state = &self.vmcb().state;
        let table = match register {
            TableRegister::Idt => &state.idtr,
            TableRegister::Gdt => &state.gdtr,
        };

        DescriptorTable {
            base:  table.base,
            limit: table.limit as u16,
        }
    }

    pub fn set_table_reg(&mut self, register: TableRegister, table: DescriptorTable) {
        let state = &mut self.vmcb_mut().state;
        let state = match register {
            TableRegister::Idt => &mut state.idtr,
            TableRegister::Gdt => &mut state.gdtr,
        };

        state.base  = table.base;
        state.limit = table.limit as u32;
    }
}
