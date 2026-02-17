use std::io::Read;

use crate::bitstream::BitReader;
use crate::error::ShnError;

/// Shorten magic bytes.
const MAGIC: &[u8; 4] = b"ajkg";

/// Shorten file types.
pub const TYPE_S8: i32 = 1; // signed 8-bit
pub const TYPE_U8: i32 = 2; // unsigned 8-bit
pub const TYPE_S16HL: i32 = 3; // signed 16-bit, high byte first (big-endian / AIFF)
pub const TYPE_U16HL: i32 = 4; // unsigned 16-bit, high byte first
pub const TYPE_S16LH: i32 = 5; // signed 16-bit, low byte first (little-endian / WAV)
pub const TYPE_U16LH: i32 = 6; // unsigned 16-bit, low byte first

/// Default values for version < 2.
const DEFAULT_V0_NMEAN: usize = 0;
const DEFAULT_V2_NMEAN: usize = 4;
const DEFAULT_BLOCK_SIZE: usize = 256;
const DEFAULT_MAXNLPC: usize = 0;
const DEFAULT_NSKIP: usize = 0;

/// Known command IDs (used to identify the first verbatim command).
pub const FN_VERBATIM: i32 = 9;

/// Bit widths for reading various fields.
const FNSIZE: u32 = 2;
const VERBATIM_CKSIZE_SIZE: u32 = 5;
const VERBATIM_BYTE_SIZE: u32 = 8;

/// Parsed Shorten header information.
#[derive(Debug, Clone)]
pub struct ShnHeader {
    pub version: u8,
    pub file_type: i32,
    pub channels: u32,
    pub blocksize: usize,
    pub maxnlpc: usize,
    pub nmean: usize,
    pub nskip: usize,
    /// The first audio command read after the header (and any initial VERBATIM blocks).
    /// The decoder needs this because we consumed it while looking for the WAVE header.
    pub first_audio_cmd: Option<i32>,
}

/// Information extracted from the embedded WAVE/AIFF header.
#[derive(Debug, Clone)]
pub struct WaveInfo {
    pub sample_rate: u32,
    pub bits_per_sample: u32,
    pub channels: u32,
    /// Total number of audio data bytes (from the "data" chunk size).
    pub data_bytes: u32,
}

/// Parse the Shorten file header and the embedded WAVE header.
///
/// After this returns, the BitReader is positioned just past the first
/// verbatim command, ready to read audio commands.
pub fn parse_header<R: Read>(
    reader: &mut BitReader<R>,
) -> Result<(ShnHeader, WaveInfo), ShnError> {
    // Read 4-byte magic directly (before entering bitstream mode)
    let mut magic = [0u8; 4];
    for b in magic.iter_mut() {
        *b = reader.read_byte_direct()?;
    }
    if &magic != MAGIC {
        return Err(ShnError::InvalidMagic);
    }

    // Read 1-byte version directly
    let version = reader.read_byte_direct()?;
    if version == 0 || version > 3 {
        return Err(ShnError::UnsupportedVersion(version));
    }

    // From here on, all reads go through the bitstream reader
    let file_type = reader.read_ulong()? as i32;
    if !(TYPE_S8..=TYPE_U16LH).contains(&file_type) {
        return Err(ShnError::UnsupportedFileType(file_type));
    }

    let channels = reader.read_ulong()?;

    // Version >= 2 has additional header fields
    let (blocksize, maxnlpc, nmean, nskip) = if version >= 2 {
        let bs = reader.read_ulong()? as usize;
        let maxnlpc = reader.read_ulong()? as usize;
        let nmean = reader.read_ulong()? as usize;
        let nskip = reader.read_ulong()? as usize;
        (bs, maxnlpc, nmean, nskip)
    } else {
        (
            DEFAULT_BLOCK_SIZE,
            DEFAULT_MAXNLPC,
            if version >= 1 {
                DEFAULT_V2_NMEAN
            } else {
                DEFAULT_V0_NMEAN
            },
            DEFAULT_NSKIP,
        )
    };

    // Skip `nskip` bytes (rare, usually 0)
    for _ in 0..nskip {
        reader.read_ulong()?;
    }

    // Read commands looking for VERBATIM blocks that contain the WAVE header.
    // Some SHN files (raw-encoded) don't have VERBATIM blocks at all.
    let mut wave_info = None;
    #[allow(unused_assignments)]
    let mut first_audio_cmd = None;

    loop {
        let cmd = reader.read_unsigned_rice(FNSIZE)? as i32;

        if cmd == FN_VERBATIM {
            let nbytes = reader.read_unsigned_rice(VERBATIM_CKSIZE_SIZE)? as usize;
            let verbatim_data = read_verbatim_bytes(reader, nbytes)?;
            if wave_info.is_none() {
                // Try to parse as WAVE header; ignore if it's not one
                if let Ok(wi) = parse_wave_header(&verbatim_data) {
                    wave_info = Some(wi);
                }
            }
        } else {
            // Non-VERBATIM command = first audio command
            first_audio_cmd = Some(cmd);
            break;
        }
    }

    // If no WAVE header found, infer from the file type
    let wave_info = wave_info.unwrap_or_else(|| {
        let bps = match file_type {
            TYPE_S8 | TYPE_U8 => 8,
            _ => 16,
        };
        WaveInfo {
            sample_rate: 44100, // reasonable default
            bits_per_sample: bps,
            channels,
            data_bytes: 0,
        }
    });

    let header = ShnHeader {
        version,
        file_type,
        channels,
        blocksize,
        maxnlpc,
        nmean,
        nskip,
        first_audio_cmd,
    };

    Ok((header, wave_info))
}

/// Read `n` verbatim bytes from the bitstream.
/// Each byte is Rice-coded with k=VERBATIM_BYTE_SIZE=8.
fn read_verbatim_bytes<R: Read>(reader: &mut BitReader<R>, n: usize) -> Result<Vec<u8>, ShnError> {
    let mut buf = Vec::with_capacity(n);
    for _ in 0..n {
        buf.push(reader.read_unsigned_rice(VERBATIM_BYTE_SIZE)? as u8);
    }
    Ok(buf)
}

/// Parse a RIFF/WAVE header to extract audio parameters.
///
/// The header is embedded as verbatim data in the Shorten stream.
/// We only need: sample rate, bits per sample, channels, and data chunk size.
fn parse_wave_header(data: &[u8]) -> Result<WaveInfo, ShnError> {
    if data.len() < 44 {
        return Err(ShnError::MissingWaveHeader);
    }

    // Check RIFF magic
    if &data[0..4] != b"RIFF" {
        return Err(ShnError::MissingWaveHeader);
    }
    // Check WAVE magic
    if &data[8..12] != b"WAVE" {
        return Err(ShnError::MissingWaveHeader);
    }

    // Find the "fmt " sub-chunk
    let mut pos = 12;
    let mut fmt_found = false;
    let mut sample_rate = 0u32;
    let mut bits_per_sample = 0u32;
    let mut channels = 0u32;

    while pos + 8 <= data.len() {
        let chunk_id = &data[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([
            data[pos + 4],
            data[pos + 5],
            data[pos + 6],
            data[pos + 7],
        ]) as usize;

        if chunk_id == b"fmt " {
            if pos + 8 + chunk_size > data.len() || chunk_size < 16 {
                return Err(ShnError::MissingWaveHeader);
            }
            let fmt_data = &data[pos + 8..];
            // AudioFormat (2 bytes) — should be 1 (PCM)
            // NumChannels (2 bytes)
            channels = u16::from_le_bytes([fmt_data[2], fmt_data[3]]) as u32;
            // SampleRate (4 bytes)
            sample_rate = u32::from_le_bytes([
                fmt_data[4],
                fmt_data[5],
                fmt_data[6],
                fmt_data[7],
            ]);
            // BitsPerSample (2 bytes, at offset 14 in fmt chunk)
            bits_per_sample = u16::from_le_bytes([fmt_data[14], fmt_data[15]]) as u32;
            fmt_found = true;
        }

        if chunk_id == b"data" {
            if !fmt_found {
                return Err(ShnError::MissingWaveHeader);
            }
            return Ok(WaveInfo {
                sample_rate,
                bits_per_sample,
                channels,
                data_bytes: chunk_size as u32,
            });
        }

        // Advance to next chunk (chunks are word-aligned)
        pos += 8 + chunk_size;
        if !chunk_size.is_multiple_of(2) {
            pos += 1;
        }
    }

    // If we found fmt but no data chunk, the verbatim block may not contain
    // the data chunk header. In this case, data_bytes is unknown.
    if fmt_found {
        return Ok(WaveInfo {
            sample_rate,
            bits_per_sample,
            channels,
            data_bytes: 0,
        });
    }

    Err(ShnError::MissingWaveHeader)
}
