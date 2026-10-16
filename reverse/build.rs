// SPDX-License-Identifier: MPL-2.0
use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rustc-check-cfg=cfg(whale_wave_reverse)");
    println!("cargo:rerun-if-env-changed=WHALE_WAVE_REVERSE_DIR");
    if env::var_os("CARGO_FEATURE_WAVE").is_none() {
        return;
    }
    let Some(directory) = env::var_os("WHALE_WAVE_REVERSE_DIR") else {
        return;
    };
    assert_eq!(
        env::var("HOST").unwrap(),
        "x86_64-unknown-linux-gnu",
        "Wave reverse bootstrap currently requires a Linux x86_64 host"
    );
    assert_eq!(
        env::var("TARGET").unwrap(),
        "x86_64-unknown-linux-gnu",
        "Wave reverse bootstrap does not cross-link yet"
    );
    let directory = PathBuf::from(directory)
        .canonicalize()
        .expect("WHALE_WAVE_REVERSE_DIR must exist");
    let archive = directory.join("libwhale_wave_reverse.a");
    assert!(archive.is_file(), "run tools/build_wave_reverse.py first");
    println!("cargo:rerun-if-changed={}", archive.display());
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=whale_wave_reverse");
    println!("cargo:rustc-cfg=whale_wave_reverse");
}
