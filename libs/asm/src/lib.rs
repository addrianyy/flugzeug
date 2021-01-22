use std::collections::HashSet;
use std::process::Command;
use std::path::Path;

pub enum Format {
    Elf32,
    Elf64,
    Win32,
    Win64,
}

fn try_command(command: &str) -> bool {
    Command::new(command)
        .arg("--version")
        .status()
        .is_ok()
}

fn llvm_suffix() -> Option<String> {
    let commands = ["llvm-ar", "llvm-lib"];

    let suffix_to_string = |suffix| {
        match suffix {
            0 => String::new(),
            _ => format!("-{}", suffix),
        }
    };

    if commands.iter().all(|command| try_command(command)) {
        return Some(suffix_to_string(0));
    }

    for version in (6..=20).rev() {
        let suffix = suffix_to_string(version);

        if commands.iter().all(|command| try_command(&format!("{}{}", command, suffix))) {
            return Some(suffix);
        }
    }

    None
}

/// Build assembly files specified by `source_files` with format `format` and link them
/// to the current binary.
pub fn link(source_files: &[impl AsRef<Path>], format: Format) {
    // Rust requires us to put all compiled files in the path specified by `OUT_DIR` environment
    // variable.
    let out_dir = std::env::var("OUT_DIR").unwrap();

    // Tell Rust that libraries that we will link to can be found in the `out_dir` path.
    println!("cargo:rustc-link-search={}", out_dir);

    let llvm_suffix = llvm_suffix().expect("Failed to find working LLVM toolchain.");

    let out_dir              = Path::new(&out_dir);
    let (format, is_windows) = match format {
        Format::Elf32 => ("elf32", false),
        Format::Elf64 => ("elf64", false),
        Format::Win32 => ("win32", true),
        Format::Win64 => ("win64", true),
    };

    let mut compiled_libraries = HashSet::new();

    // Compile and link every source assembly file.
    for source_file in source_files {
        let source_file = source_file.as_ref();

        // Get the library name by taking a file name and stripping its extension.
        let libname = source_file.with_extension("");
        let libname = libname.file_name().unwrap().to_str().unwrap();

        // Make sure that all libraries have unique names.
        assert!(compiled_libraries.insert(libname.to_owned()),
                "Some libraries have the same name.");

        // Object file will have a name `name.obj`.
        let object_file = out_dir.join(libname).with_extension("obj");

        // Convert `Path`s to UTF-8 strings.
        let source_file = source_file.to_str().unwrap();
        let object_file = object_file.to_str().unwrap();

        // Project needs to be recompiled if the source file has changed.
        println!("cargo:rerun-if-changed={}", source_file);

        // Compile `.asm` source to output file with requested format using NASM.
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

        if is_windows {
            // Library file will have a name `name.lib`.
            let library_file = out_dir.join(libname).with_extension("lib");
            let library_file = library_file.to_str().unwrap();

            // We can't directly link to the object file so we need to make a library file first.
            println!("\nMaking library for {}...", libname);
            let status = Command::new(format!("llvm-lib{}", llvm_suffix))
                .args(&[
                    object_file,
                    &format!("/out:{}", library_file),
                ])
                .status()
                .expect("Failed to invoke `llvm-lib`.");
            if !status.success() {
                panic!("Failed to make library.");
            }
        } else {
            // Archive file will have a name `libname.a`.
            let archive_file = out_dir.join(format!("lib{}", libname)).with_extension("a");
            let archive_file = archive_file.to_str().unwrap();

            // We can't directly link to the object file so we need to make an archive file first.
            println!("\nMaking archive for {}...", libname);
            let status = Command::new(format!("llvm-ar{}", llvm_suffix))
                .args(&[
                    "crus",
                    archive_file,
                    object_file,
                ])
                .status()
                .expect("Failed to invoke `llvm-ar`.");
            if !status.success() {
                panic!("Failed to make archive.");
            }
        }

        println!("Done!");

        // Link to the newly compiled library.
        println!("cargo:rustc-link-lib=static={}", libname);
    }
}

/// Build assembly files as binary files and make them embeddable in the Rust program.
pub fn embed(source_files: &[impl AsRef<Path>]) {
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let out_dir = Path::new(&out_dir);

    let mut compiled_binaries = HashSet::new();

    // Compile every source assembly file.
    for source_file in source_files {
        let source_file = source_file.as_ref();

        // Get the binary name by taking a file name and stripping its extension.
        let binname = source_file.with_extension("");
        let binname = binname.file_name().unwrap().to_str().unwrap();

        // Make sure that all binaries have unique names.
        assert!(compiled_binaries.insert(binname.to_owned()),
                "Some binaries have the same name.");

        // Binary file will have a name `name.bin`.
        let binary_file = out_dir.join(binname).with_extension("bin");

        // Convert `Path`s to UTF-8 strings.
        let source_file = source_file.to_str().unwrap();
        let binary_file = binary_file.to_str().unwrap();

        // Project needs to be recompiled if the source file has changed.
        println!("cargo:rerun-if-changed={}", source_file);

        // Compile `.asm` source to output file with binary format using NASM.
        println!("\nCompiling {}...", binname);
        let status = Command::new("nasm")
            .args(&[
                source_file,
                "-f", "bin",
                "-o", binary_file,
            ])
            .status()
            .expect("Failed to invoke `nasm`.");

        if !status.success() {
            panic!("Failed to compile.");
        }
        println!("Done!");
    }
}
