use std::process::Command;
use std::path::Path;
use std::fs;

use elfparse::{Elf, Bitness};

// 1 kilobyte
const MAX_EARLY_BOOTLOADER_SIZE: usize = 1024;

// 400 kilobytes
const MAX_BOOTLOADER_SIZE: usize = 400 * 1024;

fn build(command: &str, directory: Option<&Path>, args: &[&str], fail_message: &str) -> bool {
    let mut to_run = Command::new(command);

    if let Some(directory) = directory {
        to_run.current_dir(directory);
    }

    let status = to_run
        .args(args)
        .status()
        .expect(&format!("Invoking {} failed.", command));

    if !status.success() {
        println!("{}", fail_message);

        false
    } else {
        println!("Done!");
        true
    }
}

fn hash(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c_9dc5_u32;

    for &byte in bytes {
        hash ^= byte as u32;
        hash = hash.wrapping_mul(16_777_619);
    }

    hash
}

fn prepare_bootloader_binary(binary: Vec<u8>) -> (Vec<u8>, u32) {
    println!("\nPreparing bootloader binary...");

    let elf = Elf::parse(&binary).expect("Failed to parse bootloader ELF.");

    assert!(elf.bitness() == Bitness::Bits32, "Bootloader is not 32 bit.");
    assert!(elf.base_address() == 0x10000, "Bootloader has invalid base address.");

    let mut mapped = Vec::new();

    macro_rules! pad {
        ($amount: expr) => {
            mapped.extend(vec![0u8; $amount]);
        }
    }

    elf.for_each_segment(|segment| {
        if !segment.load {
            return;
        }

        let virt_offset = (segment.virt_addr - elf.base_address()) as usize;
        let virt_size   = segment.virt_size as usize;

        pad!(virt_offset.checked_sub(mapped.len())
             .expect("Segments are not in ascending order."));
        
        mapped.extend_from_slice(segment.bytes);

        pad!(virt_size.checked_sub(segment.bytes.len())
             .expect("Virtual size is smaller than file size."));
    }).expect("Failed to iterate over bootloader segments.");

    pad!(((mapped.len() + 0xfff) & !0xfff) - mapped.len());

    let hash = hash(&mapped);

    println!("Bootloader base is {:#x}.", elf.base_address());
    println!("Bootloader size is {:#x}.", mapped.len());
    println!("Bootloader hash is {:#x}.", hash);

    (mapped, hash)
}

fn prepare_kernel_binary(mut binary: Vec<u8>) -> (Vec<u8>, u32) {
    println!("\nPreparing kernel binary...");

    binary.extend(vec![0u8; ((binary.len() + 0xfff) & !0xfff) - binary.len()]);

    let elf = Elf::parse(&binary).expect("Failed to parse kernel ELF.");

    assert!(elf.bitness() == Bitness::Bits64, "Kernel is not 64 bit.");

    let hash = hash(&binary);

    println!("Kernel base is {:#x}.", elf.base_address());
    println!("Kernel size is {:#x}.", binary.len());
    println!("Kernel hash is {:#x}.", hash);

    (binary, hash)
}

const BDD_SIGNATURE: u32 = 0x1778cf9d;

#[repr(C)]
struct BootDiskDescriptor {
    signature:           u32,
    bootloader_lba:      u32,
    bootloader_sectors:  u32,
    bootloader_checksum: u32,
    kernel_lba:          u32,
    kernel_sectors:      u32,
    kernel_checksum:     u32,
}

fn create_boot_image(early_bootloader: &[u8], bootloader: &[u8], kernel: &[u8],
                     bootloader_checksum: u32, kernel_checksum: u32) -> Vec<u8> {
    assert!(early_bootloader.len() <= MAX_EARLY_BOOTLOADER_SIZE, "Early bootloader is too big.");
    assert!(bootloader.len() <= MAX_BOOTLOADER_SIZE, "Bootloader is too big.");

    assert!(early_bootloader.len() % 512 == 0, "Early bootloader size is not aligned.");
    assert!(bootloader.len() % 4096 == 0, "Bootloader size is not aligned.");
    assert!(kernel.len() % 4096 == 0, "Kernel size is not aligned.");
    
    assert!(std::mem::size_of::<BootDiskDescriptor>() < 512, "Boot disk descriptor is too big.");

    let bootloader_sectors = (bootloader.len() % 512) as u32;
    let kernel_sectors     = (kernel.len()     % 512) as u32;

    let first_free_lba = (early_bootloader.len() % 512 + 1) as u32;
    let bootloader_lba = first_free_lba;
    let kernel_lba     = bootloader_lba + bootloader_sectors;

    let bdd = BootDiskDescriptor {
        signature: BDD_SIGNATURE,
        bootloader_lba,
        bootloader_sectors,
        bootloader_checksum,
        kernel_lba,
        kernel_sectors,
        kernel_checksum,
    };

    let mut bdd_sector = vec![0u8; 512];
    unsafe {
        std::ptr::copy_nonoverlapping(&bdd as *const BootDiskDescriptor as *const u8,
                                      bdd_sector.as_mut_ptr(),
                                      std::mem::size_of::<BootDiskDescriptor>());
    }

    let mut image = Vec::new();

    image.extend_from_slice(&early_bootloader[..512]);
    image.extend_from_slice(&bdd_sector);
    image.extend_from_slice(&early_bootloader[512..]);
    image.extend_from_slice(&bootloader);
    image.extend_from_slice(&kernel);

    assert!(image.len() % 512 == 0, "Created image was not aligned.");

    image
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

    let early_bootloader = std::fs::read(make_path!(bootloader_build_dir, "early.bin"))
        .expect("Failed to read early bootloader binary.");

    let bootloader = std::fs::read(make_path!(bootloader_build_dir, "i586-unknown-none",
                                              "release", "bootloader"))
        .expect("Failed to read kernel binary.");

    let kernel = std::fs::read(make_path!(kernel_build_dir, "x86_64-unknown-none",
                                          "release", "kernel"))
        .expect("Failed to read kernel binary.");

    let (bootloader, bootloader_checksum) = prepare_bootloader_binary(bootloader);
    let (kernel,     kernel_checksum)     = prepare_kernel_binary(kernel);

    println!("\nCreating bootable image...");

    let image = create_boot_image(&early_bootloader, &bootloader, &kernel,
                                  bootloader_checksum, kernel_checksum);

    println!("Done!");

    std::fs::write(Path::new("build").join("image"), &image)
        .expect("Failed to write created image to disk.");

    println!("\nEverything done!");
}
