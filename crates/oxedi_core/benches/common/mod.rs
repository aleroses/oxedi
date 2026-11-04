//! Loaders shared by the bench files; each file uses only some of them.
#![allow(dead_code)]

mod profiler;
pub use profiler::profiled;

/// The three largest anonymized samples, the engine's workload.
pub const SAMPLES: &[&str] = &[
    "edi835_test_united.rmt",
    "edi835_test_versant.RMT",
    "edi835_test_eyemed.RMT",
];

pub fn load_from(dir: &str, name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(dir)
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()))
}
