# shorten-rs

[![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)](https://www.rust-lang.org/) [![Built with Claude Code](https://img.shields.io/badge/Built%20with-Claude%20Code-6B48FF?logo=anthropic&logoColor=white)](https://claude.ai/claude-code)

Pure Rust decoder for [Shorten (SHN)](https://en.wikipedia.org/wiki/Shorten_(file_format)) lossless audio files. Zero dependencies.

Shorten is a lossless audio codec from 1994, still widely used in live music trading communities (Grateful Dead, Phish, etc.) for archival concert recordings.

## Usage

```rust
use shorten_rs::ShnReader;

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

This decoder was written without reference to existing source code (SoftSound's reference implementation, FFmpeg's decoder, or any other implementation). Developed from public technical documentation and empirical testing against real-world SHN files.

## License

MIT
