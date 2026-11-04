//! Sampling profiler behind criterion's `--profile-time`. A normal bench run never starts it;
//! with `--profile-time N` each benchmark runs for N seconds under a sampler and leaves
//! `flamegraph.svg` and `top.txt` in its profile directory.

use criterion::Criterion;

/// The criterion configuration every bench group uses: the default, plus the sampling profiler
/// on Unix.
pub fn profiled() -> Criterion {
    #[cfg(unix)]
    {
        Criterion::default().with_profiler(unix::SamplingProfiler::default())
    }
    #[cfg(not(unix))]
    {
        Criterion::default()
    }
}

#[cfg(unix)]
mod unix {
    use criterion::profiler::Profiler;
    use pprof::{ProfilerGuard, ProfilerGuardBuilder, Report};
    use std::collections::HashMap;
    use std::fs::File;
    use std::path::Path;

    /// Samples per second of CPU time.
    const FREQUENCY: i32 = 997;
    /// Functions listed in `top.txt`.
    const TOP: usize = 25;

    #[derive(Default)]
    pub struct SamplingProfiler {
        guard: Option<ProfilerGuard<'static>>,
    }

    impl Profiler for SamplingProfiler {
        fn start_profiling(&mut self, benchmark_id: &str, _benchmark_dir: &Path) {
            // The runtime's own frames hide the caller's; they are left out of the samples.
            let started = ProfilerGuardBuilder::default()
                .frequency(FREQUENCY)
                .blocklist(&["libc", "libgcc", "pthread", "vdso"])
                .build();
            match started {
                Ok(guard) => self.guard = Some(guard),
                Err(e) => eprintln!("profiler: cannot start sampling `{benchmark_id}`: {e}"),
            }
        }

        fn stop_profiling(&mut self, benchmark_id: &str, benchmark_dir: &Path) {
            let Some(guard) = self.guard.take() else {
                return;
            };
            let report = match guard.report().build() {
                Ok(report) => report,
                Err(e) => {
                    eprintln!("profiler: cannot build the report of `{benchmark_id}`: {e}");
                    return;
                }
            };
            if let Err(e) = std::fs::create_dir_all(benchmark_dir) {
                eprintln!(
                    "profiler: cannot create {} for `{benchmark_id}`: {e}",
                    benchmark_dir.display()
                );
                return;
            }
            let svg = benchmark_dir.join("flamegraph.svg");
            let written = File::create(&svg)
                .map_err(|e| e.to_string())
                .and_then(|file| report.flamegraph(file).map_err(|e| e.to_string()));
            if let Err(e) = written {
                eprintln!(
                    "profiler: cannot write {} for `{benchmark_id}`: {e}",
                    svg.display()
                );
            }
            let top = benchmark_dir.join("top.txt");
            if let Err(e) = std::fs::write(&top, top_self(&report)) {
                eprintln!(
                    "profiler: cannot write {} for `{benchmark_id}`: {e}",
                    top.display()
                );
            }
        }
    }

    /// The functions with the most samples at the top of the stack, one per line:
    /// samples, share of all samples, name.
    fn top_self(report: &Report) -> String {
        let mut by_name: HashMap<String, isize> = HashMap::new();
        let mut total: isize = 0;
        for (frames, count) in &report.data {
            total += count;
            let leaf = frames
                .frames
                .first()
                .and_then(|inlined| inlined.first())
                .map_or_else(|| "[unknown]".to_string(), |symbol| symbol.name());
            *by_name.entry(leaf).or_default() += count;
        }
        let mut ranked: Vec<_> = by_name.into_iter().collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        let mut text = format!(
            "{:>8} {:>6}  function (self samples of {total})\n",
            "samples", "share"
        );
        for (name, count) in ranked.into_iter().take(TOP) {
            let share = if total > 0 {
                100.0 * count as f64 / total as f64
            } else {
                0.0
            };
            text.push_str(&format!("{count:>8} {share:>5.1}%  {name}\n"));
        }
        text
    }
}
