use std::collections::HashSet;
use std::process::Command;
use std::path::Path;

pub fn build_and_link(source_files: &[impl AsRef<Path>], format: &str) {
    let out_dir = std::env::var("OUT_DIR").unwrap();

    println!("cargo:rustc-link-search={}", out_dir);

    let out_dir = Path::new(&out_dir);

    let mut compiled_libraries = HashSet::new();

    for source_file in source_files {
        let source_file = source_file.as_ref();

        let libname = source_file.with_extension("");
        let libname = libname.file_name().unwrap().to_str().unwrap();

        assert!(compiled_libraries.insert(libname.to_owned()),
            "Some libraries have the same name.");

        let object_file  = out_dir.join(libname).with_extension("obj");
        let archive_file = out_dir.join(format!("lib{}", libname)).with_extension("a");

        let source_file  = source_file.to_str().unwrap();
        let object_file  = object_file.to_str().unwrap();
        let archive_file = archive_file.to_str().unwrap();

        println!("cargo:rerun-if-changed={}", source_file);

        println!("\nCompiling {}...", libname);

        let status = Command::new("nasm")
            .args(&[
                source_file,
                "-f", format,
                "-o", object_file,
            ])
            .status()
            .expect("Failed to invoke `nasm`.");

        if !status.success() {
            panic!("Failed to compile.");
        }

        println!("Done!");

        println!("\nMaking archive for {}...", libname);

        let status = Command::new("llvm-ar-10")
            .args(&[
                "crus",
                archive_file,
                object_file,
            ])
            .status()
            .expect("Failed to invoke `llvm-ar-10`.");

        if !status.success() {
            panic!("Failed to make archive.");
        }

        println!("Done!");

        println!("cargo:rustc-link-lib=static={}", libname);
    }
}
