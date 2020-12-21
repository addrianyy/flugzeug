fn main() {
    asmlink::build_and_link(&["src/interrupts.asm"], "elf64");
}
