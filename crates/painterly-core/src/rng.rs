//! シード固定で再現可能な乱数（numpy.random.default_rng(seed) の役割）。
//! プラットフォーム間で結果が一致するよう ChaCha8 を使う。

use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub struct Rng64(ChaCha8Rng);

impl Rng64 {
    pub fn seed_from(seed: u64) -> Self {
        Rng64(ChaCha8Rng::seed_from_u64(seed))
    }
    /// [0, 1) の一様乱数
    pub fn random(&mut self) -> f32 {
        self.0.gen::<f32>()
    }
    /// [lo, hi) の一様乱数
    pub fn uniform(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.0.gen::<f32>()
    }
    pub fn gen_range(&mut self, range: std::ops::Range<usize>) -> usize {
        self.0.gen_range(range)
    }
    pub fn gen_range_f64(&mut self, range: std::ops::Range<f64>) -> f64 {
        self.0.gen_range(range)
    }
}
