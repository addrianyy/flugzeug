use core::convert::TryInto;

use crate::{BOOT_BLOCK, mm, binaries};

pub struct APEntrypoint {
    code_address: usize,
    code_buffer:  &'static mut [u8],
    finalized:    bool,
}

impl APEntrypoint {
    pub unsafe fn new() -> Self {
        if false {
            // Reserve memory to test segment handling in AP entrypoint.
            // Testing only.
            mm::allocate_low_boot_memory(0x11000, 0x1000)
                .expect("Failed to allocate testing area.");
        }

        // 8KB of stack should be enough.
        const STACK_SIZE: usize = 8 * 1024;

        let entrypoint_code = binaries::AP_ENTRYPOINT;
        let code_size       = (entrypoint_code.len() + 0xfff) & !0xfff;
        let area_size       = code_size + STACK_SIZE;

        assert!(area_size & 0xfff == 0, "AP entrypoint area size is not page aligned.");

        // Allocate AP area in low memory that is accesible by 16 bit code.
        let area_address = mm::allocate_low_boot_memory(area_size as u64, 0x1000)
            .expect("Failed to allocate AP entrypoint.");

        let code_address = (area_address as usize) + STACK_SIZE;
        let code_buffer  = core::slice::from_raw_parts_mut(code_address as *mut u8,
                                                           entrypoint_code.len());

        code_buffer.copy_from_slice(entrypoint_code);

        Self {
            code_address,
            code_buffer,
            finalized: false,
        }
    }

    pub unsafe fn finalize_and_register(&mut self, trampoline_cr3: u64) {
        let code_buffer = &mut self.code_buffer;

        let code_address:   u32 = self.code_address.try_into().expect("AP entrypoint > 4GB");
        let trampoline_cr3: u32 = trampoline_cr3.try_into().expect("Trampoline CR3 > 4GB");

        // Make sure that AP entrypoint starts with:
        //   mov eax, 0xaabbccdd
        //   jmp skip
        //
        // AP entrypoint will expect us to change 0xaabbccdd to its own address.
        assert!(&code_buffer[..6 + 1] == &[0x66, 0xb8, 0xdd, 0xcc, 0xbb, 0xaa, 0xeb]);

        // Replace imm in `mov` to code base address.
        code_buffer[2..6].copy_from_slice(&code_address.to_le_bytes());

        // Calculate jump target relative to `code_address`.
        let jmp_target_offset = (code_buffer[6 + 1] + 6 + 2) as usize;

        // 6 bytes for mov and 2 bytes for jmp.
        let mut current_offset = 6 + 2;
        let mut current_buffer = &mut code_buffer[current_offset..];

        macro_rules! write {
            ($value: expr) => {{
                let value: u64 = $value;
                let bytes      = value.to_le_bytes();

                current_buffer[..bytes.len()].copy_from_slice(&bytes);
                current_offset += bytes.len();

                #[allow(unused)]
                {
                    current_buffer = &mut current_buffer[bytes.len()..];
                }
            }}
        }

        // trampoline_cr3:        dq 0
        write!(trampoline_cr3 as u64);

        // bootloader_entrypoint: dq 0
        write!(crate::efi_main as *const () as u64);

        // Make sure we have written expected amount of bytes.
        assert_eq!(current_offset, jmp_target_offset,
                   "Data area in AP entrypoint was corrupted.");

        // Set AP entrypoint address so it will be used by the kernel.
        *BOOT_BLOCK.ap_entrypoint.lock() = Some(code_address as u64);

        self.finalized = true;
    }
}

impl Drop for APEntrypoint {
    fn drop(&mut self) {
        assert!(self.finalized, "AP entrypoint is being destroyed without finalization.");
    }
}
