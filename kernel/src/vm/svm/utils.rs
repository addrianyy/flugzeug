use core::alloc::{GlobalAlloc, Layout};

use crate::mm::{self, PhysicalPage};

use lock::Lock;

use super::VmError;

const VM_CR_MSR:       u32 = 0xc001_0114;
const VM_HSAVE_PA_MSR: u32 = 0xc001_0117;
const EFER_MSR:        u32 = 0xc000_0080;

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C, align(64))]
struct FxSave {
    fcw:        u16,
    fsw:        u16,
    ftw:        u8,
    _rsvd0:     u8,
    fop:        u16,
    fip:        u32,
    fcs:        u16,
    _rsvd1:     u16,
    fdp:        u32,
    fds:        u16,
    _rsvd2:     u16,
    mxcsr:      u32,
    mxcsr_mask: u32,
    mm:         [u128; 8],
    xmm:        [u128; 16],
    reserved:   [u128; 6],
}

pub struct XsaveArea {
    pointer: *mut u8,
}

impl XsaveArea {
    pub fn new() -> Self {
        unsafe {
            // Allocate the XSAVE area with appropriate size and alignment.
            let xsave_size   = core!().xsave_size();
            let xsave_layout = Layout::from_size_align(xsave_size, 64)
                .expect("Failed to create XSAVE layout.");
            let xsave_area   = mm::GLOBAL_ALLOCATOR.alloc(xsave_layout);

            assert!(!xsave_area.is_null(), "Failed to allocate XSAVE area.");

            // Zero out XSAVE area as required by the architecture.
            core::ptr::write_bytes(xsave_area, 0, xsave_size);

            let mut fxsave = FxSave::default();

            // Setup initial FPU state.
            fxsave.fcw        = 0x40;
            fxsave.mxcsr      = 0x1f80;
            fxsave.mxcsr_mask = 0xffff_0000;

            core::ptr::write(xsave_area as *mut FxSave, fxsave);

            Self {
                pointer: xsave_area,
            }
        }
    }

    pub fn pointer(&mut self) -> *mut u8 {
        self.pointer
    }
}

impl Drop for XsaveArea {
    fn drop(&mut self) {
        unsafe {
            let xsave_size   = core!().xsave_size();
            let xsave_layout = Layout::from_size_align(xsave_size, 64)
                .expect("Failed to create XSAVE layout.");

            // Free the XSAVE area.
            mm::GLOBAL_ALLOCATOR.dealloc(self.pointer, xsave_layout);
        }
    }
}

#[derive(Default)]
pub struct SvmFeatures {
    pub nr_asids:       u32,
    pub decode_assists: bool,
    pub nrip_save:      bool,
    pub nested_paging:  bool,
    pub vmcb_clean:     bool,
}

impl SvmFeatures {
    pub fn get() -> Self {
        let mut features = SvmFeatures::default();

        if !cpu::get_features().svm {
            return features;
        }

        let cpuid = cpu::cpuid(0x8000_000a, 0);

        features.nr_asids       = cpuid.ebx;
        features.decode_assists = (cpuid.edx & (1 << 7)) != 0;
        features.vmcb_clean     = (cpuid.edx & (1 << 5)) != 0;
        features.nrip_save      = (cpuid.edx & (1 << 3)) != 0;
        features.nested_paging  = (cpuid.edx & (1 << 0)) != 0;

        features
    }
}

pub fn enable_svm() -> Result<(), VmError> {
    // If host save area is allocated than SVM is already enabled on this processor.
    if core!().host_save_area.lock().is_some() {
        return Ok(());
    }

    {
        let cpuid      = cpu::cpuid(0, 0);
        let mut vendor = [0u8; 3 * 4];

        vendor[0.. 4].copy_from_slice(&cpuid.ebx.to_le_bytes());
        vendor[4.. 8].copy_from_slice(&cpuid.edx.to_le_bytes());
        vendor[8..12].copy_from_slice(&cpuid.ecx.to_le_bytes());

        if &vendor != b"AuthenticAMD" {
            return Err(VmError::NonAmdCpu);
        }
    }

    if !cpu::get_features().svm {
        return Err(VmError::SvmNotSupported);
    }

    let svm_cr = unsafe { cpu::rdmsr(VM_CR_MSR) };
    if  svm_cr & (1 << 4) != 0 {
        return Err(VmError::SvmDisabled);
    }

    // Allocate area used by the SVM to store host state on `vmrun`.
    let host_save_area = PhysicalPage::new([0u8; 4096]);

    unsafe {
        // Enable SVM.
        cpu::wrmsr(EFER_MSR, cpu::rdmsr(EFER_MSR) | (1 << 12));

        // Set physical address of the host save area.
        cpu::wrmsr(VM_HSAVE_PA_MSR, host_save_area.phys_addr().0);
    }

    // Store host save area in core locals.
    *core!().host_save_area.lock() = Some(host_save_area);

    Ok(())
}

const  MAX_CONCURRENT_VMS: usize = 2048;
static BITMAP_ENTRIES: Lock<[u64; MAX_CONCURRENT_VMS / 64]> =
    Lock::new([0; MAX_CONCURRENT_VMS / 64]);

pub struct Asid(u32);

impl Asid {
    pub fn new(nr_asids: u32) -> Option<Self> {
        let mut bitmap = BITMAP_ENTRIES.lock();

        for (index, entry) in bitmap.iter_mut().enumerate() {
            // Skip full entries.
            if *entry == !0 {
                continue;
            }

            for bit in 0..64 {
                if *entry & (1 << bit) == 0 {
                    // Take this ASID if it is usable.
                    let asid = (index * 64 + bit + 1) as u32;
                    if  asid < nr_asids {
                        *entry |= 1 << bit;

                        return Some(Self(asid));
                    } else {
                        return None;
                    }
                }
            }
        }

        None
    }

    pub fn get(&self) -> u32 {
        self.0
    }
}

impl Drop for Asid {
    fn drop(&mut self) {
        let asid = self.0;

        assert!(asid != 0, "Cannot return host ASID.");

        let mut bitmap = BITMAP_ENTRIES.lock();
        let value      = (asid - 1) as usize;

        let entry = &mut bitmap[value / 64];
        let mask  = 1 << (value % 64);

        assert!(*entry & mask == mask, "Cannot return unused ASID.");

        *entry &= !mask;
    }
}
