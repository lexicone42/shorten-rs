# shn

Pure Rust decoder for [Shorten (SHN)](https://en.wikipedia.org/wiki/Shorten_(file_format)) lossless audio files. Zero dependencies.

Shorten is a lossless audio codec from 1994, still widely used in live music trading communities (Grateful Dead, Phish, etc.) for archival concert recordings.

## Usage

```rust
use shn::ShnReader;

let mut reader = ShnReader::open("track.shn")?;
let info = reader.info();
println!("{}ch, {}Hz, {}bit", info.channels, info.sample_rate, info.bits_per_sample);

// Decode all samples (interleaved i32 PCM)
for sample in reader.samples() {
    let sample = sample?;
    // process sample...
}
```

Also works with any `Read` source:

```rust
use std::io::Cursor;
let reader = ShnReader::new(Cursor::new(shn_bytes))?;
```

## Supported formats

- Shorten versions 1, 2, 3
- 16-bit signed PCM (little-endian WAV and big-endian AIFF)
- Mono and stereo
- Fixed prediction (DIFF0-3) and quantized LPC (QLPC)
- Embedded RIFF/WAVE header extraction

This covers essentially all SHN files found in the wild.

## Clean-room implementation

Implemented from public, unencumbered sources:

- T. Robinson, "SHORTEN: Simple lossless and near-lossless waveform compression" (Cambridge University Engineering Dept, Technical Report 156, 1994)
- Library of Congress format description [fdd000199](https://www.loc.gov/preservation/digital/formats/fdd/fdd000199.shtml)

No code derived from SoftSound's reference implementation or FFmpeg's decoder.

## License

MIT
