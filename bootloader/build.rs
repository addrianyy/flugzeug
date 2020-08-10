fn main() {
    asmlink::build_and_link(&["src/bios.asm"], "elf32");
}
