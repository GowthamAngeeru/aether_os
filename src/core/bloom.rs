use std::sync::atomic::{AtomicU64, Ordering};

const FNV_OFFSET_BASIS: u64 = 14695981039346656037;
const FNV_PRIME: u64 = 1099511628211;

#[inline]
fn fnv1a(data: &[u8], seed: u64) -> u64 {
    let mut hash = FNV_OFFSET_BASIS ^ seed;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

pub struct BloomFilter {
    bits: Vec<AtomicU64>,
    m_bits: usize,
    k_hashes: u32,
}

impl BloomFilter {
    pub fn new(expected_elements: usize, false_positive_rate: f64) -> Self {
        assert!(
            expected_elements > 0,
            "BloomFilter: expected_elements must be > 0"
        );
        assert!(
            false_positive_rate > 0.0 && false_positive_rate < 1.0,
            "BloomFilter: false_positive_rate must be between 0.0 and 1.0"
        );

        let n = expected_elements as f64;
        let p = false_positive_rate;
        let m_exact = -(n * p.ln()) / (2.0f64.ln().powi(2));

        let m_bits_raw = m_exact.ceil() as usize;
        let m_bits = m_bits_raw.next_multiple_of(64);

        let k = (m_bits as f64 / n) * 2.0f64.ln();
        let k_hashes = (k.ceil() as u32).max(1);

        let words = m_bits / 64;
        let bits = (0..words).map(|_| AtomicU64::new(0)).collect();

        Self {
            bits,
            m_bits,
            k_hashes,
        }
    }

    #[inline]
    fn set_bit(&self, index: usize) {
        let word = index / 64;
        let shift = index % 64;
        self.bits[word].fetch_or(1u64 << shift, Ordering::Relaxed);
    }

    fn get_bit(&self, index: usize) -> bool {
        let word = index / 64;
        let shift = index % 64;
        (self.bits[word].load(Ordering::Relaxed) >> shift) & 1 == 1
    }

    #[inline]
    fn hash_indexes<'a>(&'a self, key: &str) -> impl Iterator<Item = usize> + 'a {
        let bytes = key.as_bytes();
        let h1 = fnv1a(bytes, 0);
        let h2 = fnv1a(bytes, FNV_OFFSET_BASIS);

        (0..self.k_hashes).map(move |i| {
            let combined = h1.wrapping_add((i as u64).wrapping_mul(h2));
            (combined as usize) % self.m_bits
        })
    }

    pub fn insert(&self, key: &str) {
        for idx in self.hash_indexes(key) {
            self.set_bit(idx);
        }
    }

    pub fn contains(&self, key: &str) -> bool {
        self.hash_indexes(key).all(|idx| self.get_bit(idx))
    }

    pub fn estimated_fp_rate(&self) -> f64 {
        // FIX 4: Sum into a u64 first, then cast to f64 later
        let set_bits: u64 = self
            .bits
            .iter()
            .map(|word| word.load(Ordering::Relaxed).count_ones() as u64)
            .sum();

        let fill_ratio = set_bits as f64 / self.m_bits as f64;
        fill_ratio.powi(self.k_hashes as i32)
    }

    pub fn memory_bytes(&self) -> usize {
        self.bits.len() * std::mem::size_of::<AtomicU64>()
    }
}

impl std::fmt::Debug for BloomFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BloomFilter")
            .field("m_bits", &self.m_bits)
            .field("k_hashes", &self.k_hashes)
            .field("memory_kb", &(self.memory_bytes() / 1024))
            .field("estimated_fp_rate", &self.estimated_fp_rate())
            .finish()
    }
}
