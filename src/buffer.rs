/// Number of history samples maintained before the current block.
///
/// The highest-order fixed predictor (DIFF3) uses coefficients [3, -3, 1],
/// requiring the 3 most recent samples. NWRAP=3 provides exactly this.
pub const NWRAP: usize = 3;

/// Per-channel sample buffer with wrap-around history.
///
/// The buffer stores `NWRAP` history samples followed by the current block.
/// Indexing is relative to the start of the current block, so index -1
/// refers to the last sample of the previous block (history region).
pub struct ChannelBuffer {
    /// Sample storage: [history (NWRAP)] [current block (blocksize)]
    data: Vec<i32>,
    /// Current block size (may change mid-stream via FN_BLOCKSIZE).
    blocksize: usize,
}

impl ChannelBuffer {
    /// Create a new channel buffer for the given block size.
    pub fn new(blocksize: usize) -> Self {
        let data = vec![0i32; NWRAP + blocksize];
        ChannelBuffer { data, blocksize }
    }

    /// Resize the buffer for a new block size, preserving history.
    pub fn resize(&mut self, new_blocksize: usize) {
        if new_blocksize != self.blocksize {
            self.blocksize = new_blocksize;
            self.data.resize(NWRAP + new_blocksize, 0);
        }
    }

    /// Get sample at position `i` relative to the current block start.
    /// Negative indices access the history region.
    #[inline]
    pub fn get(&self, i: isize) -> i32 {
        self.data[(NWRAP as isize + i) as usize]
    }

    /// Set sample at position `i` relative to the current block start.
    #[inline]
    pub fn set(&mut self, i: isize, val: i32) {
        self.data[(NWRAP as isize + i) as usize] = val;
    }

    /// After decoding a block, copy the last NWRAP samples to the history region
    /// so the next block's predictors can reference them via negative indices.
    pub fn wrap_around(&mut self) {
        let bs = self.blocksize;
        for i in 0..NWRAP {
            self.data[i] = self.data[bs + i];
        }
    }

    /// Get the current block size.
    pub fn blocksize(&self) -> usize {
        self.blocksize
    }

    /// Iterate over the decoded samples in the current block (not including history).
    pub fn block_samples(&self) -> &[i32] {
        &self.data[NWRAP..NWRAP + self.blocksize]
    }
}

/// Rolling mean tracker for computing the DC offset (coffset).
///
/// Shorten uses a running mean of recent sample blocks to center the residuals.
/// The mean window size is `nmean` (typically 4 for v2+).
pub struct MeanAccumulator {
    /// Circular buffer of block means.
    values: Vec<i32>,
    /// Write index into the circular buffer.
    index: usize,
}

impl MeanAccumulator {
    pub fn new(nmean: usize) -> Self {
        MeanAccumulator {
            values: vec![0i32; nmean],
            index: 0,
        }
    }

    /// Compute the current offset from the running mean.
    ///
    /// The offset array stores per-block means (block_sum / blocksize, rounded).
    /// coffset is the mean of these per-block means, also rounded.
    /// Uses all nmean slots (including initial zeros), matching the reference.
    pub fn coffset(&self, _blocksize: usize) -> i32 {
        let nmean = self.values.len();
        if nmean == 0 {
            return 0;
        }
        let sum: i64 = self.values.iter().map(|&v| v as i64).sum();
        let nmean_i64 = nmean as i64;
        ((sum + nmean_i64 / 2) / nmean_i64) as i32
    }

    /// Push a new block sum into the rolling window.
    pub fn push(&mut self, block_sum: i32) {
        let cap = self.values.len();
        if cap == 0 {
            return;
        }
        self.values[self.index % cap] = block_sum;
        self.index += 1;
    }

    /// Number of mean slots (nmean).
    pub fn capacity(&self) -> usize {
        self.values.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_buffer_basic() {
        let mut buf = ChannelBuffer::new(4);
        // History should be zero
        assert_eq!(buf.get(-1), 0);
        assert_eq!(buf.get(-2), 0);
        assert_eq!(buf.get(-3), 0);
        // Set some samples
        buf.set(0, 10);
        buf.set(1, 20);
        buf.set(2, 30);
        buf.set(3, 40);
        assert_eq!(buf.get(0), 10);
        assert_eq!(buf.get(3), 40);
        // Wrap around
        buf.wrap_around();
        // History should now be samples [1,2,3] = [20,30,40]
        assert_eq!(buf.get(-3), 20);
        assert_eq!(buf.get(-2), 30);
        assert_eq!(buf.get(-1), 40);
    }

    #[test]
    fn mean_accumulator() {
        let mut acc = MeanAccumulator::new(4);
        assert_eq!(acc.coffset(256), 0); // No data yet
        // Now we push per-block MEANS (not raw sums)
        // Push a block mean of 10
        acc.push(10);
        // coffset = (10 + 0 + 0 + 0 + 2) / 4 = 12 / 4 = 3 (diluted by unfilled slots)
        assert_eq!(acc.coffset(256), 3);
        // Fill all 4 slots with mean=10
        acc.push(10);
        acc.push(10);
        acc.push(10);
        // coffset = (40 + 2) / 4 = 42 / 4 = 10
        assert_eq!(acc.coffset(256), 10);
    }
}
