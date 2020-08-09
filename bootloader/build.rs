fn main() {
    asmlink::build_and_link(&["src/low_level.asm"], "elf32");
}
