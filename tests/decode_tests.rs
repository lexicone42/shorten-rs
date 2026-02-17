use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Read a WAV file and return raw PCM samples as i16.
fn read_wav_samples(path: &Path) -> Vec<i16> {
    let mut f = File::open(path).unwrap();
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).unwrap();

    // Find "data" chunk
    let mut pos = 12; // skip RIFF header
    while pos + 8 <= buf.len() {
        let chunk_id = &buf[pos..pos + 4];
        let chunk_size = u32::from_le_bytes([buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7]]) as usize;
        if chunk_id == b"data" {
            let data = &buf[pos + 8..pos + 8 + chunk_size];
            return data
                .chunks_exact(2)
                .map(|c| i16::from_le_bytes([c[0], c[1]]))
                .collect();
        }
        pos += 8 + chunk_size;
        if !chunk_size.is_multiple_of(2) {
            pos += 1;
        }
    }
    panic!("no data chunk found in WAV");
}

#[test]
fn decode_stereo_test() {
    let shn_path = Path::new("tests/data/stereo/test.shn");
    let wav_path = Path::new("tests/data/stereo/test.wav");

    if !shn_path.exists() || !wav_path.exists() {
        eprintln!("Skipping real file test — test files not found");
        return;
    }

    let mut reader = shn::ShnReader::open(shn_path).expect("failed to open SHN");
    let info = reader.info().clone();
    assert_eq!(info.channels, 2, "expected stereo");
    assert_eq!(info.sample_rate, 44100, "expected 44.1kHz");
    assert_eq!(info.bits_per_sample, 16, "expected 16-bit");

    let shn_samples: Vec<i32> = reader
        .samples()
        .collect::<Result<_, _>>()
        .expect("failed to decode SHN");

    let wav_samples = read_wav_samples(wav_path);

    assert_eq!(shn_samples.len(), wav_samples.len(),
        "sample count mismatch: SHN={} WAV={}", shn_samples.len(), wav_samples.len());

    // Find first mismatch and print context
    let mut first_mismatch = None;
    let mut mismatches = 0;
    let mut max_diff: i32 = 0;
    for (i, (&shn_s, &wav_s)) in shn_samples.iter().zip(wav_samples.iter()).enumerate() {
        let diff = (shn_s - wav_s as i32).abs();
        if diff != 0 {
            if first_mismatch.is_none() {
                first_mismatch = Some(i);
                // Print context around first mismatch
                let start = if i > 10 { i - 10 } else { 0 };
                let end = (i + 20).min(shn_samples.len());
                eprintln!("\n=== First mismatch at sample {} ===", i);
                eprintln!("Frame={}, Channel={}", i / 2, i % 2);
                eprintln!("Block (per-ch)={}, Sample-in-block={}", (i / 2) / 256, (i / 2) % 256);
                for j in start..end {
                    let s = shn_samples[j];
                    let w = wav_samples[j] as i32;
                    let d = s - w;
                    let marker = if d != 0 { " <<<" } else { "" };
                    eprintln!("  [{:5}] ch{} SHN={:6} WAV={:6} diff={:3}{}",
                        j, j % 2, s, w, d, marker);
                }
            }
            mismatches += 1;
            max_diff = max_diff.max(diff);
        }
    }

    // Also check: are ALL errors on the same channel?
    if mismatches > 0 {
        let ch0_errors: usize = shn_samples.iter().zip(wav_samples.iter()).enumerate()
            .filter(|(i, (&s, &w))| i % 2 == 0 && s != w as i32).count();
        let ch1_errors: usize = shn_samples.iter().zip(wav_samples.iter()).enumerate()
            .filter(|(i, (&s, &w))| i % 2 == 1 && s != w as i32).count();
        eprintln!("\nError distribution: ch0={}, ch1={}", ch0_errors, ch1_errors);
        eprintln!("Total mismatches: {} out of {} (max diff: {})", mismatches, shn_samples.len(), max_diff);

        panic!("{} sample mismatches out of {} (max diff: {})",
            mismatches, shn_samples.len(), max_diff);
    }

    eprintln!("Decoded {} samples successfully ({} channels, {}Hz)",
        shn_samples.len(), info.channels, info.sample_rate);
}

#[test]
fn decode_mono_test() {
    let shn_path = Path::new("tests/data/mono/test.shn");
    let wav_path = Path::new("tests/data/mono/test.wav");

    if !shn_path.exists() || !wav_path.exists() {
        eprintln!("Skipping mono test — test files not found");
        return;
    }

    let mut reader = shn::ShnReader::open(shn_path).expect("failed to open SHN");
    let info = reader.info().clone();
    assert_eq!(info.channels, 1, "expected mono");
    assert_eq!(info.bits_per_sample, 16, "expected 16-bit");

    let shn_samples: Vec<i32> = reader
        .samples()
        .collect::<Result<_, _>>()
        .expect("failed to decode SHN");

    let wav_samples = read_wav_samples(wav_path);

    assert_eq!(shn_samples.len(), wav_samples.len(),
        "sample count mismatch: SHN={} WAV={}", shn_samples.len(), wav_samples.len());

    let mut mismatches = 0;
    let mut max_diff: i32 = 0;
    let mut first_mismatch = None;
    for (i, (&shn_s, &wav_s)) in shn_samples.iter().zip(wav_samples.iter()).enumerate() {
        let diff = (shn_s - wav_s as i32).abs();
        if diff != 0 {
            if first_mismatch.is_none() {
                first_mismatch = Some(i);
                let start = if i > 5 { i - 5 } else { 0 };
                let end = (i + 10).min(shn_samples.len());
                eprintln!("\n=== First mismatch at sample {} (block {}, pos {}) ===",
                    i, i / 256, i % 256);
                for j in start..end {
                    let s = shn_samples[j];
                    let w = wav_samples[j] as i32;
                    let d = s - w;
                    let marker = if d != 0 { " <<<" } else { "" };
                    eprintln!("  [{:5}] SHN={:6} WAV={:6} diff={:3}{}", j, s, w, d, marker);
                }
            }
            mismatches += 1;
            max_diff = max_diff.max(diff);
        }
    }

    if mismatches > 0 {
        panic!("{} sample mismatches out of {} (max diff: {})",
            mismatches, shn_samples.len(), max_diff);
    }

    eprintln!("Decoded {} mono samples successfully ({}Hz)",
        shn_samples.len(), info.sample_rate);
}
