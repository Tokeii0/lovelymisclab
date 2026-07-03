//! CPython-compatible Mersenne Twister (the `random` module): seed derivation
//! (int, or str/bytes via SHA-512), and the primitives behind `random.randint`,
//! `random.random`, `random.shuffle` (Python 3) and the Python 2 shuffle. Verified
//! byte-exact against CPython output. Used by the blind-watermark extractors that
//! must reproduce a password/seed-based permutation or XOR mask.
#![allow(dead_code)] // shared RNG util — not every helper is used by every node yet

use num_bigint::BigUint;
use sha2::{Digest, Sha512};

pub struct Mt {
    mt: [u32; 624],
    mti: usize,
}

impl Mt {
    pub fn new() -> Self {
        Mt {
            mt: [0; 624],
            mti: 625,
        }
    }

    fn init_genrand(&mut self, s: u32) {
        self.mt[0] = s;
        for i in 1..624 {
            let p = self.mt[i - 1] ^ (self.mt[i - 1] >> 30);
            self.mt[i] = 1812433253u32.wrapping_mul(p).wrapping_add(i as u32);
        }
        self.mti = 624;
    }

    fn init_by_array(&mut self, key: &[u32]) {
        self.init_genrand(19650218);
        let (mut i, mut j) = (1usize, 0usize);
        let mut k = 624.max(key.len());
        while k > 0 {
            let p = self.mt[i - 1] ^ (self.mt[i - 1] >> 30);
            self.mt[i] = (self.mt[i] ^ p.wrapping_mul(1664525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i >= 624 {
                self.mt[0] = self.mt[623];
                i = 1;
            }
            if j >= key.len() {
                j = 0;
            }
            k -= 1;
        }
        k = 623;
        while k > 0 {
            let p = self.mt[i - 1] ^ (self.mt[i - 1] >> 30);
            self.mt[i] = (self.mt[i] ^ p.wrapping_mul(1566083941)).wrapping_sub(i as u32);
            i += 1;
            if i >= 624 {
                self.mt[0] = self.mt[623];
                i = 1;
            }
            k -= 1;
        }
        self.mt[0] = 0x8000_0000;
    }

    pub fn seed_biguint(&mut self, n: &BigUint) {
        let mut key = n.to_u32_digits(); // little-endian words
        if key.is_empty() {
            key.push(0);
        }
        self.init_by_array(&key);
    }

    fn genrand(&mut self) -> u32 {
        if self.mti >= 624 {
            const MAG: [u32; 2] = [0, 0x9908_b0df];
            let m = &mut self.mt;
            for kk in 0..227 {
                let y = (m[kk] & 0x8000_0000) | (m[kk + 1] & 0x7fff_ffff);
                m[kk] = m[kk + 397] ^ (y >> 1) ^ MAG[(y & 1) as usize];
            }
            for kk in 227..623 {
                let y = (m[kk] & 0x8000_0000) | (m[kk + 1] & 0x7fff_ffff);
                m[kk] = m[kk - 227] ^ (y >> 1) ^ MAG[(y & 1) as usize];
            }
            let y = (m[623] & 0x8000_0000) | (m[0] & 0x7fff_ffff);
            m[623] = m[396] ^ (y >> 1) ^ MAG[(y & 1) as usize];
            self.mti = 0;
        }
        let mut y = self.mt[self.mti];
        self.mti += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    pub fn getrandbits(&mut self, k: u32) -> u32 {
        self.genrand() >> (32 - k)
    }

    /// `random._randbelow(n)` for n > 0.
    pub fn randbelow(&mut self, n: u32) -> u32 {
        if n == 0 {
            return 0;
        }
        let bits = 32 - n.leading_zeros(); // n.bit_length()
        let mut r = self.getrandbits(bits);
        while r >= n {
            r = self.getrandbits(bits);
        }
        r
    }

    /// `random.randint(0, 255)`.
    pub fn randint_byte(&mut self) -> u8 {
        self.randbelow(256) as u8
    }

    /// `random.random()` — 53-bit float in [0, 1).
    pub fn random_f64(&mut self) -> f64 {
        let a = (self.genrand() >> 5) as f64; // 27 bits
        let b = (self.genrand() >> 6) as f64; // 26 bits
        (a * 67108864.0 + b) * (1.0 / 9007199254740992.0)
    }

    /// `random.shuffle(x)` (Python 3): Fisher-Yates using `_randbelow`.
    pub fn shuffle<T>(&mut self, x: &mut [T]) {
        for i in (1..x.len()).rev() {
            let j = self.randbelow((i + 1) as u32) as usize;
            x.swap(i, j);
        }
    }

    /// Python 2 shuffle: `j = int(random() * (i + 1))`.
    pub fn old_shuffle<T>(&mut self, x: &mut [T]) {
        for i in (1..x.len()).rev() {
            let j = (self.random_f64() * (i as f64 + 1.0)) as usize;
            x.swap(i, j.min(i));
        }
    }

    /// numpy `RandomState.shuffle` — Fisher-Yates with `rk_interval` (uses the LOW
    /// bits of the raw MT output with a power-of-two mask + rejection to [0, i]).
    pub fn numpy_shuffle<T>(&mut self, x: &mut [T]) {
        for i in (1..x.len()).rev() {
            let mut mask = i as u32;
            mask |= mask >> 1;
            mask |= mask >> 2;
            mask |= mask >> 4;
            mask |= mask >> 8;
            mask |= mask >> 16;
            let j = loop {
                let v = self.genrand() & mask;
                if v <= i as u32 {
                    break v as usize;
                }
            };
            x.swap(i, j);
        }
    }
}

/// Seed like numpy `RandomState(seed)` for a uint32 seed: `init_genrand` (NOT the
/// `init_by_array` that CPython's `random.seed(int)` uses). `.random_f64()` then
/// reproduces `RandomState.random()`.
pub fn mt_numpy(seed: u32) -> Mt {
    let mut m = Mt::new();
    m.init_genrand(seed);
    m
}

/// Seed from an integer written as a decimal string (CPython `random.seed(int)`).
pub fn mt_from_int(s: &str) -> Option<Mt> {
    let n: BigUint = s.trim().parse().ok()?;
    let mut mt = Mt::new();
    mt.seed_biguint(&n);
    Some(mt)
}

/// Seed from a u64 (CPython `random.seed(int)`).
pub fn mt_from_u64(n: u64) -> Mt {
    let mut mt = Mt::new();
    mt.seed_biguint(&BigUint::from(n));
    mt
}

/// Seed from a string (CPython `random.seed(str)` → SHA-512 derivation).
pub fn mt_from_str(s: &str) -> Mt {
    let enc = s.as_bytes();
    let mut buf = enc.to_vec();
    buf.extend_from_slice(&Sha512::digest(enc));
    let n = BigUint::from_bytes_be(&buf);
    let mut mt = Mt::new();
    mt.seed_biguint(&n);
    mt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn randint_matches_cpython() {
        let mut m = mt_from_int("114514").unwrap();
        let seq: Vec<u8> = (0..8).map(|_| m.randint_byte()).collect();
        assert_eq!(seq, [119, 53, 105, 81, 147, 139, 69, 228]);
        let mut m2 = mt_from_str("secret");
        let seq2: Vec<u8> = (0..8).map(|_| m2.randint_byte()).collect();
        assert_eq!(seq2, [3, 66, 32, 224, 181, 132, 109, 12]);
    }

    #[test]
    fn numpy_random_and_shuffle() {
        // np.random.RandomState(1).random(5)[0] == 0.417022004702574
        let mut m = mt_numpy(1);
        assert!((m.random_f64() - 0.417022004702574).abs() < 1e-12);
        // np.random.RandomState(1).shuffle(arange(79)) == [63, 27, 31, 69, 46, ...]
        let mut m2 = mt_numpy(1);
        let mut v: Vec<u32> = (0..79).collect();
        m2.numpy_shuffle(&mut v);
        assert_eq!(&v[..5], &[63, 27, 31, 69, 46]);
    }

    #[test]
    fn shuffle_matches_cpython() {
        // random.seed(12345); random.shuffle(list(range(300))) == [158,177,265,69,114,...]
        let mut m = mt_from_u64(12345);
        let mut v: Vec<u32> = (0..300).collect();
        m.shuffle(&mut v);
        assert_eq!(&v[..10], &[158, 177, 265, 69, 114, 220, 185, 11, 139, 29]);
        // continuing the same RNG state: shuffle(range(960))
        let mut n: Vec<u32> = (0..960).collect();
        m.shuffle(&mut n);
        assert_eq!(&n[..10], &[371, 31, 959, 813, 233, 874, 527, 10, 807, 919]);
    }
}
