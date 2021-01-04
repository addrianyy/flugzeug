fn main() {
    asm::link( &["src/enter_kernel.asm"], asm::Format::Win64);
    asm::embed(&["src/ap_entrypoint.asm"]);
}
