//! Measures how long password key derivation takes for a few Argon2id cost
//! settings, so the default can be chosen from numbers on real machines.
//!
//! Run with: `cargo run --release -p ne-crypto --example argon2_bench`

use std::time::Instant;

use ne_crypto::password::{KdfParams, derive_keys};

const RUNS: u32 = 3;

fn main() {
    let candidates = [
        (19 * 1024, 2, 1),
        (46 * 1024, 1, 1),
        (64 * 1024, 3, 4),
        (128 * 1024, 3, 4),
        (256 * 1024, 3, 4),
    ];
    let salt = [7u8; 16];

    println!("memory_mib  iterations  parallelism  median_ms");
    for (memory_kib, iterations, parallelism) in candidates {
        let params = KdfParams {
            memory_kib,
            iterations,
            parallelism,
        };
        let mut samples: Vec<u128> = (0..RUNS)
            .map(|_| {
                let start = Instant::now();
                derive_keys(b"benchmark password", &salt, params).expect("derive keys");
                start.elapsed().as_millis()
            })
            .collect();
        samples.sort_unstable();
        println!(
            "{:>10}  {:>10}  {:>11}  {:>9}",
            memory_kib / 1024,
            iterations,
            parallelism,
            samples[samples.len() / 2]
        );
    }
}
