//! Throughput of the loop engine with the structure checker over the three
//! largest samples.

mod common;

use common::{SAMPLES, load_from};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxedi_core::{Delimiters, LoopEngine, Spec, StructureChecker, Tokenizer};
use std::hint::black_box;

/// Runs the engine and the structure checker over every segment and returns
/// how many diagnostics the checker raised.
fn run_check(spec: &Spec, bytes: &[u8]) -> usize {
    let mut engine = LoopEngine::new(spec);
    let delimiters = Delimiters::from_isa(bytes).expect("sample has an ISA");
    let mut checker = StructureChecker::new(spec, &delimiters);
    let mut diagnostics = 0usize;
    for segment in Tokenizer::new(bytes).expect("sample has an ISA") {
        let events = engine.feed(&segment);
        diagnostics += checker.on(&segment, events).len();
    }
    engine.finish();
    diagnostics + checker.finish().len()
}

fn check_samples(c: &mut Criterion) {
    let spec = Spec::builtin_835();
    let mut group = c.benchmark_group("check");
    for name in SAMPLES {
        let bytes = load_from("tests/samples", name);
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bytes, |b, bytes| {
            b.iter(|| run_check(&spec, black_box(bytes)));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = common::profiled();
    targets = check_samples
}
criterion_main!(benches);
