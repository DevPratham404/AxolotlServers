//! Conversion of Docker's raw `ContainerStatsResponse` into the compact,
//! browser-facing [`StatsSample`]. Pure functions so the arithmetic is testable
//! without a Docker daemon.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use bollard::models::{
    ContainerCpuStats, ContainerMemoryStats, ContainerNetworkStats, ContainerStatsResponse,
};
use cluster_shared::StatsSample;

/// Docker reports cumulative CPU nanoseconds. Derive the percentage from the
/// delta against the previous sample:
///
/// `(cpu_delta / system_delta) * online_cpus * 100`
pub fn cpu_percent(
    previous_total: u64,
    previous_system: u64,
    total: u64,
    system: u64,
    online_cpus: u32,
) -> f64 {
    if system <= previous_system || total < previous_total {
        return 0.0;
    }

    let cpu_delta = total - previous_total;
    let system_delta = system - previous_system;
    let cpus = online_cpus.max(1) as f64;

    ((cpu_delta as f64 / system_delta as f64) * cpus * 100.0).clamp(0.0, cpus * 100.0)
}

/// Usage includes page cache, which is reclaimable and would make an idle
/// container look full. Subtract the cgroup's inactive file cache when present.
pub fn working_set(usage: u64, limit: u64, stats: Option<&HashMap<String, u64>>) -> u64 {
    let reclaimable = stats
        .and_then(|values| {
            values
                .get("total_inactive_file") // cgroup v1
                .or_else(|| values.get("inactive_file")) // cgroup v2
                .or_else(|| values.get("cache")) // older engines
        })
        .copied()
        .unwrap_or(0);

    let working = usage.saturating_sub(reclaimable);
    if limit == 0 {
        working
    } else {
        working.min(limit)
    }
}

pub fn memory_percent(used: u64, limit: u64) -> f64 {
    if limit == 0 {
        return 0.0;
    }
    ((used as f64 / limit as f64) * 100.0).clamp(0.0, 100.0)
}

pub fn network_totals(networks: Option<&HashMap<String, ContainerNetworkStats>>) -> (u64, u64) {
    let Some(networks) = networks else {
        return (0, 0);
    };

    networks.values().fold((0, 0), |(rx, tx), interface| {
        (
            rx.saturating_add(interface.rx_bytes.unwrap_or(0)),
            tx.saturating_add(interface.tx_bytes.unwrap_or(0)),
        )
    })
}

fn online_cpus(cpu: Option<&ContainerCpuStats>) -> u32 {
    cpu.and_then(|stats| {
        stats.online_cpus.or_else(|| {
            stats
                .cpu_usage
                .as_ref()
                .and_then(|usage| usage.percpu_usage.as_ref())
                .map(|cores| cores.len() as u32)
        })
    })
    .filter(|count| *count > 0)
    .unwrap_or(1)
}

fn total_cpu(cpu: Option<&ContainerCpuStats>) -> (u64, u64) {
    match cpu {
        Some(stats) => (
            stats
                .cpu_usage
                .as_ref()
                .and_then(|usage| usage.total_usage)
                .unwrap_or(0),
            stats.system_cpu_usage.unwrap_or(0),
        ),
        None => (0, 0),
    }
}

/// Build a sample from a Docker stats frame; `None` when the frame has no CPU
/// or memory data at all (Docker occasionally emits an empty first frame).
pub fn from_docker(stats: &ContainerStatsResponse) -> Option<StatsSample> {
    let memory: &ContainerMemoryStats = stats.memory_stats.as_ref()?;
    let limit = memory.limit.unwrap_or(0);
    let used = working_set(memory.usage.unwrap_or(0), limit, memory.stats.as_ref());

    let current = stats.cpu_stats.as_ref()?;
    let previous = stats.precpu_stats.as_ref();
    let (previous_total, previous_system) = total_cpu(previous);
    let (total, system) = total_cpu(Some(current));

    let (rx, tx) = network_totals(stats.networks.as_ref());

    Some(StatsSample {
        cpu_percent: cpu_percent(
            previous_total,
            previous_system,
            total,
            system,
            online_cpus(Some(current)),
        ),
        memory_used_bytes: used,
        memory_limit_bytes: limit,
        memory_percent: memory_percent(used, limit),
        network_rx_bytes: rx,
        network_tx_bytes: tx,
        pids: stats
            .pids_stats
            .as_ref()
            .and_then(|pids| pids.current)
            .unwrap_or(0),
        timestamp_ms: now_ms(),
    })
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_cpu_percentage_across_cores() {
        // Half of one core over a two-core window => 50% of the container's limit.
        assert!((cpu_percent(0, 0, 100, 400, 2) - 50.0).abs() < f64::EPSILON);
        // Busy on one of two cores => 50%.
        assert!((cpu_percent(1_000, 10_000, 1_100, 10_200, 2) - 100.0).abs() < 0.001);
    }

    #[test]
    fn handles_missing_or_zero_deltas() {
        assert_eq!(cpu_percent(0, 0, 0, 0, 4), 0.0);
        assert_eq!(cpu_percent(100, 100, 50, 200, 4), 0.0);
        assert_eq!(cpu_percent(50, 200, 100, 200, 4), 0.0);
    }

    #[test]
    fn subtracts_reclaimable_cache_from_memory() {
        let stats = HashMap::from([("inactive_file".to_string(), 400_u64)]);
        assert_eq!(working_set(1_000, 2_000, Some(&stats)), 600);
        assert_eq!(working_set(1_000, 2_000, None), 1_000);
        // Never report more than the limit.
        assert_eq!(working_set(3_000, 2_000, None), 2_000);
    }

    #[test]
    fn memory_percent_is_clamped() {
        assert_eq!(memory_percent(0, 0), 0.0);
        assert_eq!(memory_percent(50, 100), 50.0);
        assert_eq!(memory_percent(200, 100), 100.0);
    }

    #[test]
    fn sums_network_interfaces() {
        let networks = HashMap::from([
            (
                "eth0".to_string(),
                ContainerNetworkStats {
                    rx_bytes: Some(10),
                    tx_bytes: Some(20),
                    ..Default::default()
                },
            ),
            (
                "eth1".to_string(),
                ContainerNetworkStats {
                    rx_bytes: Some(5),
                    tx_bytes: None,
                    ..Default::default()
                },
            ),
        ]);
        assert_eq!(network_totals(Some(&networks)), (15, 20));
        assert_eq!(network_totals(None), (0, 0));
    }
}
