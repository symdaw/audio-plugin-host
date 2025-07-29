use std::env;

use cmake::Config;

fn main() {
    let crate_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    cbindgen::Builder::new()
        .with_crate(crate_dir)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("vst3-wrapper/source/bindings.h");

    let mut dst = Config::new("vst3-wrapper")
        .build_target("vst3wrapper")
        .profile("Release")
        .no_default_flags(true)
        .build()
        .join("build");

    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        dst.push("Release");
        println!("cargo:rustc-link-lib=ole32");
    } else {
        println!("cargo:rustc-link-lib=stdc++fs");
    }

    println!("cargo::warning={}", dst.display());

    println!("cargo:rustc-link-search=native={}", dst.display());

    println!("cargo:rustc-link-lib=static=vst3wrapper");
    println!("cargo:rustc-link-lib=static=VST_SDK");
}
