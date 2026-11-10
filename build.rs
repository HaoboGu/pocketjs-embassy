use std::{env, path::PathBuf, process::Command};

fn main() {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let qjs = root.join(".deps/quickjs");
    let pocket = root.join(".deps/pocketjs");
    assert!(qjs.join("quickjs.c").exists(), "run tools/setup.sh first");
    let output_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let mut build = cc::Build::new();
    build
        .include(&qjs)
        .include(pocket.join("contracts/generated"))
        .include(pocket.join("engine/guest-quickjs/include"))
        .include(pocket.join("engine/guest-quickjs/src"))
        .flag_if_supported("-std=gnu11")
        .flag_if_supported("-fno-strict-aliasing")
        .define("NDEBUG", None)
        .define("__STDC_NO_ATOMICS__", "1")
        .define("malloc", "pocket_c_malloc")
        .define("calloc", "pocket_c_calloc")
        .define("realloc", "pocket_c_realloc")
        .define("free", "pocket_c_free")
        .define("malloc_size", "pocket_c_usable_size")
        .define("malloc_usable_size", "pocket_c_usable_size")
        .warnings(false);
    for file in ["quickjs.c", "dtoa.c", "libregexp.c", "libunicode.c"] {
        build.file(qjs.join(file));
    }
    build
        .file(pocket.join("engine/guest-quickjs/src/guest.c"))
        .file(root.join("native/guest.c"));
    if env::var("TARGET").unwrap() == "riscv32imafc-unknown-none-elf" {
        let toolchain = root.join(".deps/riscv32-esp-elf/bin");
        for function in ["printf", "vprintf", "sprintf", "snprintf", "vsnprintf"] {
            build.define(function, format!("pocket_libc_{function}").as_str());
        }
        println!("cargo:rerun-if-env-changed=POCKETJS_QJS_OPT_LEVEL");
        let optimization = env::var("POCKETJS_QJS_OPT_LEVEL").unwrap_or_else(|_| "2".into());
        build
            .compiler(toolchain.join("riscv32-esp-elf-gcc"))
            .archiver(toolchain.join("riscv32-esp-elf-ar"))
            .opt_level_str(&optimization)
            .flag("-mabi=ilp32f")
            .define("POCKET_EMBEDDED", None);
        println!("cargo:rerun-if-env-changed=POCKETJS_QJS_IRAM");
        if env::var("POCKETJS_QJS_IRAM").as_deref() != Ok("0") {
            build.define("POCKET_QJS_IRAM", None);
        }
        if env::var_os("CARGO_FEATURE_FRAME_PROFILE").is_some() {
            build.define("POCKET_FRAME_PROFILE", None);
        }
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
    }
    build.compile("pocket_quickjs");
    if env::var("TARGET").unwrap() == "riscv32imafc-unknown-none-elf" {
        for library in ["m", "pocket_c", "nosys", "gcc"] {
            println!("cargo:rustc-link-lib=static:-bundle={library}");
        }
    }
    println!("cargo:rerun-if-changed=native/guest.c");
    println!("cargo:rerun-if-changed=.deps/quickjs/quickjs.c");
    println!("cargo:rerun-if-changed=.deps/pocketjs/engine/guest-quickjs/src/guest.c");
}
