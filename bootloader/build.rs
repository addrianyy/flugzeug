fn main() {
    asmlink::build_and_link(&["src/bios.asm", "src/enter_kernel.asm"], "elf32");
}
