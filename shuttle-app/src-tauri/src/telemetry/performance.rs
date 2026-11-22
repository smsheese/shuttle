use parking_lot::Mutex;
use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

const RING_CAPACITY: usize = 120;
const TIMING_CAPACITY: usize = 200;

/// Operations whose latency is reported. Anything else is ignored.
pub const TIMED_OPERATIONS: &[&str] = &[
    "chat_list",
    "chat_open",
    "search_chats",
    "search_messages",
];

#[derive(Clone, Copy, Debug)]
struct Sample {
    cpu_percent: f32,
    memory_mb: f32,
    sidecar_cpu_percent: f32,
    sidecar_memory_mb: f32,
    sidecar_count: u32,
}

#[derive(Debug, Clone)]
pub struct OperationTiming {
    pub operation: &'static str,
    pub count: u64,
    pub p50_ms: u64,
    pub p95_ms: u64,
    pub max_ms: u64,
}

pub struct PerformanceSampler {
    foreground: Mutex<bool>,
    samples: Mutex<VecDeque<(Instant, Sample)>>,
    last_emit: Mutex<Instant>,
    system: Mutex<System>,
    timings: Mutex<HashMap<&'static str, Vec<u32>>>,
}

impl PerformanceSampler {
    pub fn new() -> Self {
        Self {
            foreground: Mutex::new(true),
            samples: Mutex::new(VecDeque::new()),
            last_emit: Mutex::new(Instant::now()),
            system: Mutex::new(System::new()),
            timings: Mutex::new(HashMap::new()),
        }
    }

    pub fn record_timing(&self, operation: &str, elapsed: Duration) {
        let Some(op) = TIMED_OPERATIONS.iter().find(|o| **o == operation) else {
            return;
        };
        let mut timings = self.timings.lock();
        let list = timings.entry(op).or_default();
        if list.len() >= TIMING_CAPACITY {
            list.remove(0);
        }
        list.push(elapsed.as_millis().min(u32::MAX as u128) as u32);
    }

    pub fn drain_timings(&self) -> Vec<OperationTiming> {
        let mut timings = self.timings.lock();
        let mut out = Vec::new();
        for (operation, mut values) in timings.drain() {
            if values.is_empty() {
                continue;
            }
            values.sort_unstable();
            let at = |p: f32| values[((values.len() as f32 - 1.0) * p).round() as usize] as u64;
            out.push(OperationTiming {
                operation,
                count: values.len() as u64,
                p50_ms: at(0.5),
                p95_ms: at(0.95),
                max_ms: *values.last().unwrap_or(&0) as u64,
            });
        }
        out
    }

    pub fn set_foreground(&self, foreground: bool) {
        *self.foreground.lock() = foreground;
        self.samples.lock().clear();
        *self.last_emit.lock() = Instant::now();
    }

    pub fn is_foreground(&self) -> bool {
        *self.foreground.lock()
    }

    pub fn sample_interval(&self) -> Duration {
        if *self.foreground.lock() {
            Duration::from_secs(60)
        } else {
            Duration::from_secs(180)
        }
    }

    pub fn emit_interval(&self) -> Duration {
        if *self.foreground.lock() {
            Duration::from_secs(15 * 60)
        } else {
            Duration::from_secs(30 * 60)
        }
    }

    pub fn record_sample(&self) {
        let sample = self.measure();
        let mut ring = self.samples.lock();
        ring.push_back((Instant::now(), sample));
        while ring.len() > RING_CAPACITY {
            ring.pop_front();
        }
    }

    fn measure(&self) -> Sample {
        let own = Pid::from_u32(std::process::id());
        let mut system = self.system.lock();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_cpu().with_memory(),
        );
        let mut family: HashSet<Pid> = HashSet::from([own]);
        loop {
            let before = family.len();
            for (pid, process) in system.processes() {
                if process.parent().is_some_and(|p| family.contains(&p)) {
                    family.insert(*pid);
                }
            }
            if family.len() == before {
                break;
            }
        }
        let mb = |bytes: u64| bytes as f32 / 1_048_576.0;
        let mut sample = Sample {
            cpu_percent: 0.0,
            memory_mb: 0.0,
            sidecar_cpu_percent: 0.0,
            sidecar_memory_mb: 0.0,
            sidecar_count: 0,
        };
        for pid in family {
            let Some(process) = system.process(pid) else { continue };
            if pid == own {
                sample.cpu_percent = process.cpu_usage();
                sample.memory_mb = mb(process.memory());
            } else {
                sample.sidecar_cpu_percent += process.cpu_usage();
                sample.sidecar_memory_mb += mb(process.memory());
                sample.sidecar_count += 1;
            }
        }
        sample
    }

    pub fn maybe_snapshot(&self) -> Option<PerformanceSnapshot> {
        let elapsed = self.last_emit.lock().elapsed();
        if elapsed < self.emit_interval() {
            return None;
        }
        *self.last_emit.lock() = Instant::now();
        let ring = self.samples.lock();
        if ring.is_empty() {
            return None;
        }
        let series = |f: fn(&Sample) -> f32| {
            let mut v: Vec<f32> = ring.iter().map(|(_, s)| f(s)).collect();
            v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            v
        };
        let cpu = series(|s| s.cpu_percent);
        let mem = series(|s| s.memory_mb);
        let side_cpu = series(|s| s.sidecar_cpu_percent);
        let side_mem = series(|s| s.sidecar_memory_mb);
        Some(PerformanceSnapshot {
            foreground: self.is_foreground(),
            sample_count: ring.len() as u64,
            cpu_avg: avg(&cpu),
            cpu_p95: percentile(&cpu, 0.95),
            memory_avg_mb: avg(&mem),
            memory_p95_mb: percentile(&mem, 0.95),
            sidecar_cpu_avg: avg(&side_cpu),
            sidecar_memory_avg_mb: avg(&side_mem),
            sidecar_memory_p95_mb: percentile(&side_mem, 0.95),
            sidecar_count: ring.iter().map(|(_, s)| s.sidecar_count).max().unwrap_or(0),
        })
    }
}

#[derive(Debug, Clone)]
pub struct PerformanceSnapshot {
    pub foreground: bool,
    pub sample_count: u64,
    pub cpu_avg: f32,
    pub cpu_p95: f32,
    pub memory_avg_mb: f32,
    pub memory_p95_mb: f32,
    pub sidecar_cpu_avg: f32,
    pub sidecar_memory_avg_mb: f32,
    pub sidecar_memory_p95_mb: f32,
    pub sidecar_count: u32,
}

fn avg(values: &[f32]) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f32>() / values.len() as f32
}

fn percentile(values: &[f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let idx = ((values.len() as f32 - 1.0) * p).round() as usize;
    values[idx.min(values.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_interval_is_180s() {
        let sampler = PerformanceSampler::new();
        sampler.set_foreground(false);
        assert_eq!(sampler.sample_interval(), Duration::from_secs(180));
        assert_eq!(sampler.emit_interval(), Duration::from_secs(30 * 60));
    }

    #[test]
    fn measures_real_memory_and_ignores_unlisted_operations() {
        let sampler = PerformanceSampler::new();
        sampler.record_sample();
        let first = sampler.samples.lock().back().copied().unwrap().1;
        assert!(first.memory_mb > 1.0, "own RSS must be measured on every OS");
        sampler.record_timing("chat_open", Duration::from_millis(40));
        sampler.record_timing("chat_open", Duration::from_millis(900));
        sampler.record_timing("whatever_the_user_typed", Duration::from_millis(5));
        let timings = sampler.drain_timings();
        assert_eq!(timings.len(), 1);
        assert_eq!(timings[0].operation, "chat_open");
        assert_eq!(timings[0].max_ms, 900);
        assert!(sampler.drain_timings().is_empty());
    }

    #[test]
    fn foreground_interval_is_60s() {
        let sampler = PerformanceSampler::new();
        sampler.set_foreground(true);
        assert_eq!(sampler.sample_interval(), Duration::from_secs(60));
        assert_eq!(sampler.emit_interval(), Duration::from_secs(15 * 60));
    }
}
