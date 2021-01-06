fn main() {
    asm::link(&["src/interrupts.asm"], asm::Format::Elf64);
    asm::embed(&["src/vm/vkernel.asm"]);
}
