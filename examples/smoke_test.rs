fn main() {
    let path = std::env::args().nth(1).expect("usage: shn_smoke <file.shn>");
    let mut reader = shn::ShnReader::open(&path).expect("open failed");
    let info = reader.info().clone();
    println!("{}ch {}Hz {}bit", info.channels, info.sample_rate, info.bits_per_sample);

    let samples: Vec<i32> = reader.samples()
        .collect::<Result<_, _>>()
        .expect("decode failed");
    
    let duration = samples.len() as f64 / (info.sample_rate as f64 * info.channels as f64);
    println!("{} samples, {:.1}s", samples.len(), duration);
    println!("first 8: {:?}", &samples[..8.min(samples.len())]);
}
