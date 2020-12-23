#[inline(always)]
#[allow(unused)]
pub fn get_tsc() -> u64 {
    unsafe {
        core::arch::x86_64::_rdtsc()
    }
}
