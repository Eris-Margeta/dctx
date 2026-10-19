//! One-shot SHA-256 hex for configuration body fingerprints.
//! Implements FIPS 180-4 §5 and §6.2.2 for spans this crate already accepted.
//! Depends on no other crate. `domain::body_fingerprint` is the only caller.
//! The digest matches `dctx_hash::Sha256`. Messages here are far below the length cap.

const IV: [u32; 8] = [
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
];

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// Lowercase hex SHA-256 of `data`.
pub(crate) fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha::new();
    hasher.absorb(data);
    hasher.hex()
}

/// SHA-256 of `body` with `split_whitespace` words joined by one space.
pub(crate) fn sha256_hex_whitespace(body: &str) -> String {
    let mut hasher = Sha::new();
    let mut separate = false;
    for word in body.split_whitespace() {
        if separate {
            hasher.absorb_byte(b' ');
        }
        hasher.absorb(word.as_bytes());
        separate = true;
    }
    hasher.hex()
}

struct Sha {
    state: [u32; 8],
    block: [u8; 64],
    filled: usize,
    total: u64,
}

impl Sha {
    fn new() -> Self {
        Self { state: IV, block: [0; 64], filled: 0, total: 0 }
    }

    fn absorb(&mut self, mut data: &[u8]) {
        let Some(total) = self.total.checked_add(data.len() as u64) else {
            return;
        };
        self.total = total;
        if self.filled > 0 {
            let take = (64 - self.filled).min(data.len());
            self.block[self.filled..self.filled + take].copy_from_slice(&data[..take]);
            self.filled += take;
            data = &data[take..];
            if self.filled == 64 {
                compress(&mut self.state, &self.block);
                self.filled = 0;
            }
        }
        while data.len() >= 64 {
            let mut block = [0u8; 64];
            block.copy_from_slice(&data[..64]);
            compress(&mut self.state, &block);
            data = &data[64..];
        }
        if !data.is_empty() {
            self.block[..data.len()].copy_from_slice(data);
            self.filled = data.len();
        }
    }

    fn absorb_byte(&mut self, byte: u8) {
        let Some(total) = self.total.checked_add(1) else {
            return;
        };
        self.total = total;
        self.block[self.filled] = byte;
        self.filled += 1;
        if self.filled == 64 {
            compress(&mut self.state, &self.block);
            self.filled = 0;
        }
    }

    fn hex(mut self) -> String {
        let bits = self.total.wrapping_mul(8);
        self.block[self.filled] = 0x80;
        let padded = self.filled + 1;
        if padded > 56 {
            self.block[padded..].fill(0);
            compress(&mut self.state, &self.block);
            self.block = [0; 64];
        } else {
            self.block[padded..56].fill(0);
        }
        self.block[56..].copy_from_slice(&bits.to_be_bytes());
        compress(&mut self.state, &self.block);
        hex_state(&self.state)
    }
}

macro_rules! round {
    ($a:ident, $b:ident, $c:ident, $d:ident, $e:ident, $f:ident, $g:ident, $h:ident, $word:expr, $k:expr) => {{
        let t1 = $h
            .wrapping_add($e.rotate_right(6) ^ $e.rotate_right(11) ^ $e.rotate_right(25))
            .wrapping_add(($e & $f) ^ (!$e & $g))
            .wrapping_add($k)
            .wrapping_add($word);
        let t2 = ($a.rotate_right(2) ^ $a.rotate_right(13) ^ $a.rotate_right(22))
            .wrapping_add(($a & $b) ^ ($a & $c) ^ ($b & $c));
        $d = $d.wrapping_add(t1);
        $h = t1.wrapping_add(t2);
    }};
}

fn compress(state: &mut [u32; 8], block: &[u8; 64]) {
    let mut w = [0u32; 16];
    for index in 0..16 {
        let start = index * 4;
        w[index] = u32::from_be_bytes([block[start], block[start + 1], block[start + 2], block[start + 3]]);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    // Sixteen rounds, then the next schedule block. Names rotate so the adds stay in registers.
    let mut schedule = 0usize;
    while schedule < 64 {
        if schedule >= 16 {
            for step in 0..16 {
                let index = (schedule + step) & 15;
                let s0 = w[(index + 1) & 15].rotate_right(7)
                    ^ w[(index + 1) & 15].rotate_right(18)
                    ^ (w[(index + 1) & 15] >> 3);
                let s1 = w[(index + 14) & 15].rotate_right(17)
                    ^ w[(index + 14) & 15].rotate_right(19)
                    ^ (w[(index + 14) & 15] >> 10);
                w[index] = w[index]
                    .wrapping_add(s0)
                    .wrapping_add(w[(index + 9) & 15])
                    .wrapping_add(s1);
            }
        }
        let base = schedule;
        round!(a, b, c, d, e, f, g, h, w[(base) & 15], K[base]);
        round!(h, a, b, c, d, e, f, g, w[(base + 1) & 15], K[base + 1]);
        round!(g, h, a, b, c, d, e, f, w[(base + 2) & 15], K[base + 2]);
        round!(f, g, h, a, b, c, d, e, w[(base + 3) & 15], K[base + 3]);
        round!(e, f, g, h, a, b, c, d, w[(base + 4) & 15], K[base + 4]);
        round!(d, e, f, g, h, a, b, c, w[(base + 5) & 15], K[base + 5]);
        round!(c, d, e, f, g, h, a, b, w[(base + 6) & 15], K[base + 6]);
        round!(b, c, d, e, f, g, h, a, w[(base + 7) & 15], K[base + 7]);
        round!(a, b, c, d, e, f, g, h, w[(base + 8) & 15], K[base + 8]);
        round!(h, a, b, c, d, e, f, g, w[(base + 9) & 15], K[base + 9]);
        round!(g, h, a, b, c, d, e, f, w[(base + 10) & 15], K[base + 10]);
        round!(f, g, h, a, b, c, d, e, w[(base + 11) & 15], K[base + 11]);
        round!(e, f, g, h, a, b, c, d, w[(base + 12) & 15], K[base + 12]);
        round!(d, e, f, g, h, a, b, c, w[(base + 13) & 15], K[base + 13]);
        round!(c, d, e, f, g, h, a, b, w[(base + 14) & 15], K[base + 14]);
        round!(b, c, d, e, f, g, h, a, w[(base + 15) & 15], K[base + 15]);
        schedule += 16;
    }
    for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *slot = slot.wrapping_add(value);
    }
}

fn hex_state(state: &[u32; 8]) -> String {
    let mut hex = String::with_capacity(64);
    for word in state {
        for shift in [24, 16, 8, 0] {
            let byte = (word >> shift) as u8;
            hex.push(char::from(HEX[usize::from(byte >> 4)]));
            hex.push(char::from(HEX[usize::from(byte & 15)]));
        }
    }
    hex
}

const HEX: &[u8; 16] = b"0123456789abcdef";

#[cfg(test)]
mod tests {
    use super::*;
    use dctx_hash::Sha256;

    fn reference(data: &[u8]) -> String {
        let digest = Sha256::digest(data).expect("length");
        digest.as_bytes().iter().map(|byte| format!("{byte:02x}")).collect()
    }

    #[test]
    fn digests_match_dctx_hash_across_block_edges() {
        let samples: [&[u8]; 8] = [
            b"",
            b"a",
            b"abc",
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            &[0u8; 55],
            &[0u8; 56],
            &[0u8; 63],
            &[0u8; 64],
        ];
        for sample in samples {
            assert_eq!(sha256_hex(sample), reference(sample), "len {}", sample.len());
        }
        let long = vec![b'a'; 1000];
        assert_eq!(sha256_hex(&long), reference(&long));
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let spaced = " a\n\tb  c ";
        let normalized = spaced.split_whitespace().collect::<Vec<_>>().join(" ");
        assert_eq!(sha256_hex_whitespace(spaced), reference(normalized.as_bytes()));
        assert_eq!(sha256_hex_whitespace(""), reference(b""));
    }
}
