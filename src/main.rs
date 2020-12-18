use std::process::Command;
use std::path::Path;
use std::io::Write;
use std::fs;

fn build(command: &str, directory: Option<&Path>, args: &[&str], envs: &[(&str, &str)],
         fail_message: &str) -> bool {
    let mut to_run = Command::new(command);

    if let Some(directory) = directory {
        to_run.current_dir(directory);
    }

    let status = to_run
        .args(args)
        .envs(envs.iter().map(|&(k, v)| (k, v)))
        .status()
        .unwrap_or_else(|_| panic!("Invoking `{}` failed.", command));

    if !status.success() {
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

    let bootloader_name = "uefi_bootloader";

    fs::create_dir_all(Path::new("build"))
        .expect("Couldn't create `build` directory.");

    fs::create_dir_all(Path::new("build").join(bootloader_name))
        .expect("Couldn't create `build/xx_bootloader` directory.");

    fs::create_dir_all(make_path!(Path::new("build"), "kernel"))
        .expect("Couldn't create `build/kernel` directory.");

    let bootloader_dir = Path::new(bootloader_name).canonicalize()
        .expect("Couldn't get path to `xx_bootloader` directory");

    let kernel_dir = Path::new("kernel").canonicalize()
        .expect("Couldn't get path to `kernel` directory");

    let bootloader_build_dir = Path::new("build").join(bootloader_name).canonicalize()
        .expect("Couldn't get path to `build/xx_bootloader` directory");

    let kernel_build_dir = Path::new("build").join("kernel").canonicalize()
        .expect("Couldn't get path to `build/kernel` directory");

    println!("\nCompiling kernel...");
    if !build(
        "cargo", Some(&kernel_dir),
        &[
            "build", "--release", "--target-dir",
            make_path!(kernel_build_dir),
        ],
        &[],
        "Building kernel failed.",
    ) {
        std::process::exit(1);
    }

    println!("Compiling AP entrypoint...");
    if !build(
        "nasm", None,
        &[
            make_path!(bootloader_dir, "src", "ap_entrypoint.asm"),
            "-o",
            make_path!(bootloader_build_dir, "ap_entrypoint.bin"),
        ],
        &[],
        "Building bootloader `ap_entrypoint.asm` component failed.",
    ) {
        std::process::exit(1);
    }

    let ap_entrypoint_path = make_path!(bootloader_build_dir, "ap_entrypoint.bin").to_owned();
    let kernel_path = make_path!(kernel_build_dir, "x86_64-unknown-none",
                                 "release", "kernel").to_owned();

    println!("\nCompiling bootloader...");
    if !build(
        "cargo", Some(&bootloader_dir),
        &[
            "build", "--release", "--target-dir",
            make_path!(bootloader_build_dir),
        ],
        &[
            ("FLUGZEUG_KERNEL_PATH", &kernel_path),
            ("FLUGZEUG_AP_ENTRYPOINT_PATH", &ap_entrypoint_path),
        ],
        "Building bootloader failed.",
    ) {
        std::process::exit(1);
    }

    let bootloader = std::fs::read(make_path!(bootloader_build_dir, "x86_64-unknown-uefi",
                                              "release", "bootloader.efi"))
        .expect("Failed to read bootloader binary.");

    println!("\nCreating bootable image...");

    let image_path = Path::new("build").join("uefi_image");
    let image_file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(image_path)
        .expect("Failed to create FAT bootable image.");

    let startup = r#"
fs0:
flugzeug.efi
    "#;

    image_file
        .set_len((bootloader.len() + startup.len() + 10 * 0x10_0000) as u64)
        .expect("Failed to set length of FAT image file.");

    fatfs::format_volume(&image_file, fatfs::FormatVolumeOptions::new())
        .expect("Failed to format FAT32 image file.");

    let fs = fatfs::FileSystem::new(&image_file, fatfs::FsOptions::new())
        .expect("Failed to open FAT32 filesystem.");

    let mut bootloader_file = fs.root_dir().create_file("flugzeug.efi").unwrap();
    bootloader_file.truncate().unwrap();
    bootloader_file.write_all(&bootloader).unwrap();

    let mut startup_file = fs.root_dir().create_file("startup.nsh").unwrap();
    startup_file.truncate().unwrap();
    startup_file.write_all(startup.as_bytes()).unwrap();

    println!("Done!");

    println!("\nEverything done!");
}
