fn main() {
    asmlink::build_and_link(&["src/enter_kernel.asm"], "win64");
}
