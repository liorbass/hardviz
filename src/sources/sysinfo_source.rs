//! CPU, memory and network readings via the `sysinfo` crate.
//!
//! Cross-platform. Provides utilisation and capacity; it does not read CPU
//! temperature (Windows uses `sources/lhm.rs`, Linux `sources/hwmon.rs`, macOS
//! `sources/macos/sensors.rs`), so it does not claim the metric and another
//! source fills the gap.

use crate::metrics::{metric, MetricId, BYTES_PER_MB};
use crate::num::{count_f64, total_f64};
use crate::source::{MetricSource, Readings, SourceStatus};
use std::time::Instant;
use sysinfo::{Networks, System};

const PROVIDES: &[MetricId] = &[
    metric::CPU_USAGE,
    metric::RAM_USAGE,
    metric::RAM_USED,
    metric::RAM_TOTAL,
    metric::SWAP_USAGE,
    metric::SWAP_USED,
    // Registered after LHM, so on Windows LHM's live average wins; sysinfo's
    // Windows value is the nominal clock and serves only as a fallback.
    metric::CPU_CLOCK,
    // Only written where the OS reports it (not Windows); declaring it
    // everywhere keeps the source's contract static.
    metric::CPU_LOAD,
    metric::NET_DOWN,
    metric::NET_UP,
    metric::UPTIME,
];

/// Below this a "clock" is a misread, not a reading: sysinfo derives Apple
/// Silicon clocks from a static pmgr table, which comes out as 4 MHz on an M4.
const MIN_PLAUSIBLE_MHZ: f64 = 100.0;

/// Re-scan the adapter list this often (in polls) so a VPN or hot-plugged NIC
/// that appears later is counted.
const NET_RESCAN_POLLS: u32 = 30;

/// Adapters that would double-count traffic already seen on a physical NIC
/// (loopback, VM/container bridges, WSL's Hyper-V switch).
fn is_virtual_adapter(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "lo"
        || ["loopback", "vethernet", "vmware", "virtualbox", "hyper-v", "docker", "veth", "br-", "virbr", "vboxnet"]
            .iter()
            .any(|k| n.contains(k))
}

pub struct SysinfoSource {
    system: System,
    /// CPU clocks come from a separate `System`: on Windows (sysinfo 0.32),
    /// refreshing frequency on the instance that measures usage corrupts the
    /// usage reading, which then sits at or near 100% regardless of load.
    clocks: System,
    networks: Networks,
    net_polls: u32,
    last_net: Instant,
    /// sysinfo derives CPU utilisation from two samples, so the first poll
    /// only primes it and reports no usage.
    cpu_primed: bool,
    status: SourceStatus,
}

impl SysinfoSource {
    #[must_use]
    pub fn new() -> Self {
        // `new()`, not `new_all()`: we expose only CPU and memory, so there is no
        // reason to enumerate processes, disks, networks and components.
        Self {
            system: System::new(),
            clocks: System::new(),
            networks: Networks::new(),
            net_polls: 0,
            last_net: Instant::now(),
            cpu_primed: false,
            status: SourceStatus::not_yet_polled(),
        }
    }
}

impl Default for SysinfoSource {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricSource for SysinfoSource {
    fn id(&self) -> &'static str {
        "sysinfo"
    }

    fn display_name(&self) -> &'static str {
        "sysinfo (CPU/RAM)"
    }

    fn provides(&self) -> &'static [MetricId] {
        PROVIDES
    }

    fn poll(&mut self, out: &mut Readings) {
        // Refresh only what this source exposes. `refresh_all()` would also
        // enumerate every process, disk, network interface and component (about
        // 1 s per poll on a busy machine), stalling every other metric.
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.clocks.refresh_cpu_frequency();

        if self.cpu_primed {
            out.set(metric::CPU_USAGE, f64::from(self.system.global_cpu_usage()));
        }
        self.cpu_primed = true;

        let freqs: Vec<f64> =
            self.clocks.cpus().iter().map(|c| total_f64(c.frequency())).filter(|f| *f >= MIN_PLAUSIBLE_MHZ).collect();
        if !freqs.is_empty() {
            out.set(metric::CPU_CLOCK, freqs.iter().sum::<f64>() / count_f64(freqs.len()));
        }

        // sysinfo reports memory in bytes as of 0.32.
        let (total, used) = (self.system.total_memory(), self.system.used_memory());
        out.set(metric::RAM_TOTAL, total_f64(total) / BYTES_PER_MB);
        out.set(metric::RAM_USED, total_f64(used) / BYTES_PER_MB);
        if total > 0 {
            out.set(metric::RAM_USAGE, total_f64(used) / total_f64(total) * 100.0);
        }

        // No swap configured is "not available", not a zero reading.
        let (swap_total, swap_used) = (self.system.total_swap(), self.system.used_swap());
        if swap_total > 0 {
            out.set(metric::SWAP_USED, total_f64(swap_used) / BYTES_PER_MB);
            out.set(metric::SWAP_USAGE, total_f64(swap_used) / total_f64(swap_total) * 100.0);
        }

        // sysinfo returns zeros for the load average on Windows (the OS does
        // not report one); leave the metric unset there rather than fake it.
        #[cfg(not(windows))]
        out.set(metric::CPU_LOAD, System::load_average().one);

        out.set(metric::UPTIME, total_f64(System::uptime()));

        if let Some((down, up)) = self.poll_network() {
            out.set(metric::NET_DOWN, down);
            out.set(metric::NET_UP, up);
        }

        self.status = SourceStatus::Online;
    }

    fn status(&self) -> SourceStatus {
        self.status.clone()
    }
}

impl SysinfoSource {
    /// Bytes moved since the last refresh, converted to MB/s over the real
    /// elapsed time (the poll interval is configurable and can jitter).
    /// Returns (down, up) in MB/s, or `None` when this poll has no valid delta.
    fn poll_network(&mut self) -> Option<(f64, f64)> {
        self.net_polls += 1;
        // The adapter list is built on the first poll and rescanned periodically
        // so adapters that appear later (VPN, USB tethering) are counted.
        if self.net_polls == 1 || self.net_polls.is_multiple_of(NET_RESCAN_POLLS) {
            self.networks.refresh_list();
        } else {
            self.networks.refresh();
        }
        let elapsed = self.last_net.elapsed().as_secs_f64();
        self.last_net = Instant::now();

        let (mut rx, mut tx) = (0u64, 0u64);
        for (name, data) in &self.networks {
            if is_virtual_adapter(name) {
                continue;
            }
            rx += data.received();
            tx += data.transmitted();
        }

        // The first refresh after a (re)scan reports totals since boot, not a
        // delta; skip it rather than plot a huge spike.
        if self.net_polls == 1 || self.net_polls.is_multiple_of(NET_RESCAN_POLLS) || elapsed <= 0.0 {
            return None;
        }
        Some((total_f64(rx) / BYTES_PER_MB / elapsed, total_f64(tx) / BYTES_PER_MB / elapsed))
    }
}

#[cfg(test)]
mod tests {
    use super::is_virtual_adapter;

    #[test]
    fn filters_virtual_adapters() {
        for n in [
            "lo",
            "vEthernet (WSL (Hyper-V firewall))",
            "VMware Network Adapter VMnet8",
            "docker0",
            "veth12ab",
            "virbr0",
        ] {
            assert!(is_virtual_adapter(n), "{n}");
        }
        for n in ["Ethernet 2", "Wi-Fi", "eth0", "wlp3s0", "enp5s0", "Tailscale"] {
            assert!(!is_virtual_adapter(n), "{n}");
        }
    }
}
