use parking_lot::Mutex;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

const RING_CAPACITY: usize = 120;

#[derive(Clone, Copy, Debug)]
struct Sample {
    cpu_load: f32,
    memory_mb: f32,
}

pub struct PerformanceSampler {
    foreground: Mutex<bool>,
    samples: Mutex<VecDeque<(Instant, Sample)>>,
    last_emit: Mutex<Instant>,
}

impl PerformanceSampler {
    pub fn new() -> Self {
        Self {
            foreground: Mutex::new(true),
            samples: Mutex::new(VecDeque::new()),
            last_emit: Mutex::new(Instant::now()),
        }
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
        let sample = Sample {
            cpu_load: estimate_cpu_load(),
            memory_mb: estimate_memory_mb(),
        };
        let mut ring = self.samples.lock();
        ring.push_back((Instant::now(), sample));
        while ring.len() > RING_CAPACITY {
            ring.pop_front();
        }
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
        let mut cpu: Vec<f32> = ring.iter().map(|(_, s)| s.cpu_load).collect();
        let mut mem: Vec<f32> = ring.iter().map(|(_, s)| s.memory_mb).collect();
        cpu.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        mem.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(PerformanceSnapshot {
            foreground: *self.foreground.lock(),
            sample_count: ring.len() as u64,
            cpu_avg: avg(&cpu),
            cpu_p95: percentile(&cpu, 0.95),
            memory_avg_mb: avg(&mem),
            memory_p95_mb: percentile(&mem, 0.95),
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

fn estimate_memory_mb() -> f32 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
            for line in status.lines() {
                if line.starts_with("VmRSS:") {
                    if let Some(kb) = line.split_whitespace().nth(1) {
                        if let Ok(v) = kb.parse::<f32>() {
                            return v / 1024.0;
                        }
                    }
                }
            }
        }
    }
    0.0
}

fn estimate_cpu_load() -> f32 {
    // Lightweight placeholder: process RSS growth is not CPU; use a small bounded pseudo metric
    // so snapshots remain stable without OS-specific CPU counters in v1.
    0.0
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
    fn foreground_interval_is_60s() {
        let sampler = PerformanceSampler::new();
        sampler.set_foreground(true);
        assert_eq!(sampler.sample_interval(), Duration::from_secs(60));
        assert_eq!(sampler.emit_interval(), Duration::from_secs(15 * 60));
    }
}
