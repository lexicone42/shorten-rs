# Shorten (SHN) Audio Format

Quick reference for the Shorten lossless audio codec, as implemented by this crate.

## References

- T. Robinson, "SHORTEN: Simple lossless and near-lossless waveform compression"
  (Cambridge University Engineering Dept, Technical Report 156, 1994)
- Library of Congress format description fdd000199

## File Layout

```
[4 bytes] Magic: "ajkg"
[1 byte]  Version (1-3; this crate supports all three)
[bitstream from here on — MSB-first bit packing]
  file_type   — ulong (5 = signed 16-bit little-endian / WAV)
  channels    — ulong
  blocksize   — ulong (v2+, default 256)
  maxnlpc     — ulong (v2+, default 0)
  nmean       — ulong (v2+, default 4)
  nskip       — ulong (v2+, default 0)
  [nskip ulongs skipped]
  [VERBATIM blocks containing embedded RIFF/WAVE header]
  [audio commands in round-robin channel order]
  FN_QUIT
```

## Bitstream Primitives

All multi-bit values are packed MSB-first into a byte stream.

### uvar(k) — Unsigned Rice Code

Zeros-before-one (ZBO) encoding:
1. Count leading 0-bits until a 1-bit stop → quotient `q`
2. Read `k` mantissa bits → remainder `r`
3. Value = `(q << k) | r`

### var(k) — Signed Rice Code

1. Read `uvar(k + 1)` → unsigned value `u`
2. Sign-unfold: even → `u/2`, odd → `-(u+1)/2`

The extra mantissa bit (k+1 vs k) accounts for the sign doubling the range.

### ulong — Variable-Length Unsigned Integer

Two-level Rice encoding:
1. `nbits = uvar(2)`
2. `value = uvar(nbits)`

## Commands

Read via `uvar(FNSIZE=2)`:

| ID | Name         | Action |
|----|--------------|--------|
| 0  | FN_DIFF0     | Fixed predictor, order 0 (DC offset only) |
| 1  | FN_DIFF1     | Fixed predictor, order 1: `s[-1]` |
| 2  | FN_DIFF2     | Fixed predictor, order 2: `2*s[-1] - s[-2]` |
| 3  | FN_DIFF3     | Fixed predictor, order 3: `3*s[-1] - 3*s[-2] + s[-3]` |
| 4  | FN_QUIT      | End of stream |
| 5  | FN_BLOCKSIZE | Change block size: `new_bs = ulong()` |
| 6  | FN_BITSHIFT  | Set bit shift: `shift = uvar(2)` |
| 7  | FN_QLPC      | Quantized LPC prediction |
| 8  | FN_ZERO      | Block of silence (all zeros) |
| 9  | FN_VERBATIM  | Raw bytes: `n = uvar(5)`, then `n` bytes each as `uvar(8)` |

## Audio Block Decoding

For DIFF0-3 and QLPC:
1. Read `energy = uvar(ENERGYSIZE=3)` — Rice parameter for residuals
2. For QLPC only: read `order = uvar(LPCQSIZE=2)`, then `order` coefficients via `var(LPCQSIZE=2)`
3. For each sample in block:
   - Read residual via `var(energy)`
   - Compute prediction from history samples (fixed coefficients or LPC)
   - `sample = residual + prediction`
4. Post-process:
   - Apply bitshift: `sample <<= bitshift`
   - Update running mean (store `(block_sum + blocksize/2) / blocksize`)
   - Copy last 3 samples to history for next block

### DC Offset (coffset)

DIFF0 uses a DC offset prediction computed from a rolling window of `nmean` block means:

```
coffset = (sum_of_means + nmean/2) / nmean
```

DIFF1-3 don't use coffset (it algebraically cancels with the predictor coefficients).
QLPC doesn't use coffset (the LPC coefficients naturally capture DC).

### Mean Accumulator

The offset array stores per-block means (not raw sums). After decoding each block:

```
block_mean = (block_sum + blocksize/2) / blocksize
```

This mean is pushed into a circular buffer of size `nmean`. The intermediate rounding
when storing means vs dividing by the product is critical for bit-exact reproduction.

## Channel Ordering

Channels are encoded in round-robin order: one block per channel, cycling.
For stereo output, blocks are interleaved: `[ch0_s0, ch1_s0, ch0_s1, ch1_s1, ...]`

## Key Constants

```
NWRAP = 3       (history samples for predictors)
FNSIZE = 2      (bits for command Rice parameter)
ENERGYSIZE = 3  (bits for residual Rice parameter)
BITSHIFTSIZE = 2
LPCQSIZE = 2
LPCQUANT = 5    (LPC coefficient quantization bits)
ULONGSIZE = 2   (bits for ulong's first-level Rice)
```
