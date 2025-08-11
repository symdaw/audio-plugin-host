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
        // .cxxflag("-stdlib=libc++") // macos
        .build()
        .join("build");

    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        dst.push("Release");
        println!("cargo:rustc-link-lib=ole32");
    } else if std::env::var("CARGO_CFG_TARGET_OS") == Ok("linux".to_string()) {
        println!("cargo:rustc-link-lib=stdc++fs");
    } else if std::env::var("CARGO_CFG_TARGET_OS") == Ok("macos".to_string()) {
        println!("cargo:rustc-link-lib=dylib=objc");
        println!("cargo:rustc-link-lib=framework=Foundation");
        println!("cargo:rustc-link-lib=c++");
    }

    println!("cargo::warning={}", dst.display());

    println!("cargo:rustc-link-search=native={}", dst.display());

    println!("cargo:rustc-link-lib=static=vst3wrapper");
    println!("cargo:rustc-link-lib=static=VST_SDK");
}
