use std::path::{Path, PathBuf};
use std::io::Write;
use std::fs;

use crate::build::{ImageBuilder, BuildParameters};

pub struct UefiBuilder {
    kernel_path:          PathBuf,
    bootloader_build_dir: PathBuf,
}

impl ImageBuilder for UefiBuilder {
    fn new(kernel_path: &Path, _bootloader_dir: &Path, bootloader_build_dir: &Path) -> Self {
        Self {
            kernel_path:          kernel_path.to_owned(),
            bootloader_build_dir: bootloader_build_dir.to_owned(),
        }
    }

    fn bootloader_name() -> &'static str {
        "uefi_bootloader"
    }

    fn image_name() -> &'static str {
        "flugzeug_uefi"
    }

    fn build_bootloader_dependencies(&mut self) {}

    fn bootloader_build_parameters(&mut self) -> BuildParameters {
        let kernel_path = make_path!(self.kernel_path).to_owned();

        BuildParameters {
            args: vec![],
            envs: vec![
                (String::from("FLUGZEUG_KERNEL_PATH"), kernel_path),
            ],
        }
    }

    fn create_image(&mut self, image_path: &Path) {
        let bootloader = std::fs::read(make_path!(self.bootloader_build_dir,
                                                  "x86_64-unknown-uefi",
                                                  "release", "uefi_bootloader.efi"))
            .expect("Failed to read bootloader binary.");

        if !Path::new(image_path).exists() {
            fs::File::create(image_path)
                .expect("Failed to create FAT bootable image.");
        }

        let image_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(image_path)
            .expect("Failed to open FAT bootable image.");

        image_file
            .set_len(512 * 1024 * 1024)
            .expect("Failed to set length of FAT image file.");

        fatfs::format_volume(&image_file, fatfs::FormatVolumeOptions::new())
            .expect("Failed to format FAT32 image file.");

        let fs = fatfs::FileSystem::new(&image_file, fatfs::FsOptions::new())
            .expect("Failed to open FAT32 filesystem.");

        assert_eq!(fs.fat_type(), fatfs::FatType::Fat32, "Created invalid FAT.");

        let mut bootloader_file = fs.root_dir()
            .create_dir("efi")
            .unwrap()
            .create_dir("boot")
            .unwrap()
            .create_file("bootx64.efi")
            .unwrap();

        bootloader_file.truncate().unwrap();
        bootloader_file.write_all(&bootloader).unwrap();
    }
}
