//! Throughput of the writer, in bytes written, over the tables of the
//! largest sample: the whole write (plan, nesting, emission and the read
//! back that checks the file).

mod common;

use common::load_from;
use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use oxedi_core::write::{Envelope, write};
use oxedi_core::{Document, Processor, Spec};
use std::hint::black_box;

fn write_united(c: &mut Criterion) {
    let spec = Spec::builtin_835();
    let bytes = load_from("tests/samples", "edi835_test_united.rmt");
    let document = Document::parse(&bytes[..]).expect("sample has an ISA");
    let (tables, _) = Processor::run(&spec, &document, None);
    let envelope = Envelope::new("ZZ", "SENDER", "ZZ", "RECEIVER", 19_723, 45_000);
    let written = write(&spec, &tables, &envelope).expect("the sample writes back");
    let mut group = c.benchmark_group("write");
    group.throughput(Throughput::Bytes(written.len() as u64));
    group.bench_function("edi835_test_united.rmt", |b| {
        b.iter(|| write(&spec, black_box(&tables), &envelope));
    });
    group.finish();
}

criterion_group! {
    name = benches;
    config = common::profiled();
    targets = write_united
}
criterion_main!(benches);
