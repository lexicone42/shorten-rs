use std::io::Read;

use crate::error::ShnError;

/// MSB-first bit reader over any `Read` source.
///
/// Shorten uses MSB-first bit packing: the first bit read from a byte is the
/// most significant bit. Bits are consumed from a 32-bit accumulator that is
/// refilled from the underlying reader one byte at a time.
pub struct BitReader<R: Read> {
    reader: R,
    /// Bit accumulator — bits are left-justified (MSB = next bit to read).
    buf: u32,
    /// Number of valid bits remaining in `buf`.
    bits_left: u32,
}

impl<R: Read> BitReader<R> {
    pub fn new(reader: R) -> Self {
        BitReader {
            reader,
            buf: 0,
            bits_left: 0,
        }
    }

    /// Read a single byte directly from the underlying stream (bypasses bit buffer).
    /// Used for reading the magic and version before bitstream mode begins.
    pub fn read_byte_direct(&mut self) -> Result<u8, ShnError> {
        let mut b = [0u8; 1];
        self.reader.read_exact(&mut b)?;
        Ok(b[0])
    }

    /// Ensure at least `n` bits are available in the accumulator.
    fn fill(&mut self, n: u32) -> Result<(), ShnError> {
        while self.bits_left < n {
            let mut b = [0u8; 1];
            self.reader.read_exact(&mut b)?;
            self.buf |= (b[0] as u32) << (24 - self.bits_left);
            self.bits_left += 8;
        }
        Ok(())
    }

    /// Read `n` bits (MSB-first) and return them as a u32. Max 25 bits per call
    /// to stay safe with the 32-bit accumulator and single-byte refill.
    pub fn read_bits(&mut self, n: u32) -> Result<u32, ShnError> {
        debug_assert!(n <= 25, "read_bits limited to 25 bits per call");
        if n == 0 {
            return Ok(0);
        }
        self.fill(n)?;
        let val = self.buf >> (32 - n);
        self.buf <<= n;
        self.bits_left -= n;
        Ok(val)
    }

    /// Read an unsigned Rice-coded value with parameter `k` (uvar_get).
    ///
    /// Zeros-before-one (ZBO) encoding:
    /// - Count leading 0-bits until a 1-bit (stop bit) is found → quotient `q`
    /// - Read `k` mantissa bits → remainder `r`
    /// - Value = (q << k) | r
    pub fn read_unsigned_rice(&mut self, k: u32) -> Result<u32, ShnError> {
        // Count leading 0-bits (unary quotient)
        let mut q = 0u32;
        loop {
            self.fill(1)?;
            let bit = self.buf >> 31;
            self.buf <<= 1;
            self.bits_left -= 1;
            if bit == 0 {
                q += 1;
            } else {
                break; // stop bit (1)
            }
        }
        // Read k mantissa bits (remainder)
        let r = if k > 0 { self.read_bits(k)? } else { 0 };
        Ok((q << k) | r)
    }

    /// Read a signed Rice-coded value (var_get).
    ///
    /// The signed encoding uses k+1 mantissa bits in the underlying unsigned
    /// Rice code (the sign-folding approximately doubles the magnitude).
    ///
    /// Sign-folding: unsigned → signed mapping:
    /// 0→0, 1→-1, 2→1, 3→-2, 4→2, ... (even=positive, odd=negative)
    pub fn read_signed_rice(&mut self, k: u32) -> Result<i32, ShnError> {
        let u = self.read_unsigned_rice(k + 1)?;
        let signed = if u & 1 == 0 {
            (u >> 1) as i32
        } else {
            -((u >> 1) as i32) - 1
        };
        Ok(signed)
    }

    /// Read a "ulong" — Shorten's variable-length unsigned integer.
    ///
    /// Two-level Rice encoding:
    /// 1. Read nbits = uvar_get(ULONGSIZE=2) — how many mantissa bits the value needs
    /// 2. Read value = uvar_get(nbits) — the actual value, Rice-coded with nbits mantissa
    pub fn read_ulong(&mut self) -> Result<u32, ShnError> {
        let nbits = self.read_unsigned_rice(2)?; // ULONGSIZE = 2
        let value = self.read_unsigned_rice(nbits)?;
        Ok(value)
    }

    /// Read `n` bytes directly from the underlying stream.
    /// This should only be called when the bit buffer is byte-aligned
    /// (after reading the header), or we drain the bit buffer first.
    pub fn read_bytes(&mut self, n: usize) -> Result<Vec<u8>, ShnError> {
        let mut buf = vec![0u8; n];
        // First drain any complete bytes from the bit buffer
        let mut i = 0;
        while i < n && self.bits_left >= 8 {
            buf[i] = (self.buf >> 24) as u8;
            self.buf <<= 8;
            self.bits_left -= 8;
            i += 1;
        }
        // Read the rest directly
        if i < n {
            self.reader.read_exact(&mut buf[i..])?;
        }
        Ok(buf)
    }

    /// Get a reference to the underlying reader.
    pub fn inner(&self) -> &R {
        &self.reader
    }

    /// Consume the BitReader and return the underlying reader.
    pub fn into_inner(self) -> R {
        self.reader
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_bits_basic() {
        // 0xA5 = 1010_0101, 0x3C = 0011_1100
        let data: &[u8] = &[0xA5, 0x3C];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_bits(4).unwrap(), 0b1010);
        assert_eq!(br.read_bits(4).unwrap(), 0b0101);
        assert_eq!(br.read_bits(8).unwrap(), 0x3C);
    }

    #[test]
    fn read_bits_across_boundary() {
        let data: &[u8] = &[0xFF, 0x00];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_bits(5).unwrap(), 0b11111);
        assert_eq!(br.read_bits(6).unwrap(), 0b111000);
    }

    #[test]
    fn unsigned_rice_k0() {
        // ZBO k=0: pure unary. Value N = N zeros + stop(1).
        // Value 0: 1 (just stop bit)
        // Value 3: 0001 (three zeros + stop)
        // Packed: 1_0001_000 = 0x88
        let data: &[u8] = &[0x88];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_unsigned_rice(0).unwrap(), 0);
        assert_eq!(br.read_unsigned_rice(0).unwrap(), 3);
    }

    #[test]
    fn unsigned_rice_k2() {
        // ZBO k=2: q zeros + stop(1) + 2 mantissa bits
        // Value 5 = (1 << 2) | 1 → q=1, r=01 → 0_1_01 = 0101
        // Value 2 = (0 << 2) | 2 → q=0, r=10 → 1_10 = 110
        // Packed: 0101_110_0 = 0b01011100 = 0x5C
        let data: &[u8] = &[0x5C];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_unsigned_rice(2).unwrap(), 5);
        assert_eq!(br.read_unsigned_rice(2).unwrap(), 2);
    }

    #[test]
    fn signed_rice() {
        // Signed rice(k) reads unsigned rice(k+1) then sign-unfolds.
        // k=0: reads unsigned rice(1) values.
        // ZBO rice(1): stop(1) + 1 mantissa bit
        // Unsigned 0 → signed 0:  code = 1 0 (val=0: q=0, r=0)
        // Unsigned 1 → signed -1: code = 1 1 (val=1: q=0, r=1)
        // Unsigned 2 → signed 1:  code = 0 1 0 (val=2: q=1, r=0)
        // Packed: 10_11_010_0 = 0b10110100 = 0xB4
        let data: &[u8] = &[0xB4];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_signed_rice(0).unwrap(), 0);
        assert_eq!(br.read_signed_rice(0).unwrap(), -1);
        assert_eq!(br.read_signed_rice(0).unwrap(), 1);
    }

    #[test]
    fn ulong_encoding() {
        // ulong: read_unsigned_rice(2) → nbits, then read_unsigned_rice(nbits) → value
        // For value 5: nbits=3 (5 needs 3 bits)
        //   Step 1: uvar(3, k=2): q=0, r=11 → 1 11 (3 bits)
        //   Step 2: uvar(5, k=3): q=0, r=101 → 1 101 (4 bits)
        // Total: 111 1101 = 0b1111101_0 = 0xFA
        let data: &[u8] = &[0xFA];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_ulong().unwrap(), 5);
    }

    #[test]
    fn ulong_zero() {
        // ulong(0): nbits=0.
        //   Step 1: uvar(0, k=2): q=0, r=00 → 1 00 (3 bits)
        //   Step 2: uvar(0, k=0): q=0, r=nothing → 1 (1 bit)
        // Total: 100 1 = 0b1001_0000 = 0x90
        let data: &[u8] = &[0x90];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_ulong().unwrap(), 0);
    }

    #[test]
    fn decode_real_header() {
        // Real SHN header bytes (after magic+version) for type=5, ch=2, bs=256:
        // FB B1 70 09 F9 (and more for remaining fields)
        // Full header: type=5, channels=2, blocksize=256, maxnlpc=0, nmean=4, nskip=0
        let data: &[u8] = &[0xFB, 0xB1, 0x70, 0x09, 0xF9, 0x20];
        let mut br = BitReader::new(data);
        assert_eq!(br.read_ulong().unwrap(), 5);   // filetype
        assert_eq!(br.read_ulong().unwrap(), 2);   // channels
        assert_eq!(br.read_ulong().unwrap(), 256); // blocksize
        assert_eq!(br.read_ulong().unwrap(), 0);   // maxnlpc
        assert_eq!(br.read_ulong().unwrap(), 4);   // nmean
        assert_eq!(br.read_ulong().unwrap(), 0);   // nskip
    }
}
