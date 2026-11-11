use std::{env, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if env::var("TARGET").unwrap() != "riscv32imafc-unknown-none-elf" {
        return;
    }
    let output_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut build = cc::Build::new();
    build.file(root.join("native/support.c"));
    let toolchain = root.join(".deps/riscv32-esp-elf/bin");
    for function in ["printf", "vprintf", "sprintf", "snprintf", "vsnprintf"] {
        build.define(function, format!("pocket_libc_{function}").as_str());
    }
    build
        .compiler(toolchain.join("riscv32-esp-elf-gcc"))
        .archiver(toolchain.join("riscv32-esp-elf-ar"))
        .opt_level(2)
        .flag("-mabi=ilp32f")
        .define("POCKET_EMBEDDED", None);
    for library in ["libc.a", "libgcc.a"] {
        let output = Command::new(toolchain.join("riscv32-esp-elf-gcc"))
            .args([
                "-march=rv32imafc",
                "-mabi=ilp32f",
                &format!("-print-file-name={library}"),
            ])
            .output()
            .expect("query RISC-V library path");
        assert!(output.status.success());
        let path = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
        println!(
            "cargo:rustc-link-search=native={}",
            path.parent().unwrap().display()
        );
        if library == "libc.a" {
            // esp-hal owns the stack guard and failure handler.
            let archive = output_dir.join("libpocket_c.a");
            std::fs::copy(&path, &archive).unwrap();
            let status = Command::new(toolchain.join("riscv32-esp-elf-ar"))
                .arg("d")
                .arg(&archive)
                .arg("libc_a-stack_protector.o")
                .status()
                .expect("remove duplicate C startup object");
            assert!(status.success());
            // The radio supplies its own formatter; keep newlib formatting private to QuickJS.
            let mut rename = Command::new(toolchain.join("riscv32-esp-elf-objcopy"));
            for function in ["printf", "vprintf", "sprintf", "snprintf", "vsnprintf"] {
                rename.arg(format!("--redefine-sym={function}=pocket_libc_{function}"));
            }
            assert!(rename.arg(&archive).status().unwrap().success());
        }
    }
    build.compile("mosaico_newlib");
    for library in ["m", "pocket_c", "nosys", "gcc"] {
        println!("cargo:rustc-link-lib=static:-bundle={library}");
    }
    println!("cargo:rerun-if-changed=native/support.c");
}
