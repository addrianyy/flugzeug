fn main() {
    asm::link(&["src/interrupts.asm"], asm::Format::Elf64);
}
