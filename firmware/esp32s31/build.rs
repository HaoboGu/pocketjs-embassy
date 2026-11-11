fn main() {
    println!("cargo:rerun-if-changed=keyboard.toml");
    println!("cargo:rerun-if-changed=rwtext_hook.x");
    println!(
        "cargo:rustc-link-search={}",
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
    );
    println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
}
