//! Per-core CPU utilisation for the per-core widget.
//!
//! Like processes, not a metric source: the output is one value per core,
//! which the static metric catalogue cannot express. Sampled on the poller
//! thread, and only while a widget asks for it.

use sysinfo::System;

pub struct CoreSampler {
    system: System,
    /// sysinfo derives usage from two refreshes; the first only primes it.
    primed: bool,
}

impl CoreSampler {
    #[must_use]
    pub fn new() -> Self {
        Self { system: System::new(), primed: false }
    }

    /// Usage per core, 0-100, in CPU order. Empty on the first call: a usage
    /// figure needs two refreshes, and a fabricated zero would be a guess.
    pub fn sample(&mut self) -> Vec<f32> {
        self.system.refresh_cpu_usage();
        if !self.primed {
            self.primed = true;
            return Vec::new();
        }
        self.system.cpus().iter().map(|c| c.cpu_usage()).collect()
    }
}

impl Default for CoreSampler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_sample_primes_and_later_ones_report_every_core() {
        let mut sampler = CoreSampler::new();
        assert!(sampler.sample().is_empty(), "no usage without two refreshes");
        let usage = sampler.sample();
        assert!(!usage.is_empty(), "this machine has cores");
        assert!(usage.iter().all(|u| (0.0..=100.0).contains(u)), "{usage:?}");
    }
}
