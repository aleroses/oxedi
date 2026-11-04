//! Throughput of the tokenizer and the document index pass over the three
//! largest fixtures.

mod common;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxedi_core::{Document, Tokenizer};
use std::hint::black_box;

const FIXTURES: &[&str] = &[
    "emedny_sample.txt",
    "united_healthcare_legacy_sample.txt",
    "multi_claim_sample.txt",
];

fn load(name: &str) -> Vec<u8> {
    common::load_from("tests/fixtures", name)
}

fn tokenize_fixtures(c: &mut Criterion) {
    let mut group = c.benchmark_group("tokenize");
    for name in FIXTURES {
        let bytes = load(name);
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bytes, |b, bytes| {
            b.iter(|| {
                Tokenizer::new(black_box(bytes))
                    .expect("fixture has an ISA")
                    .count()
            });
        });
    }
    group.finish();
}

fn index_fixtures(c: &mut Criterion) {
    let mut group = c.benchmark_group("index");
    for name in FIXTURES {
        let bytes = load(name);
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bytes, |b, bytes| {
            b.iter(|| {
                Document::parse(black_box(&bytes[..]))
                    .expect("fixture has an ISA")
                    .len()
            });
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = common::profiled();
    targets = tokenize_fixtures, index_fixtures
}
criterion_main!(benches);
