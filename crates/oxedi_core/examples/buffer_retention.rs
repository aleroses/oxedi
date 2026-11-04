//! Measures two ways for a document to own its bytes: `Document<'static>`
//! (a `Cow::Owned` buffer plus spans) and a shared `Arc<[u8]>` buffer plus
//! the same spans. For each, over the largest sample: building from an owned
//! `Vec<u8>`, iterating every segment, cloning, and the heap held by N
//! documents built from N inputs and by N clones of one document.
//!
//! Run with `cargo run --release -p oxedi_core --example buffer_retention`.

use std::alloc::{GlobalAlloc, Layout, System};
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use oxedi_core::{Delimiters, Document, Frame, Segment, Span, next_frame, trivia::is_trivia};

/// The system allocator, counting the bytes currently allocated.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

// SAFETY: every call is forwarded to `System` unchanged; the counter is
// only bookkeeping.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: same contract as the caller's.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The alternative: the bytes behind an `Arc`, the spans as they are.
#[derive(Clone)]
struct SharedDocument {
    bytes: Arc<[u8]>,
    delims: Delimiters,
    spans: Vec<Span>,
}

impl SharedDocument {
    fn parse(bytes: Vec<u8>) -> SharedDocument {
        let bytes: Arc<[u8]> = Arc::from(bytes);
        let start = bytes.iter().position(|&b| !is_trivia(b)).unwrap_or(0);
        let delims = Delimiters::from_isa(&bytes[start..]).expect("sample has an ISA");
        let mut spans = Vec::new();
        let (mut rest, mut offset) = (&bytes[..], 0);
        while let Some((frame, next)) = next_frame(rest, &delims) {
            let trivia = frame.raw.len() - frame.body.len() - usize::from(frame.terminated);
            let raw = offset..offset + frame.raw.len();
            let body = raw.start + trivia..raw.start + trivia + frame.body.len();
            spans.push(Span {
                raw: raw.clone(),
                body,
                terminated: frame.terminated,
            });
            offset = raw.end;
            rest = next;
        }
        SharedDocument {
            bytes,
            delims,
            spans,
        }
    }

    fn segments(&self) -> impl Iterator<Item = Segment<'_>> {
        self.spans.iter().enumerate().map(|(index, span)| {
            let frame = Frame {
                raw: &self.bytes[span.raw.clone()],
                body: &self.bytes[span.body.clone()],
                terminated: span.terminated,
            };
            Segment::parse(index, frame, &self.delims)
        })
    }
}

/// Median of `runs` timings of `work`.
fn median(runs: usize, mut work: impl FnMut()) -> Duration {
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            work();
            start.elapsed()
        })
        .collect();
    times.sort();
    times[runs / 2]
}

/// Heap bytes still held after `build` returns its value.
fn retained<T>(build: impl FnOnce() -> T) -> (T, usize) {
    let before = LIVE.load(Ordering::Relaxed);
    let value = build();
    (value, LIVE.load(Ordering::Relaxed).saturating_sub(before))
}

fn mib(bytes: usize) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn main() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/samples/edi835_test_united.rmt"
    );
    let input = std::fs::read(path).expect("the sample is readable");
    let runs = 51;
    println!(
        "input: {} bytes, {} segments",
        input.len(),
        Document::parse(&input[..])
            .expect("sample has an ISA")
            .len()
    );

    let cow_build = median(runs, || {
        black_box(Document::parse(black_box(input.clone())).expect("sample has an ISA"));
    });
    let arc_build = median(runs, || {
        black_box(SharedDocument::parse(black_box(input.clone())));
    });
    let copy_only = median(runs, || {
        black_box(black_box(&input).clone());
    });
    println!(
        "build from Vec<u8> (minus the input clone {copy_only:?}): cow {:?}, arc {:?}",
        cow_build.saturating_sub(copy_only),
        arc_build.saturating_sub(copy_only)
    );

    let cow = Document::parse(input.clone()).expect("sample has an ISA");
    let arc = SharedDocument::parse(input.clone());
    let cow_iter = median(runs, || {
        black_box(cow.segments().map(|s| s.elements.len()).sum::<usize>());
    });
    let arc_iter = median(runs, || {
        black_box(arc.segments().map(|s| s.elements.len()).sum::<usize>());
    });
    println!("iterate every segment: cow {cow_iter:?}, arc {arc_iter:?}");

    let cow_clone = median(runs, || {
        black_box(black_box(&cow).clone());
    });
    let arc_clone = median(runs, || {
        black_box(black_box(&arc).clone());
    });
    println!("clone: cow {cow_clone:?}, arc {arc_clone:?}");

    for n in [1, 10, 100] {
        let (cows, cow_held) = retained(|| {
            (0..n)
                .map(|_| Document::parse(input.clone()).expect("sample has an ISA"))
                .collect::<Vec<_>>()
        });
        drop(cows);
        let (arcs, arc_held) = retained(|| {
            (0..n)
                .map(|_| SharedDocument::parse(input.clone()))
                .collect::<Vec<_>>()
        });
        drop(arcs);
        let (cow_clones, cow_clone_held) =
            retained(|| (0..n).map(|_| cow.clone()).collect::<Vec<_>>());
        drop(cow_clones);
        let (arc_clones, arc_clone_held) =
            retained(|| (0..n).map(|_| arc.clone()).collect::<Vec<_>>());
        drop(arc_clones);
        println!(
            "N={n:>3}: {n} inputs held: cow {}, arc {}; {n} clones of one: cow {}, arc {}",
            mib(cow_held),
            mib(arc_held),
            mib(cow_clone_held),
            mib(arc_clone_held)
        );
    }
}
