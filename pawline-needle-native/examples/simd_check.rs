fn main() {
    println!("NEEDLE_SIMD={:?} has_avx2={}", std::env::var("NEEDLE_SIMD").ok(), needle_core::quant::has_avx2());
}
