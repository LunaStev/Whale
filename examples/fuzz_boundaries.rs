#[path = "../fuzz/support.rs"]
mod fuzz;

use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    path::PathBuf,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let target = fuzz::Target::parse(&args.next().expect("target: assembler|object|ir|ast"))
        .expect("unknown target");
    let mode = args.next().expect("cases count or --replay");
    if mode == "--replay" {
        let path = args.next().expect("replay path");
        let bytes = fuzz::read_bounded(std::path::Path::new(&path));
        fuzz::check(target, &bytes);
        println!("replay passed");
        return;
    }
    let cases: usize = mode.parse().unwrap();
    assert!((1..=100000).contains(&cases));
    let seed: u64 = args.next().unwrap_or_else(|| "1".into()).parse().unwrap();
    let corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fuzz/corpus");
    let seeds = fuzz::seeds(&corpus, target);
    let mut seed_accepted = 0;
    for (_, bytes) in &seeds {
        seed_accepted += usize::from(fuzz::check(target, bytes));
    }
    assert!(seed_accepted > 0, "corpus needs a valid seed");
    let mut rng = fuzz::Rng::new(seed);
    let mut accepted = seed_accepted;
    for case in 0..cases {
        let (path, bytes) = &seeds[rng.index(seeds.len())];
        let input = if case % 8 == 0 { bytes.clone() } else { fuzz::mutate(bytes, &mut rng) };
        match catch_unwind(AssertUnwindSafe(|| fuzz::check(target, &input))) {
            Ok(valid) => accepted += usize::from(valid),
            Err(_) => {
                std::panic::set_hook(Box::new(|_| {}));
                let directory = std::env::var_os("WHALE_FUZZ_ARTIFACT_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| std::env::temp_dir().join("whale-fuzz"));
                std::fs::create_dir_all(&directory).unwrap();
                let crash = directory.join(format!("{}-{seed}-{case}.crash", target.name()));
                let original = directory.join(format!("{}-{seed}-{case}.original", target.name()));
                // Retain evidence before minimization: the process may hit its
                // independent time/memory limit while reducing a failing input.
                std::fs::write(&original, &input).unwrap();
                std::fs::write(directory.join(format!("{}-{seed}-{case}.replay", target.name())),
                    format!("origin={}\nreplay: cargo run --locked --all-features --example fuzz_boundaries -- {} --replay {}\n",path.display(),target.name(),original.display())).unwrap();
                let attempts = std::cell::Cell::new(0usize);
                let started = std::time::Instant::now();
                let minimal = fuzz::minimize(input, |b| {
                    if attempts.get() >= 256 || started.elapsed().as_secs() >= 5 {
                        return false;
                    }
                    attempts.set(attempts.get() + 1);
                    catch_unwind(AssertUnwindSafe(|| fuzz::check(target, b))).is_err()
                });
                std::fs::write(&crash, minimal).unwrap();
                eprintln!("seed={} case={case} origin={} replay: cargo run --locked --features socket-cli --example fuzz_boundaries -- {} --replay {}",seed,path.display(),target.name(),crash.display());
                std::process::exit(1);
            },
        }
    }
    assert!(accepted > 0, "fuzz invocation must execute at least one valid seed");
    println!(
        "{} seed={seed} cases={cases} accepted={accepted} max_input={} passed",
        target.name(),
        fuzz::MAX_INPUT
    );
}
