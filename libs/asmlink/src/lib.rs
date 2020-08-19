use std::collections::HashSet;
use std::process::Command;
use std::path::Path;

/// Build assembly files specified by `source_files` with format `format` and link them
/// to the current binary.
pub fn build_and_link(source_files: &[impl AsRef<Path>], format: &str) {
    // Rust requires us to put all compiled files in the path specified by `OUT_DIR` environment
    // variable.
    let out_dir = std::env::var("OUT_DIR").unwrap();

    // Tell Rust that libraries that we will link to can be found in the `out_dir` path.
    println!("cargo:rustc-link-search={}", out_dir);

    let out_dir = Path::new(&out_dir);

    let mut compiled_libraries = HashSet::new();

    // Compile and link every source assembly file.
    for source_file in source_files {
        let source_file = source_file.as_ref();

        // Get the library name by taking a file name and stripping it's extension.
        let libname = source_file.with_extension("");
        let libname = libname.file_name().unwrap().to_str().unwrap();

        // Make sure that all libraries have unique names.
        assert!(compiled_libraries.insert(libname.to_owned()),
            "Some libraries have the same name.");

        // Object file will have a name `name.obj`.
        let object_file = out_dir.join(libname).with_extension("obj");

        // Archive file will have a name `libname.a`.
        let archive_file = out_dir.join(format!("lib{}", libname)).with_extension("a");

        // Convert `Path`s to UTF-8 strings.
        let source_file  = source_file.to_str().unwrap();
        let object_file  = object_file.to_str().unwrap();
        let archive_file = archive_file.to_str().unwrap();

        // Project needs to be recompiled if source file has changed.
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

        // We can't directly link to the object file so we need to make an archive file first.
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

        // Link to the newly compiled library.
        println!("cargo:rustc-link-lib=static={}", libname);
    }
}
