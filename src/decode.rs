use std::io::Read;

use crate::bitstream::BitReader;
use crate::buffer::{ChannelBuffer, MeanAccumulator};
use crate::error::ShnError;
use crate::header::ShnHeader;

// ─── Command IDs ─────────────────────────────────────────────────────────────
const FN_DIFF0: i32 = 0;
const FN_DIFF1: i32 = 1;
const FN_DIFF2: i32 = 2;
const FN_DIFF3: i32 = 3;
const FN_QUIT: i32 = 4;
const FN_BLOCKSIZE: i32 = 5;
const FN_BITSHIFT: i32 = 6;
const FN_QLPC: i32 = 7;
const FN_ZERO: i32 = 8;
const FN_VERBATIM: i32 = 9;

// ─── Constants ───────────────────────────────────────────────────────────────
const FNSIZE: u32 = 2;
const ENERGYSIZE: u32 = 3;
const BITSHIFTSIZE: u32 = 2;
const LPCQSIZE: u32 = 2;
const LPCQUANT: i32 = 5;
const VERBATIM_CKSIZE_SIZE: u32 = 5;
const VERBATIM_BYTE_SIZE: u32 = 8;

/// Fixed-predictor coefficients for DIFF0 through DIFF3.
///
/// From TR-156: DIFF0 predicts 0 (no prediction), DIFF1 predicts sample[-1],
/// DIFF2 predicts 2*sample[-1] - sample[-2], DIFF3 predicts
/// 3*sample[-1] - 3*sample[-2] + sample[-3].
const FIXED_COEFFS: [[i32; 3]; 4] = [
    [0, 0, 0],   // DIFF0: prediction = 0
    [1, 0, 0],   // DIFF1: prediction = s[-1]
    [2, -1, 0],  // DIFF2: prediction = 2*s[-1] - s[-2]
    [3, -3, 1],  // DIFF3: prediction = 3*s[-1] - 3*s[-2] + s[-3]
];

/// State for decoding one Shorten stream.
pub struct Decoder<R: Read> {
    pub reader: BitReader<R>,
    pub channels: u32,
    pub blocksize: usize,
    pub maxnlpc: usize,
    pub nmean: usize,
    pub version: u8,
    pub bitshift: u32,

    /// Per-channel sample buffers.
    pub buffers: Vec<ChannelBuffer>,
    /// Per-channel DC offset accumulators.
    pub means: Vec<MeanAccumulator>,

    /// Which channel we're currently decoding.
    pub current_channel: u32,
    /// Whether the stream has ended (FN_QUIT encountered).
    pub finished: bool,
    /// Samples decoded in the current block, ready for output.
    pub output_buf: Vec<i32>,
    /// Read position within output_buf.
    pub output_pos: usize,
    /// The first audio command that was pre-read by the header parser.
    pending_cmd: Option<i32>,
}

impl<R: Read> Decoder<R> {
    pub fn new(reader: BitReader<R>, header: &ShnHeader) -> Self {
        let nchan = header.channels as usize;
        let mut buffers = Vec::with_capacity(nchan);
        let mut means = Vec::with_capacity(nchan);
        for _ in 0..nchan {
            buffers.push(ChannelBuffer::new(header.blocksize));
            means.push(MeanAccumulator::new(header.nmean));
        }

        Decoder {
            reader,
            channels: header.channels,
            blocksize: header.blocksize,
            maxnlpc: header.maxnlpc,
            nmean: header.nmean,
            version: header.version,
            bitshift: 0,
            buffers,
            means,
            current_channel: 0,
            finished: false,
            output_buf: Vec::new(),
            output_pos: 0,
            pending_cmd: header.first_audio_cmd,
        }
    }

    /// Decode the next block of samples. Returns `true` if a block was decoded,
    /// `false` if the stream has ended (FN_QUIT).
    ///
    /// After a successful call, interleaved samples are in `self.output_buf`.
    pub fn decode_next_block(&mut self) -> Result<bool, ShnError> {
        if self.finished {
            return Ok(false);
        }

        // We need to decode one block per channel, then interleave.
        // Shorten encodes channels in round-robin order: ch0 block, ch1 block, ch0 block, ...
        // For interleaved output, we collect one block from each channel.

        let nchan = self.channels as usize;
        let mut blocks_decoded = 0;

        while blocks_decoded < nchan {
            // Use the pre-read command from header parsing, if available
            let cmd = if let Some(c) = self.pending_cmd.take() {
                c
            } else {
                self.reader.read_unsigned_rice(FNSIZE)? as i32
            };

            match cmd {
                FN_QUIT => {
                    self.finished = true;
                    return Ok(false);
                }

                FN_BLOCKSIZE => {
                    let new_bs = self.reader.read_ulong()? as i32;
                    if new_bs <= 0 || new_bs > 65536 {
                        return Err(ShnError::InvalidBlockSize(new_bs));
                    }
                    self.blocksize = new_bs as usize;
                    for buf in &mut self.buffers {
                        buf.resize(self.blocksize);
                    }
                }

                FN_BITSHIFT => {
                    self.bitshift = self.reader.read_unsigned_rice(BITSHIFTSIZE)?;
                }

                FN_VERBATIM => {
                    let nbytes = self.reader.read_unsigned_rice(VERBATIM_CKSIZE_SIZE)? as usize;
                    for _ in 0..nbytes {
                        self.reader.read_unsigned_rice(VERBATIM_BYTE_SIZE)?;
                    }
                }

                FN_ZERO => {
                    let ch = self.current_channel as usize;
                    let bs = self.blocksize;
                    let buf = &mut self.buffers[ch];
                    buf.resize(bs);
                    for i in 0..bs {
                        buf.set(i as isize, 0);
                    }
                    self.finish_channel_block(ch)?;
                    blocks_decoded += 1;
                }

                FN_DIFF0 | FN_DIFF1 | FN_DIFF2 | FN_DIFF3 => {
                    let order = cmd as usize;
                    self.decode_fixed_prediction(order)?;
                    blocks_decoded += 1;
                }

                FN_QLPC => {
                    self.decode_qlpc()?;
                    blocks_decoded += 1;
                }

                _ => return Err(ShnError::InvalidCommand(cmd)),
            }
        }

        // Interleave the decoded blocks from all channels
        self.interleave_output();
        Ok(true)
    }

    /// Decode a block using fixed polynomial prediction (DIFF0-DIFF3).
    fn decode_fixed_prediction(&mut self, order: usize) -> Result<(), ShnError> {
        let ch = self.current_channel as usize;
        let energy = self.reader.read_unsigned_rice(ENERGYSIZE)?;
        let bs = self.blocksize;
        let buf = &mut self.buffers[ch];
        buf.resize(bs);

        // Compute DC offset from running mean
        let coffset = self.means[ch].coffset(bs);
        let coeffs = &FIXED_COEFFS[order];

        for i in 0..bs {
            let residual = self.reader.read_signed_rice(energy)?;
            let ii = i as isize;
            let prediction = if order == 0 {
                coffset
            } else {
                let mut pred = 0i32;
                for (j, &c) in coeffs.iter().enumerate().take(order) {
                    pred += c * buf.get(ii - j as isize - 1);
                }
                pred
            };
            buf.set(ii, residual + prediction);
        }

        self.finish_channel_block(ch)?;
        Ok(())
    }

    /// Decode a block using quantized LPC prediction.
    fn decode_qlpc(&mut self) -> Result<(), ShnError> {
        let ch = self.current_channel as usize;
        let energy = self.reader.read_unsigned_rice(ENERGYSIZE)?;
        let lpc_order = self.reader.read_unsigned_rice(LPCQSIZE)? as usize;

        if lpc_order > self.maxnlpc || lpc_order > 128 {
            return Err(ShnError::InvalidLpcOrder(lpc_order as i32));
        }

        // Read LPC coefficients
        let mut lpc_coeffs = Vec::with_capacity(lpc_order);
        for _ in 0..lpc_order {
            lpc_coeffs.push(self.reader.read_signed_rice(LPCQSIZE)?);
        }

        let bs = self.blocksize;
        let buf = &mut self.buffers[ch];
        buf.resize(bs);

        for i in 0..bs {
            let residual = self.reader.read_signed_rice(energy)?;
            let ii = i as isize;

            let mut prediction: i64 = 0;
            for (j, &coeff) in lpc_coeffs.iter().enumerate() {
                prediction += coeff as i64 * buf.get(ii - j as isize - 1) as i64;
            }
            let predicted = (prediction >> LPCQUANT) as i32;
            buf.set(ii, residual + predicted);
        }

        self.finish_channel_block(ch)?;
        Ok(())
    }

    /// Post-process a decoded channel block: apply bitshift, update mean, wrap around.
    fn finish_channel_block(&mut self, ch: usize) -> Result<(), ShnError> {
        let bs = self.blocksize;
        let buf = &mut self.buffers[ch];

        // Apply bitshift (if nonzero, samples were quantized during encoding)
        if self.bitshift > 0 {
            for i in 0..bs {
                let v = buf.get(i as isize);
                buf.set(i as isize, v << self.bitshift);
            }
        }

        // Update the running mean with this block's mean (sum / blocksize, rounded)
        if self.nmean > 0 {
            let block_sum: i64 = (0..bs).map(|i| buf.get(i as isize) as i64).sum();
            // For bitshifted data, compute the un-shifted sum first
            let effective_sum = if self.bitshift > 0 {
                block_sum >> self.bitshift
            } else {
                block_sum
            };
            // Store the per-block mean (sum / blocksize), rounded with bias
            let bs_i64 = bs as i64;
            let block_mean = ((effective_sum + bs_i64 / 2) / bs_i64) as i32;
            self.means[ch].push(block_mean);
        }

        // Copy last NWRAP samples to history region for next block's predictors
        buf.wrap_around();

        // Advance to next channel (round-robin)
        self.current_channel = (self.current_channel + 1) % self.channels;

        Ok(())
    }

    /// Interleave decoded blocks from all channels into output_buf.
    fn interleave_output(&mut self) {
        let nchan = self.channels as usize;
        let bs = self.blocksize;

        self.output_buf.clear();
        self.output_buf.reserve(nchan * bs);
        self.output_pos = 0;

        if nchan == 1 {
            // Mono: just copy directly
            self.output_buf
                .extend_from_slice(self.buffers[0].block_samples());
        } else {
            // Interleave: sample0_ch0, sample0_ch1, sample1_ch0, sample1_ch1, ...
            for i in 0..bs {
                for ch in 0..nchan {
                    self.output_buf.push(self.buffers[ch].block_samples()[i]);
                }
            }
        }
    }

    /// Get the next sample from the output buffer, or None if exhausted.
    pub fn next_sample(&mut self) -> Option<i32> {
        if self.output_pos < self.output_buf.len() {
            let s = self.output_buf[self.output_pos];
            self.output_pos += 1;
            Some(s)
        } else {
            None
        }
    }
}
