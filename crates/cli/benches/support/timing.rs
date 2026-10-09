use std::{hint::black_box, time::Instant};

/// Setup and result destruction are excluded. Run serially in release mode.
pub fn measure<S, R>(name: &str, mut setup: impl FnMut() -> S, mut run: impl FnMut(S) -> R) {
    let mut samples = Vec::with_capacity(31);
    for iteration in 0..34 {
        let input = setup();
        let start = Instant::now();
        let output = black_box(run(black_box(input)));
        let elapsed = start.elapsed();
        drop(output);
        if iteration >= 3 {
            samples.push(elapsed);
        }
    }
    samples.sort();
    println!("{name}: min={:?} median={:?} p90={:?} (31 samples, 3 warmups)", samples[0], samples[15], samples[27]);
}
