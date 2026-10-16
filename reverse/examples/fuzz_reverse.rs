// SPDX-License-Identifier: MPL-2.0
//! Bounded, deterministic native Wave decoder/CFG mutation and replay probe.
use reverse::*;

use std::{
    env, fs,
    io::{self, Read, Seek, SeekFrom, Write},
    panic::{catch_unwind, AssertUnwindSafe},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_BYTES: usize = 128;

const MAX_CASES: usize = 100_000;

const FIXTURES: &[&[u8]] = &[
    &[0x90, 0xc3],
    &[0x48, 0x89, 0xe5, 0x48, 0x83, 0xec, 8, 0xc3],
    &[0x75, 3, 0x90, 0xeb, 1, 0x90, 0xc3],
    &[0xe8, 0xfb, 0xff, 0xff, 0xff, 0xc3],
    &[0x48, 0x8b, 4, 0x85, 0, 0, 0, 0, 0xc3],
    &[0xf3, 0x45, 0x0f, 0x58, 0xc1, 0xc3],
    &[0xf2, 0x0f, 0x10, 5, 0, 0, 0, 0, 0xc3],
    &[0xff, 0xd0, 0xff, 0xe0],
    &[0x40, 0xb4, 1, 0xc3],
    &[0x66, 0x68, 0, 0x80, 0xc3],
];

fn require(ok: bool, reason: &str) -> Result<(), String> {
    if ok {
        Ok(())
    } else {
        Err(reason.into())
    }
}

fn instruction(input: &[u8], offset: usize) -> Result<Option<Instruction>, String> {
    let result = decode(input, offset);
    require(result == decode(input, offset), "nondeterministic decoder")?;
    let decoded = match result {
        Ok(i) => i,
        Err(
            DecodeError::Invalid { .. }
            | DecodeError::Truncated { .. }
            | DecodeError::Unsupported { .. }
            | DecodeError::Overlong { .. },
        ) => return Ok(None),
        Err(e) => return Err(format!("unexpected decoder failure: {e}")),
    };
    let length = usize::from(decoded.length);
    require(decoded.offset == offset && (1..=15).contains(&length), "decoder offset/length")?;
    let bytes = input.get(offset..offset + length).ok_or("decoder span outside input")?;
    require(decoded.original_bytes() == bytes, "decoder changed original bytes")?;
    let enc = &decoded.encoding;
    require(
        usize::from(enc.opcode_offset) + usize::from(enc.opcode_len) <= length
            && (1..=2).contains(&enc.opcode_len),
        "opcode span",
    )?;
    for byte in [enc.modrm, enc.sib].into_iter().flatten() {
        require(bytes.get(usize::from(byte.offset)) == Some(&byte.value), "encoding byte")?;
    }
    for field in [enc.displacement, enc.immediate].into_iter().flatten() {
        let start = usize::from(field.offset);
        let width = usize::from(field.width);
        require((1..=8).contains(&width), "encoding field width")?;
        let raw = bytes.get(start..start + width).ok_or("encoding field outside instruction")?;
        let mut padded = [0; 8];
        padded[..width].copy_from_slice(raw);
        require(u64::from_le_bytes(padded) == field.bits, "encoding field raw bits")?;
    }
    let mut padded = input.to_vec();
    padded.extend_from_slice(&[0x90; 15]);
    require(decode(&padded, offset) == Ok(decoded.clone()), "suffix changed complete instruction")?;
    Ok(Some(decoded))
}

fn limits() -> CfgLimits {
    CfgLimits {
        max_input_bytes: MAX_BYTES,
        max_instructions: MAX_BYTES,
        max_blocks: MAX_BYTES,
        max_edges: 1024,
        max_stops: 256,
        max_work: 1_000_000,
        max_workspace_bytes: 1024 * 1024,
        follow_calls: true,
    }
}

fn properties(bytes: &[u8]) -> Result<(), String> {
    require(bytes.len() <= MAX_BYTES, "probe input bound")?;
    for offset in 0..=bytes.len() {
        instruction(bytes, offset)?;
    }
    if FIXTURES.contains(&bytes) {
        // This check is also determined by the bytes, so replay includes the
        // positive capability guards and cannot succeed on an error-only core.
        require(decode(bytes, 0).is_ok(), "valid seed fixture did not decode")?;
        let graph = analyze(bytes, 0x1000, &[0], &[], limits()).map_err(|e| e.to_string())?;
        require(!graph.instructions.is_empty(), "valid seed fixture has empty graph")?;
    }
    // All graph parameters are pure functions of the saved bytes: a .bin file
    // alone is sufficient to reproduce the complete decoder and CFG case.
    let mut seeds = Vec::new();
    if let Some(last) = bytes.last() {
        seeds.extend([0, usize::from(*last) % bytes.len()]);
    }
    let base = if bytes.first().is_some_and(|b| b & 4 != 0) { u64::MAX - 256 } else { 0x1000 };
    let mut overlays = Vec::new();
    if bytes.first().is_some_and(|b| b & 1 != 0) {
        if let Ok(i) = decode(bytes, 0) {
            if let Some(f) = i.encoding.displacement.or(i.encoding.immediate) {
                overlays.push(FlowOverride {
                    field_offset: usize::from(f.offset),
                    width: f.width,
                    target: match bytes[0] % 3 {
                        0 => CfgTarget::Unresolved,
                        1 => CfgTarget::Symbol(77),
                        _ => CfgTarget::Address(base + (bytes.len() / 2) as u64),
                    },
                });
            }
        }
    }
    let l = CfgLimits { follow_calls: bytes.first().is_none_or(|b| b & 2 == 0), ..limits() };
    let result = analyze(bytes, base, &seeds, &overlays, l);
    require(result == analyze(bytes, base, &seeds, &overlays, l), "nondeterministic CFG")?;
    let graph = result.map_err(|e| format!("unexpected bounded CFG failure: {e}"))?;
    require(graph.base == base && graph.work <= l.max_work, "CFG base/work accounting")?;
    let mut owners = vec![None; bytes.len()];
    let mut membership = vec![0; graph.instructions.len()];
    for (index, i) in graph.instructions.iter().enumerate() {
        require(instruction(bytes, i.offset)? == Some(i.clone()), "CFG instruction mismatch")?;
        let end = i.offset + usize::from(i.length);
        for owner in owners.get_mut(i.offset..end).ok_or("CFG span")? {
            require(owner.is_none(), "overlapping committed instructions")?;
            *owner = Some(index);
        }
    }
    let mut previous_end = 0;
    for block in &graph.blocks {
        require(
            block.offset >= previous_end && block.offset < block.end,
            "block order/nonempty interval",
        )?;
        let mut cursor = block.offset;
        for index in &block.instruction_indices {
            let i = graph.instructions.get(*index).ok_or("block instruction index")?;
            require(i.offset == cursor, "noncontiguous block")?;
            cursor += usize::from(i.length);
            membership[*index] += 1;
        }
        require(cursor == block.end, "block end")?;
        previous_end = block.end;
    }
    require(membership.iter().all(|count| *count == 1), "instruction block partition")?;
    for edge in &graph.edges {
        require(graph.instructions.iter().any(|i| i.offset == edge.from), "edge source")?;
    }
    for stop in &graph.stops {
        if let Some(conflict) = stop.conflict {
            require(
                stop.kind == StopKind::Overlap
                    && graph.instructions.iter().any(|i| i.offset == conflict),
                "overlap owner",
            )?;
        }
    }
    let tiny = CfgLimits { max_work: 0, ..l };
    require(
        matches!(
            analyze(bytes, base, &seeds, &overlays, tiny),
            Err(CfgError::Limit { resource: CfgResource::Work, .. })
        ),
        "zero work must reject atomically",
    )?;
    Ok(())
}

fn check(bytes: &[u8]) -> Result<(), String> {
    catch_unwind(AssertUnwindSafe(|| properties(bytes)))
        .unwrap_or_else(|_| Err("Rust panic".into()))
}

fn next(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e3779b97f4a7c15);
    let mut x = *state;
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}

fn generated(state: &mut u64, case: usize) -> Vec<u8> {
    let mut bytes = FIXTURES[next(state) as usize % FIXTURES.len()].to_vec();
    if case % 17 == 0 {
        return bytes;
    }
    if case % 7 == 0 {
        bytes = (0..next(state) as usize % (MAX_BYTES + 1)).map(|_| next(state) as u8).collect();
    } else {
        for _ in 0..1 + next(state) % 8 {
            let at = next(state) as usize % (bytes.len() + 1);
            match next(state) % 4 {
                0 if at < bytes.len() => {
                    bytes.remove(at);
                },
                1 if bytes.len() < MAX_BYTES => {
                    bytes.insert(at, next(state) as u8);
                },
                2 if at < bytes.len() => {
                    bytes[at] ^= 1 << (next(state) % 8);
                },
                _ if at < bytes.len() => bytes[at] = next(state) as u8,
                _ => {},
            }
        }
    }
    bytes
}

fn preserve(
    directory: &Path,
    bytes: &[u8],
    reason: &str,
    seed: u64,
    case: usize,
    mut fails: impl FnMut(&[u8]) -> bool,
) -> io::Result<PathBuf> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let prefix = directory.join(format!("failure-{stamp}-{}", std::process::id()));
    let original = prefix.with_extension("original.bin");
    // Never minimize or overwrite the original. A native signal also leaves
    // current.bin, written before entering Wave, available for exact replay.
    fs::write(&original, bytes)?;
    fs::write(prefix.with_extension("txt"), format!("seed={seed}\ncase={case}\n{reason}\n"))?;
    let mut minimized = bytes.to_vec();
    let mut at = 0;
    for _ in 0..256 {
        if at >= minimized.len() {
            break;
        }
        let mut candidate = minimized.clone();
        candidate.remove(at);
        if fails(&candidate) {
            minimized = candidate;
        } else {
            at += 1;
        }
    }
    fs::write(prefix.with_extension("min.bin"), minimized)?;
    Ok(original)
}

fn prepare_run(
    directory: &Path,
    replay: Option<PathBuf>,
) -> Result<(Option<Vec<u8>>, fs::File), String> {
    // Load replay before opening the journal: the selected replay may itself
    // be current.bin in this same directory and must not be truncated first.
    let replay = replay
        .map(|path| -> Result<Vec<u8>, String> {
            let mut bytes = Vec::new();
            fs::File::open(path)
                .map_err(|e| e.to_string())?
                .take((MAX_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            require(bytes.len() <= MAX_BYTES, "replay exceeds 128-byte probe bound")?;
            Ok(bytes)
        })
        .transpose()?;
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let current = fs::File::create(directory.join("current.bin")).map_err(|e| e.to_string())?;
    Ok((replay, current))
}

fn run() -> Result<(), String> {
    let mut seed = 0x5748414c455f5231u64;
    let mut cases = 10_000;
    let mut replay = None;
    let mut directory = PathBuf::from("target/fuzz-reverse");
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        let value =
            args.next().ok_or("expected --seed N, --cases N, --replay FILE or --out-dir DIR")?;
        match arg.as_str() {
            "--seed" => seed = value.parse().map_err(|_| "invalid decimal seed")?,
            "--cases" => cases = value.parse().map_err(|_| "invalid case count")?,
            "--replay" => replay = Some(PathBuf::from(value)),
            "--out-dir" => directory = value.into(),
            _ => return Err(format!("unknown option {arg}")),
        }
    }
    require((1..=MAX_CASES).contains(&cases), "--cases must be in 1..=100000")?;
    require(
        backend_available(),
        "actual Wave core required; enable wave and WHALE_WAVE_REVERSE_DIR",
    )?;
    let (replay, mut current) = prepare_run(&directory, replay)?;
    let mut execute = |bytes: &[u8], case: usize| -> Result<(), String> {
        current
            .seek(SeekFrom::Start(0))
            .and_then(|_| current.set_len(0))
            .and_then(|_| current.write_all(bytes))
            .and_then(|_| current.flush())
            .map_err(|e| e.to_string())?;
        if let Err(reason) = check(bytes) {
            let original = preserve(&directory, bytes, &reason, seed, case, |candidate| {
                check(candidate).is_err()
            })
            .map_err(|e| e.to_string())?;
            return Err(format!("{reason}; replay --replay {}", original.display()));
        }
        Ok(())
    };
    if let Some(bytes) = replay {
        execute(&bytes, 0)?;
        println!("Wave reverse replay passed ({} bytes)", bytes.len());
    } else {
        // A broken/missing native backend cannot pass by accepting only errors.
        for (case, bytes) in FIXTURES.iter().enumerate() {
            execute(bytes, case)?;
        }
        let mut state = seed;
        for case in 0..cases {
            execute(&generated(&mut state, case), case)?;
        }
        println!(
            "Wave reverse passed {} valid fixtures and {cases} mutations (seed {seed})",
            FIXTURES.len()
        );
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary() -> PathBuf {
        let directory = env::temp_dir().join(format!(
            "reverse-probe-test-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn replay_journal_is_loaded_before_it_is_truncated() {
        let directory = temporary();
        let path = directory.join("current.bin");
        fs::write(&path, FIXTURES[0]).unwrap();
        let (replay, _journal) = prepare_run(&directory, Some(path.clone())).unwrap();
        assert_eq!(replay.as_deref(), Some(FIXTURES[0]));
        fs::write(&path, [0; MAX_BYTES + 1]).unwrap();
        assert!(prepare_run(&directory, Some(path.clone())).is_err());
        assert_eq!(fs::read(path).unwrap().len(), MAX_BYTES + 1);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn original_failure_is_saved_before_bounded_minimization() {
        let directory = temporary();
        let bytes = vec![0x90; 1024];
        fs::write(directory.join("current.bin"), &bytes).unwrap();
        let mut attempts = 0;
        let original = preserve(&directory, &bytes, "injected failure", 1, 0, |_| {
            let original = fs::read_dir(&directory)
                .unwrap()
                .map(|p| p.unwrap().path())
                .find(|p| p.to_string_lossy().ends_with("original.bin"))
                .unwrap();
            assert_eq!(fs::read(original).unwrap(), bytes);
            attempts += 1;
            true
        })
        .unwrap();
        assert_eq!(attempts, 256);
        assert_eq!(fs::read(original).unwrap(), bytes);
        assert_eq!(fs::read(directory.join("current.bin")).unwrap(), bytes);
        fs::remove_dir_all(directory).unwrap();
    }
}
