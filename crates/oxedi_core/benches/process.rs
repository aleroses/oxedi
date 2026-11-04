//! Throughput of the whole processor (engine, checker, projector) over the
//! three largest samples in bytes and in table rows.

mod common;

use common::{SAMPLES, load_from};
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use oxedi_core::{Document, Processor, Spec};
use std::hint::black_box;

/// Indexes the bytes, runs the processor over the document and returns how
/// many table rows it produced.
fn run_process(spec: &Spec, bytes: &[u8]) -> usize {
    let document = Document::parse(bytes).expect("sample has an ISA");
    let (tables, _) = Processor::run(spec, &document, None);
    tables.iter().map(|table| table.len()).sum()
}

fn process_samples(c: &mut Criterion) {
    let spec = Spec::builtin_835();
    let mut group = c.benchmark_group("process");
    for name in SAMPLES {
        let bytes = load_from("tests/samples", name);
        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bytes, |b, bytes| {
            b.iter(|| run_process(&spec, black_box(bytes)));
        });
    }
    group.finish();
}

fn process_rows_samples(c: &mut Criterion) {
    let spec = Spec::builtin_835();
    let mut group = c.benchmark_group("process_rows");
    for name in SAMPLES {
        let bytes = load_from("tests/samples", name);
        let rows = run_process(&spec, &bytes);
        group.throughput(Throughput::Elements(rows as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), &bytes, |b, bytes| {
            b.iter(|| run_process(&spec, black_box(bytes)));
        });
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = common::profiled();
    targets = process_samples, process_rows_samples
}
criterion_main!(benches);
