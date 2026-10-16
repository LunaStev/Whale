// SPDX-License-Identifier: MPL-2.0
use std::{
    env, fs,
    io::Read,
    path::{Path, PathBuf},
};

const ABI_VERSION: &str = "1";

const FNV_OFFSET: u64 = 0xcbf29ce484222325;

fn update(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    hash
}

fn file_hash(path: &Path, mut hash: u64) -> Result<u64, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buffer = [0; 16384];
    loop {
        let count = file.read(&mut buffer).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(hash);
        }
        hash = update(hash, &buffer[..count]);
    }
}

fn verify(directory: &Path, manifest: &Path) -> Result<(), String> {
    let archive = directory.join("libwhale_wave_reverse.a");
    let metadata = directory.join("reverse-core.meta");
    let kernels = [manifest.join("wave/amd64.wave"), manifest.join("wave/cfg.wave")];
    for path in [&archive, &metadata, &kernels[0], &kernels[1]] {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let file = fs::File::open(&metadata).map_err(|e| format!("missing reverse-core.meta: {e}"))?;
    let mut text = String::new();
    file.take(4097).read_to_string(&mut text).map_err(|e| e.to_string())?;
    if text.len() > 4096 {
        return Err("oversized reverse-core.meta".into());
    }
    let mut fields = std::collections::BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line.split_once('=').ok_or("malformed reverse-core.meta")?;
        if !["schema", "abi_version", "source_fnv64", "archive_fnv64"].contains(&key)
            || fields.insert(key, value).is_some()
        {
            return Err("unknown or duplicate reverse-core.meta field".into());
        }
    }
    if fields.len() != 4 || fields.get("schema") != Some(&"1") {
        return Err("unsupported reverse-core.meta schema".into());
    }
    if fields.get("abi_version") != Some(&ABI_VERSION) {
        return Err("Wave reverse ABI revision mismatch".into());
    }
    let source = file_hash(&kernels[0], FNV_OFFSET)?;
    let source = file_hash(&kernels[1], update(source, b"\n"))?;
    if fields.get("source_fnv64") != Some(&format!("{source:016x}").as_str()) {
        return Err("stale Wave reverse archive: kernel source fingerprint mismatch".into());
    }
    let archive_hash = file_hash(&archive, FNV_OFFSET)?;
    if fields.get("archive_fnv64") != Some(&format!("{archive_hash:016x}").as_str()) {
        return Err("corrupt or mismatched Wave reverse archive fingerprint".into());
    }
    Ok(())
}

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
    let directory =
        PathBuf::from(directory).canonicalize().expect("WHALE_WAVE_REVERSE_DIR must exist");
    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    if let Err(error) = verify(&directory, &manifest) {
        panic!("{error}; rebuild with tools/build_wave_reverse.py before enabling Wave reverse");
    }
    println!("cargo:rustc-link-search=native={}", directory.display());
    println!("cargo:rustc-link-lib=static=whale_wave_reverse");
    println!("cargo:rustc-cfg=whale_wave_reverse");
}
