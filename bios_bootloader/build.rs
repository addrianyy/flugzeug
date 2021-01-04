fn main() {
    asm::link(&["src/bios.asm", "src/enter_kernel.asm"], asm::Format::Elf32);
}
