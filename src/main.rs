use std::process::Command;
use std::path::Path;
use std::fs;

fn build(command: &str, directory: Option<&Path>, args: &[&str], fail_message: &str) -> bool {
    let mut to_run = Command::new(command);

    if let Some(directory) = directory {
        to_run.current_dir(directory);
    }

    let s = to_run
        .args(args)
        .status()
        .expect(&format!("Invoking {} failed.", command));

    if !s.success() {
        println!("{}", fail_message);

        false
    } else {
        println!("Done!");
        true
    }
}

fn main() {
    macro_rules! make_path {
        ($path: expr) => {
            $path.to_str().unwrap()
        };
        ($path: expr, $($component: expr),*) => {
            $path
                $(.join($component))*
                .to_str().unwrap()
        };
    }

    fs::create_dir_all(Path::new("build"))
        .expect("Couldn't create `build` directory.");

    fs::create_dir_all(Path::new("build").join("bootloader"))
        .expect("Couldn't create `build/bootloader` directory.");

    fs::create_dir_all(make_path!(Path::new("build"), "kernel"))
        .expect("Couldn't create `build/kernel` directory.");

    let bootloader_dir = Path::new("bootloader").canonicalize()
        .expect("Couldn't get path to `bootloader` directory");

    let kernel_dir = Path::new("kernel").canonicalize()
        .expect("Couldn't get path to `kernel` directory");

    let bootloader_build_dir = Path::new("build").join("bootloader").canonicalize()
        .expect("Couldn't get path to `build/bootloader` directory");

    let kernel_build_dir = Path::new("build").join("kernel").canonicalize()
        .expect("Couldn't get path to `kernel/bootloader` directory");

    println!("Compiling early bootloader stage...");
    if !build(
        "nasm", None,
        &[
            make_path!(bootloader_dir, "src", "early.asm"),
            "-o",
            make_path!(bootloader_build_dir, "early.bin"),
        ],
        "Building bootloader `early.asm` component failed.",
    ) {
        return;
    }

    println!("\nCompiling low level bootloader routines...");
    if !build(
        "nasm", None,
        &[
            make_path!(bootloader_dir, "src", "low_level.asm"),
            "-felf32", "-o",
            make_path!(bootloader_build_dir, "low_level.o"),
        ],
        "Building bootloader `low_level.asm` component failed.",
    ) {
        return;
    }

    println!("\nCompiling bootloader...");
    if !build(
        "cargo", Some(&bootloader_dir),
        &[
            "build", "--release", "--target-dir",
            make_path!(bootloader_build_dir),
        ],
        "Building bootloader failed.",
    ) {
        return;
    }

    println!("\nCompiling kernel...");
    if !build(
        "cargo", Some(&kernel_dir),
        &[
            "build", "--release", "--target-dir",
            make_path!(kernel_build_dir),
        ],
        "Building kernel failed.",
    ) {
        return;
    }

    println!("Created OS image!");
}
