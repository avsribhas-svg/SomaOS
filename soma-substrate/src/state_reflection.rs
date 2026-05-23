use std::time::{SystemTime, UNIX_EPOCH};
use sysinfo::{System, Disks};
use soma_common::{SystemStateSnapshot, StateDelta};

pub struct StateReflector {
    sys: System,
}

impl StateReflector {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self { sys }
    }

    pub fn capture_snapshot(&mut self) -> SystemStateSnapshot {
        self.sys.refresh_cpu();
        self.sys.refresh_memory();
        self.sys.refresh_processes();
        
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let cpu_load: Vec<f64> = self.sys.cpus().iter().map(|cpu| cpu.cpu_usage() as f64).collect();
        
        let memory_total_kb = self.sys.total_memory();
        let memory_used_kb = self.sys.used_memory();

        // Disk usage sum
        let mut disk_total_bytes = 0;
        let mut disk_used_bytes = 0;
        let disks = Disks::new_with_refreshed_list();
        for disk in &disks {
            disk_total_bytes += disk.total_space();
            disk_used_bytes += disk.total_space() - disk.available_space();
        }

        // Fallback for disk total if zero (e.g. inside sandbox or raw KMS platform)
        if disk_total_bytes == 0 {
            disk_total_bytes = 100 * 1024 * 1024 * 1024; // 100GB dummy
            disk_used_bytes = 10 * 1024 * 1024 * 1024;  // 10GB dummy
        }

        let process_count = self.sys.processes().len() as u32;
        let system_uptime_secs = System::uptime();

        SystemStateSnapshot {
            timestamp_ms,
            cpu_load,
            memory_used_kb,
            memory_total_kb,
            disk_used_bytes,
            disk_total_bytes,
            process_count,
            system_uptime_secs,
        }
    }

    pub fn compute_delta(&self, before: SystemStateSnapshot, after: SystemStateSnapshot, action_capability: &str, action_name: &str) -> StateDelta {
        // Generate a human-readable summary of differences
        let mut diffs = Vec::new();

        // CPU change
        let before_cpu: f64 = if before.cpu_load.is_empty() { 0.0 } else { before.cpu_load.iter().sum::<f64>() / before.cpu_load.len() as f64 };
        let after_cpu: f64 = if after.cpu_load.is_empty() { 0.0 } else { after.cpu_load.iter().sum::<f64>() / after.cpu_load.len() as f64 };
        let cpu_diff = after_cpu - before_cpu;
        if cpu_diff.abs() > 2.0 {
            diffs.push(format!("Average CPU load changed by {:.1}% (from {:.1}% to {:.1}%)", cpu_diff, before_cpu, after_cpu));
        }

        // Memory change
        let mem_diff_kb = after.memory_used_kb as i64 - before.memory_used_kb as i64;
        if mem_diff_kb.abs() > 1024 { // change > 1MB
            let mb = mem_diff_kb as f64 / 1024.0;
            diffs.push(format!("Memory usage changed by {:.2} MB (used {:.2} MB / {:.2} MB)", mb, after.memory_used_kb as f64 / 1024.0, after.memory_total_kb as f64 / 1024.0));
        }

        // Disk change
        let disk_diff_bytes = after.disk_used_bytes as i64 - before.disk_used_bytes as i64;
        if disk_diff_bytes.abs() > 4096 { // change > 4KB
            let kb = disk_diff_bytes as f64 / 1024.0;
            diffs.push(format!("Disk space usage changed by {:.2} KB", kb));
        }

        // Process count change
        let proc_diff = after.process_count as i32 - before.process_count as i32;
        if proc_diff != 0 {
            diffs.push(format!("Process count changed by {} (from {} to {})", proc_diff, before.process_count, after.process_count));
        }

        let delta_summary = if diffs.is_empty() {
            "No significant resource state changes detected.".to_string()
        } else {
            diffs.join("\n")
        };

        StateDelta {
            before,
            after,
            action_capability: action_capability.to_string(),
            action_name: action_name.to_string(),
            delta_summary,
        }
    }
}
