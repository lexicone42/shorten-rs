// Library crate — many public API items are unused internally but available to consumers.
#![allow(dead_code)]

//! Pure Rust decoder for Shorten (SHN) lossless audio files.
//!
//! Implemented from:
//! - T. Robinson, "SHORTEN: Simple lossless and near-lossless waveform compression"
//!   (Cambridge University Engineering Dept, Technical Report 156, 1994)
//! - Library of Congress format description fdd000199
//!
//! No code derived from SoftSound's reference implementation or FFmpeg's decoder.
//!
//! # Example
//!
//! ```no_run
//! use shn::ShnReader;
//!
//! let mut reader = ShnReader::open("track.shn").unwrap();
//! let info = reader.info();
//! println!("{}ch, {}Hz, {}bit", info.channels, info.sample_rate, info.bits_per_sample);
//!
//! let samples: Vec<i32> = reader.samples().collect::<Result<_, _>>().unwrap();
//! ```

mod bitstream;
mod buffer;
mod decode;
pub mod error;
mod header;

use std::fs::File;
use std::io::Read;
use std::path::Path;

pub use error::ShnError;

/// Metadata about the audio contained in a Shorten file.
#[derive(Debug, Clone)]
pub struct ShnInfo {
    /// Number of audio channels (1 = mono, 2 = stereo).
    pub channels: u32,
    /// Sample rate in Hz (e.g. 44100).
    pub sample_rate: u32,
    /// Bits per sample (typically 16).
    pub bits_per_sample: u32,
}

/// A reader that decodes Shorten (SHN) audio from any `Read` source.
///
/// Modeled after `claxon::FlacReader` — open a file, read metadata, then
/// iterate over decoded PCM samples.
pub struct ShnReader<R: Read> {
    decoder: decode::Decoder<R>,
    info: ShnInfo,
}

impl ShnReader<File> {
    /// Open a Shorten file by path.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, ShnError> {
        let file = File::open(path)?;
        Self::new(file)
    }
}

impl<R: Read> ShnReader<R> {
    /// Create a new ShnReader from any `Read` source.
    ///
    /// Parses the Shorten header and embedded WAVE header immediately.
    /// After construction, call `info()` for metadata and `samples()` for audio.
    pub fn new(reader: R) -> Result<Self, ShnError> {
        let mut bit_reader = bitstream::BitReader::new(reader);
        let (shn_header, wave_info) = header::parse_header(&mut bit_reader)?;

        let info = ShnInfo {
            channels: wave_info.channels,
            sample_rate: wave_info.sample_rate,
            bits_per_sample: wave_info.bits_per_sample,
        };

        let decoder = decode::Decoder::new(bit_reader, &shn_header);

        Ok(ShnReader { decoder, info })
    }

    /// Get metadata about the audio stream.
    pub fn info(&self) -> &ShnInfo {
        &self.info
    }

    /// Returns an iterator that yields decoded PCM samples as `Result<i32>`.
    ///
    /// Samples are interleaved for multi-channel files:
    /// `[ch0_s0, ch1_s0, ch0_s1, ch1_s1, ...]`
    pub fn samples(&mut self) -> ShnSamples<'_, R> {
        ShnSamples {
            decoder: &mut self.decoder,
        }
    }

    /// Consume the reader and return the underlying `Read` source.
    pub fn into_inner(self) -> R {
        self.decoder.reader.into_inner()
    }
}

/// Iterator over decoded PCM samples from a Shorten file.
///
/// Each call to `next()` yields one sample as `Result<i32, ShnError>`.
/// For stereo files, samples alternate between channels.
pub struct ShnSamples<'a, R: Read> {
    decoder: &'a mut decode::Decoder<R>,
}

impl<R: Read> Iterator for ShnSamples<'_, R> {
    type Item = Result<i32, ShnError>;

    fn next(&mut self) -> Option<Self::Item> {
        // Try to get a sample from the current output buffer
        if let Some(s) = self.decoder.next_sample() {
            return Some(Ok(s));
        }

        // Buffer exhausted — decode the next block
        if self.decoder.finished {
            return None;
        }

        match self.decoder.decode_next_block() {
            Ok(true) => self.decoder.next_sample().map(Ok),
            Ok(false) => None, // Stream ended
            Err(e) => Some(Err(e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shn_info_display() {
        let info = ShnInfo {
            channels: 2,
            sample_rate: 44100,
            bits_per_sample: 16,
        };
        assert_eq!(info.channels, 2);
        assert_eq!(info.sample_rate, 44100);
        assert_eq!(info.bits_per_sample, 16);
    }
}
