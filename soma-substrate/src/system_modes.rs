use soma_common::{SystemMode, SystemStateSnapshot, InformationTopology, StateChannel};

pub struct ModeEngine {
    pub current_mode: SystemMode,
}

impl ModeEngine {
    pub fn new() -> Self {
        Self {
            current_mode: SystemMode::Idle,
        }
    }

    pub fn evaluate(&mut self, state: &SystemStateSnapshot) -> SystemMode {
        let avg_cpu = if state.cpu_load.is_empty() {
            0.0
        } else {
            state.cpu_load.iter().sum::<f64>() / state.cpu_load.len() as f64
        };

        let mem_usage_ratio = state.memory_used_kb as f64 / state.memory_total_kb.max(1) as f64;
        let disk_free_bytes = state.disk_total_bytes.saturating_sub(state.disk_used_bytes);

        // Degraded mode: free disk space less than 2GB
        if disk_free_bytes < 2 * 1024 * 1024 * 1024 {
            self.current_mode = SystemMode::Degraded;
            return self.current_mode;
        }

        // Stressed mode: memory usage ratio > 0.92 or process count is excessively high
        if mem_usage_ratio > 0.92 || state.process_count > 600 {
            self.current_mode = SystemMode::Stressed;
            return self.current_mode;
        }

        // UnderLoad mode: CPU load > 75% or memory usage > 80%
        if avg_cpu > 75.0 || mem_usage_ratio > 0.80 {
            self.current_mode = SystemMode::UnderLoad;
            return self.current_mode;
        }

        // Idle mode: CPU load < 10%
        if avg_cpu < 10.0 {
            self.current_mode = SystemMode::Idle;
            return self.current_mode;
        }

        // Normal operating mode
        self.current_mode = SystemMode::Active;
        self.current_mode
    }

    pub fn get_information_topology(&self) -> InformationTopology {
        match self.current_mode {
            SystemMode::Idle => InformationTopology {
                primary_channels: vec![StateChannel::Cpu, StateChannel::Memory],
                secondary_channels: vec![StateChannel::Disk, StateChannel::Process, StateChannel::Network],
                coherent_actions: vec!["system.memory_info".to_string(), "filesystem.read_file".to_string()],
                incoherent_actions: Vec::new(),
            },
            SystemMode::Active => InformationTopology {
                primary_channels: vec![StateChannel::Cpu, StateChannel::Memory, StateChannel::Network],
                secondary_channels: vec![StateChannel::Disk, StateChannel::Process],
                coherent_actions: vec!["process.list_processes".to_string(), "network.ping".to_string()],
                incoherent_actions: Vec::new(),
            },
            SystemMode::UnderLoad | SystemMode::Stressed => InformationTopology {
                primary_channels: vec![StateChannel::Cpu, StateChannel::Memory, StateChannel::Process],
                secondary_channels: vec![StateChannel::Disk, StateChannel::Network],
                coherent_actions: vec!["process.list_processes".to_string(), "process.kill_process".to_string(), "system.memory_info".to_string()],
                // Contextually incoherent actions: installing new packages or spawning heavy agent mode loops
                incoherent_actions: vec![
                    "package.install_package".to_string(),
                    "desktop_agent.start".to_string(),
                    "browser.navigate".to_string()
                ],
            },
            SystemMode::Degraded => InformationTopology {
                primary_channels: vec![StateChannel::Disk, StateChannel::Process],
                secondary_channels: vec![StateChannel::Cpu, StateChannel::Memory, StateChannel::Network],
                coherent_actions: vec!["filesystem.list_dir".to_string(), "filesystem.delete_file".to_string()],
                incoherent_actions: vec![
                    "filesystem.write_file".to_string(),
                    "package.install_package".to_string()
                ],
            },
            _ => InformationTopology {
                primary_channels: vec![StateChannel::Cpu, StateChannel::Memory],
                secondary_channels: vec![StateChannel::Disk, StateChannel::Process, StateChannel::Network],
                coherent_actions: Vec::new(),
                incoherent_actions: Vec::new(),
            }
        }
    }
}
