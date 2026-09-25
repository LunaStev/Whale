// SPDX-License-Identifier: MPL-2.0
use std::{env, path::PathBuf};
fn main() {
    println!("cargo:rustc-check-cfg=cfg(whale_wave_elf)");
    println!("cargo:rerun-if-env-changed=WHALE_WAVE_ELF_DIR");
    if let Some(directory) = env::var_os("WHALE_WAVE_ELF_DIR") {
        assert_eq!(
            env::var("HOST").unwrap(),
            "x86_64-unknown-linux-gnu",
            "Wave ELF bootstrap currently requires a Linux x86_64 host"
        );
        assert_eq!(
            env::var("TARGET").unwrap(),
            "x86_64-unknown-linux-gnu",
            "Wave ELF bootstrap does not support cross-compiling Whale yet"
        );
        let directory = PathBuf::from(directory)
            .canonicalize()
            .expect("WHALE_WAVE_ELF_DIR must exist");
        let archive = directory.join("libwhale_wave_elf.a");
        assert!(
            archive.is_file(),
            "run tools/build_wave_elf.py to build libwhale_wave_elf.a"
        );
        println!("cargo:rerun-if-changed={}", archive.display());
        println!("cargo:rustc-link-search=native={}", directory.display());
        println!("cargo:rustc-link-lib=static=whale_wave_elf");
        println!("cargo:rustc-cfg=whale_wave_elf");
    }
}
