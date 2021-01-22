use std::path::{Path, PathBuf};
use std::fs;

use build::{ImageBuilder, build};

#[macro_use] mod build;
mod bios;
mod uefi;

fn build_kernel() -> PathBuf {
    fs::create_dir_all(Path::new("build").join("kernel"))
        .expect("Couldn't create `build/kernel` directory.");

    let kernel_dir = build::canonicalize(Path::new("kernel"))
        .expect("Couldn't get path to `kernel` directory");

    let kernel_build_dir = build::canonicalize(Path::new("build").join("kernel"))
        .expect("Couldn't get path to `build/kernel` directory");

    println!("\nCompiling kernel...");
    if !build(
        "cargo", Some(&kernel_dir),
        &[
            "build", "--release", "--offline", "--target-dir",
            make_path!(kernel_build_dir),
        ],
        &[],
        "Building kernel failed.",
    ) {
        std::process::exit(1);
    }

    make_path!(kernel_build_dir, "x86_64-unknown-none", "release", "kernel")
        .to_owned().into()
}

fn build_image<B: ImageBuilder>(kernel_path: &Path) {
    let bootloader_name = B::bootloader_name();
    let image_name      = B::image_name();

    fs::create_dir_all(Path::new("build").join(bootloader_name))
        .expect("Couldn't create `build/xx_bootloader` directory.");

    let bootloader_dir = build::canonicalize(Path::new(bootloader_name))
        .expect("Couldn't get path to `xx_bootloader` directory");

    let bootloader_build_dir = build::canonicalize(Path::new("build").join(bootloader_name))
        .expect("Couldn't get path to `build/xx_bootloader` directory");
    
    let mut builder = B::new(kernel_path, &bootloader_dir, &bootloader_build_dir);

    builder.build_bootloader_dependencies();

    let parameters              = builder.bootloader_build_parameters();
    let envs: Vec<(&str, &str)> = parameters.envs.iter().map(|(k, v)| {
        let k: &str = k;
        let v: &str = v;

        (k, v)
    }).collect();

    let mut args = vec![
        "build", "--release", "--offline", "--target-dir", make_path!(bootloader_build_dir),
    ];

    args.extend(parameters.args.iter().map(|x| { let x: &str = x; x }));

    println!("\nCompiling {}...", bootloader_name);
    if !build(
        "cargo", Some(&bootloader_dir), &args, &envs,
        "Building bootloader failed.",
    ) {
        std::process::exit(1);
    }

    println!("\nCreating bootable image {}...", image_name);

    builder.create_image(&Path::new("build").join(image_name));

    println!("Done!");
}

fn main() {
    fs::create_dir_all(Path::new("build"))
        .expect("Couldn't create `build` directory.");

    let kernel_path = build_kernel();

    build_image::<uefi::UefiBuilder>(&kernel_path);
    build_image::<bios::BiosBuilder>(&kernel_path);

    println!("\nEverything done!.");
}
