use std::path::{Path, PathBuf};

use crate::build::{ImageBuilder, BuildParameters, build};

use elfparse::{Elf, Bitness, SegmentType, Machine};
use bdd::BootDiskDescriptor;

// Don't change. Hardcoded in bootloader assembly file.
const MAX_EARLY_BOOTLOADER_SIZE: usize = 3 * 512;
const MAX_BOOTLOADER_SIZE:       usize = 400 * 1024;
const BDD_SIZE:                  usize = 512;
const BOOTLOADER_BASE:           u64   = 0x10000;

fn prepare_bootloader_binary(binary: Vec<u8>) -> (Vec<u8>, u32) {
    println!("\nPreparing bootloader binary...");

    let elf = Elf::parse(&binary).expect("Failed to parse bootloader ELF.");

    assert!(elf.bitness() == Bitness::Bits32, "Bootloader is not 32 bit.");
    assert!(elf.machine() == Machine::X86, "Bootloader is not x86 binary.");
    assert!(elf.base_address() == BOOTLOADER_BASE, "Bootloader has invalid base address.");

    let mut mapped = Vec::new();

    macro_rules! pad {
        ($amount: expr) => {
            mapped.extend(vec![0u8; $amount]);
        }
    }

    elf.segments(|segment| {
        if segment.seg_type != SegmentType::Load {
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

    let checksum = bdd::checksum(&mapped);

    println!("Bootloader base is {:#x}.", elf.base_address());
    println!("Bootloader size is {:#x}.", mapped.len());
    println!("Bootloader checksum is {:#x}.", checksum);

    (mapped, checksum)
}

fn prepare_kernel_binary(mut binary: Vec<u8>) -> (Vec<u8>, u32) {
    println!("\nPreparing kernel binary...");

    binary.extend(vec![0u8; ((binary.len() + 0xfff) & !0xfff) - binary.len()]);

    let elf = Elf::parse(&binary).expect("Failed to parse kernel ELF.");

    assert!(elf.bitness() == Bitness::Bits64, "Kernel is not 64 bit.");

    let checksum = bdd::checksum(&binary);

    println!("Kernel base is {:#x}.", elf.base_address());
    println!("Kernel size is {:#x}.", binary.len());
    println!("Kernel checksum is {:#x}.", checksum);

    (binary, checksum)
}

fn create_boot_image(early_bootloader: &[u8], bootloader: &[u8], kernel: &[u8],
                     bootloader_checksum: u32, kernel_checksum: u32) -> Vec<u8> {
    assert!(early_bootloader.len() <= MAX_EARLY_BOOTLOADER_SIZE, "Early bootloader is too big.");
    assert!(bootloader.len() <= MAX_BOOTLOADER_SIZE, "Bootloader is too big.");

    assert!(early_bootloader.len() % 512 == 0, "Early bootloader size is not aligned.");
    assert!(bootloader.len() % 4096 == 0, "Bootloader size is not aligned.");
    assert!(kernel.len() % 4096 == 0, "Kernel size is not aligned.");
    
    assert!(std::mem::size_of::<BootDiskDescriptor>() <= BDD_SIZE,
            "Boot disk descriptor is too big.");

    let bootloader_sectors = (bootloader.len() / 512) as u32;
    let kernel_sectors     = (kernel.len()     / 512) as u32;

    // Add 1 to skip BDD.
    let first_free_lba = (early_bootloader.len() / 512 + 1) as u32;

    let bootloader_lba = first_free_lba;
    let kernel_lba     = bootloader_lba + bootloader_sectors;

    let bdd = BootDiskDescriptor {
        signature: bdd::SIGNATURE,
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

pub struct BiosBuilder {
    kernel_path:          PathBuf,
    bootloader_dir:       PathBuf,
    bootloader_build_dir: PathBuf,
}

impl ImageBuilder for BiosBuilder {
    fn new(kernel_path: &Path, bootloader_dir: &Path, bootloader_build_dir: &Path) -> Self {
        Self {
            kernel_path:          kernel_path.to_owned(),
            bootloader_dir:       bootloader_dir.to_owned(),
            bootloader_build_dir: bootloader_build_dir.to_owned(),
        }
    }

    fn bootloader_name() -> &'static str {
        "bios_bootloader"
    }

    fn image_name() -> &'static str {
        "flugzeug_bios"
    }

    fn build_bootloader_dependencies(&mut self) {
        println!("\nCompiling early bootloader stage...");
        if !build(
            "nasm", None,
            &[
                make_path!(self.bootloader_dir, "src", "early.asm"),
                "-o",
                make_path!(self.bootloader_build_dir, "early.bin"),
            ],
            &[],
            "Building bootloader `early.asm` component failed.",
        ) {
            std::process::exit(1);
        }
    }

    fn bootloader_build_parameters(&mut self) -> BuildParameters {
        BuildParameters {
            args: Vec::new(),
            envs: Vec::new(),
        }
    }

    fn create_image(&mut self, image_path: &Path) {
        let early_bootloader = std::fs::read(make_path!(self.bootloader_build_dir, "early.bin"))
            .expect("Failed to read early bootloader binary.");

        let bootloader = std::fs::read(make_path!(self.bootloader_build_dir, "i586-unknown-none",
                                                  "release", "bios_bootloader"))
            .expect("Failed to read bootloader binary.");

        let kernel = std::fs::read(&self.kernel_path)
            .expect("Failed to read kernel binary.");

        let (bootloader, bootloader_checksum) = prepare_bootloader_binary(bootloader);
        let (kernel,     kernel_checksum)     = prepare_kernel_binary(kernel);

        println!("\nCreating bootable image...");

        let image = create_boot_image(&early_bootloader, &bootloader, &kernel,
                                      bootloader_checksum, kernel_checksum);

        std::fs::write(image_path, &image)
            .expect("Failed to write created image to disk.");
    }
}
