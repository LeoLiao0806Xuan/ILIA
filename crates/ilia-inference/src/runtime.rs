use std::{
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{InferenceError, LlamaServerConfig, ManagedLlamaServer};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeBackend {
    Cuda,
    Vulkan,
    Cpu,
}

impl RuntimeBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
            Self::Cpu => "cpu",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimePreference {
    Auto,
    Cuda,
    Vulkan,
    Cpu,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PerformancePreset {
    EnergySaver,
    #[default]
    Balanced,
    HighPerformance,
}

impl std::str::FromStr for PerformancePreset {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "energy_saver" => Ok(Self::EnergySaver),
            "balanced" => Ok(Self::Balanced),
            "high_performance" => Ok(Self::HighPerformance),
            _ => Err(format!("unsupported performance preset: {value}")),
        }
    }
}

impl RuntimePreference {
    pub fn fallback_order(self) -> &'static [RuntimeBackend] {
        match self {
            Self::Auto | Self::Cuda => &[
                RuntimeBackend::Cuda,
                RuntimeBackend::Vulkan,
                RuntimeBackend::Cpu,
            ],
            Self::Vulkan => &[RuntimeBackend::Vulkan, RuntimeBackend::Cpu],
            Self::Cpu => &[RuntimeBackend::Cpu],
        }
    }
}

impl std::str::FromStr for RuntimePreference {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "cuda" => Ok(Self::Cuda),
            "vulkan" => Ok(Self::Vulkan),
            "cpu" => Ok(Self::Cpu),
            _ => Err(format!("unsupported backend preference: {value}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeDevice {
    pub id: String,
    pub name: String,
    pub total_memory_mib: Option<u64>,
    pub free_memory_mib: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackendProbe {
    pub backend: RuntimeBackend,
    pub executable: PathBuf,
    pub available: bool,
    pub devices: Vec<RuntimeDevice>,
    pub recommended_device: Option<String>,
    pub diagnostic: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProbeReport {
    pub preference: RuntimePreference,
    pub fallback_order: Vec<RuntimeBackend>,
    pub recommended_backend: Option<RuntimeBackend>,
    pub probes: Vec<BackendProbe>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeProfile {
    pub backend: RuntimeBackend,
    pub context_size: usize,
    pub gpu_layers: i32,
    pub device: Option<String>,
    pub flash_attention: bool,
    pub estimated_memory_mib: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeAttempt {
    pub backend: RuntimeBackend,
    pub executable: PathBuf,
    pub started: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeStartupReport {
    pub detection: RuntimeProbeReport,
    pub selected_backend: RuntimeBackend,
    pub selected_profile: RuntimeProfile,
    pub attempts: Vec<RuntimeAttempt>,
}

#[derive(Debug, Clone)]
pub struct RuntimeManagerConfig {
    pub runtime_root: PathBuf,
    pub model: PathBuf,
    pub preference: RuntimePreference,
    pub performance_preset: PerformancePreset,
    pub host: String,
    pub port: u16,
    pub startup_timeout: Duration,
    pub log_dir: PathBuf,
    pub api_key: Option<String>,
}

pub struct AutoManagedLlamaServer {
    server: ManagedLlamaServer,
    report: RuntimeStartupReport,
}

impl AutoManagedLlamaServer {
    pub fn base_url(&self) -> &str {
        self.server.base_url()
    }

    pub fn backend(&self) -> RuntimeBackend {
        self.report.selected_backend
    }

    pub fn report(&self) -> &RuntimeStartupReport {
        &self.report
    }
}

pub struct RuntimeManager;

impl RuntimeManager {
    pub fn probe(
        runtime_root: impl AsRef<Path>,
        preference: RuntimePreference,
    ) -> RuntimeProbeReport {
        let runtime_root = runtime_root.as_ref();
        let probes = [
            RuntimeBackend::Cuda,
            RuntimeBackend::Vulkan,
            RuntimeBackend::Cpu,
        ]
        .into_iter()
        .map(|backend| probe_backend(runtime_root, backend))
        .collect::<Vec<_>>();
        let recommended_backend = preference.fallback_order().iter().copied().find(|backend| {
            probes
                .iter()
                .any(|probe| probe.backend == *backend && probe.available)
        });
        RuntimeProbeReport {
            preference,
            fallback_order: preference.fallback_order().to_vec(),
            recommended_backend,
            probes,
        }
    }

    pub fn start(config: &RuntimeManagerConfig) -> Result<AutoManagedLlamaServer, InferenceError> {
        Self::start_internal(config, None)
    }

    pub fn start_cancellable(
        config: &RuntimeManagerConfig,
        cancellation: &crate::CancellationToken,
    ) -> Result<AutoManagedLlamaServer, InferenceError> {
        Self::start_internal(config, Some(cancellation))
    }

    fn start_internal(
        config: &RuntimeManagerConfig,
        cancellation: Option<&crate::CancellationToken>,
    ) -> Result<AutoManagedLlamaServer, InferenceError> {
        let detection = Self::probe(&config.runtime_root, config.preference);
        let mut attempts = Vec::new();
        for backend in config.preference.fallback_order() {
            if let Some(cancellation) = cancellation {
                cancellation.check()?;
            }
            let probe = detection
                .probes
                .iter()
                .find(|probe| probe.backend == *backend)
                .expect("all backend probes are present");
            if !probe.available {
                attempts.push(RuntimeAttempt {
                    backend: *backend,
                    executable: probe.executable.clone(),
                    started: false,
                    error: Some(probe.diagnostic.clone()),
                });
                continue;
            }
            let profile = runtime_profile(
                *backend,
                probe.recommended_device.clone(),
                config.performance_preset,
                std::fs::metadata(&config.model)
                    .map(|metadata| metadata.len())
                    .unwrap_or_default(),
            );
            let server_config = LlamaServerConfig {
                executable: probe.executable.clone(),
                model: config.model.clone(),
                host: config.host.clone(),
                port: config.port,
                context_size: profile.context_size,
                gpu_layers: profile.gpu_layers,
                startup_timeout: config.startup_timeout,
                log_path: config
                    .log_dir
                    .join(format!("llama-server-{}.log", backend.as_str())),
                api_key: config.api_key.clone(),
                device: profile.device.clone(),
                flash_attention: profile.flash_attention,
            };
            let started = match cancellation {
                Some(cancellation) => {
                    ManagedLlamaServer::start_cancellable(&server_config, cancellation)
                }
                None => ManagedLlamaServer::start(&server_config),
            };
            match started {
                Ok(server) => {
                    attempts.push(RuntimeAttempt {
                        backend: *backend,
                        executable: probe.executable.clone(),
                        started: true,
                        error: None,
                    });
                    return Ok(AutoManagedLlamaServer {
                        server,
                        report: RuntimeStartupReport {
                            detection,
                            selected_backend: *backend,
                            selected_profile: profile,
                            attempts,
                        },
                    });
                }
                Err(error) => {
                    if cancellation.is_some_and(crate::CancellationToken::is_cancelled) {
                        return Err(InferenceError::Cancelled);
                    }
                    attempts.push(RuntimeAttempt {
                        backend: *backend,
                        executable: probe.executable.clone(),
                        started: false,
                        error: Some(error.to_string()),
                    });
                }
            }
        }
        let summary = attempts
            .iter()
            .map(|attempt| {
                format!(
                    "{}: {}",
                    attempt.backend.as_str(),
                    attempt.error.as_deref().unwrap_or("unknown error")
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        Err(InferenceError::NoUsableRuntime(summary))
    }
}

fn executable_path(runtime_root: &Path, backend: RuntimeBackend) -> PathBuf {
    runtime_root.join(backend.as_str()).join(if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    })
}

fn probe_backend(runtime_root: &Path, backend: RuntimeBackend) -> BackendProbe {
    let executable = executable_path(runtime_root, backend);
    if !executable.is_file() {
        return BackendProbe {
            backend,
            diagnostic: format!("runtime executable is missing: {}", executable.display()),
            executable,
            available: false,
            devices: Vec::new(),
            recommended_device: None,
        };
    }
    let output = hidden_output(&executable, &["--list-devices"], Duration::from_secs(10));
    let Ok(output) = output else {
        return BackendProbe {
            backend,
            diagnostic: format!("runtime probe could not start: {}", output.unwrap_err()),
            executable,
            available: false,
            devices: Vec::new(),
            recommended_device: None,
        };
    };
    let diagnostic = combined_output(&output);
    let devices = parse_devices(&diagnostic);
    let available = output.status.success()
        && match backend {
            RuntimeBackend::Cuda => devices.iter().any(|device| device.id.starts_with("CUDA")),
            RuntimeBackend::Vulkan => devices.iter().any(|device| device.id.starts_with("Vulkan")),
            RuntimeBackend::Cpu => true,
        };
    let recommended_device = match backend {
        RuntimeBackend::Cuda => devices
            .iter()
            .find(|device| device.id.starts_with("CUDA"))
            .map(|device| device.id.clone()),
        RuntimeBackend::Vulkan => {
            recommended_vulkan_device(&devices).map(|device| device.id.clone())
        }
        RuntimeBackend::Cpu => Some("none".to_owned()),
    };
    BackendProbe {
        backend,
        executable,
        available,
        devices,
        recommended_device,
        diagnostic: if available {
            diagnostic
        } else if diagnostic.trim().is_empty() {
            "runtime reported no compatible device".to_owned()
        } else {
            diagnostic
        },
    }
}

pub fn runtime_profile(
    backend: RuntimeBackend,
    device: Option<String>,
    preset: PerformancePreset,
    model_bytes: u64,
) -> RuntimeProfile {
    let (context_size, gpu_layers) = match (backend, preset) {
        (RuntimeBackend::Cpu, PerformancePreset::EnergySaver) => (2_048, 0),
        (RuntimeBackend::Cpu, PerformancePreset::Balanced) => (4_096, 0),
        (RuntimeBackend::Cpu, PerformancePreset::HighPerformance) => (8_192, 0),
        (RuntimeBackend::Vulkan, PerformancePreset::EnergySaver) => (4_096, 24),
        (RuntimeBackend::Vulkan, PerformancePreset::Balanced) => (8_192, 99),
        (RuntimeBackend::Vulkan, PerformancePreset::HighPerformance) => (16_384, 99),
        (RuntimeBackend::Cuda, PerformancePreset::EnergySaver) => (4_096, 24),
        (RuntimeBackend::Cuda, PerformancePreset::Balanced) => (16_384, 99),
        (RuntimeBackend::Cuda, PerformancePreset::HighPerformance) => (32_768, 99),
    };
    let estimated_memory_mib = model_bytes.div_ceil(1024 * 1024)
        + (context_size as u64 * if backend == RuntimeBackend::Cpu { 1 } else { 2 }) / 4
        + 512;
    RuntimeProfile {
        backend,
        context_size,
        gpu_layers,
        device: if backend == RuntimeBackend::Cpu {
            Some("none".to_owned())
        } else {
            device
        },
        flash_attention: backend != RuntimeBackend::Cpu,
        estimated_memory_mib,
    }
}

fn hidden_output(executable: &Path, args: &[&str], timeout: Duration) -> std::io::Result<Output> {
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return child.wait_with_output();
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("runtime probe exceeded {timeout:?}"),
            ));
        }
        thread::sleep(Duration::from_millis(50));
    }
}

fn combined_output(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{stdout}{stderr}").trim().to_owned()
}

fn parse_devices(output: &str) -> Vec<RuntimeDevice> {
    let expression =
        Regex::new(r"(?m)^\s*((?:CUDA|Vulkan)\d+):\s+(.+?)\s+\((\d+) MiB,\s+(\d+) MiB free\)\s*$")
            .expect("valid device regex");
    expression
        .captures_iter(output)
        .map(|capture| RuntimeDevice {
            id: capture[1].to_owned(),
            name: capture[2].to_owned(),
            total_memory_mib: capture[3].parse().ok(),
            free_memory_mib: capture[4].parse().ok(),
        })
        .collect()
}

fn recommended_vulkan_device(devices: &[RuntimeDevice]) -> Option<&RuntimeDevice> {
    devices.iter().max_by_key(|device| {
        let name = device.name.to_ascii_lowercase();
        let class = if name.contains("nvidia") || name.contains("radeon") {
            3u64
        } else if name.contains("arc") {
            2
        } else {
            1
        };
        (class, device.free_memory_mib.unwrap_or_default())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_orders_are_stable() {
        assert_eq!(
            RuntimePreference::Auto.fallback_order(),
            &[
                RuntimeBackend::Cuda,
                RuntimeBackend::Vulkan,
                RuntimeBackend::Cpu
            ]
        );
        assert_eq!(
            RuntimePreference::Vulkan.fallback_order(),
            &[RuntimeBackend::Vulkan, RuntimeBackend::Cpu]
        );
        assert_eq!(
            RuntimePreference::Cpu.fallback_order(),
            &[RuntimeBackend::Cpu]
        );
    }

    #[test]
    fn parses_devices_and_prefers_discrete_vulkan_gpu() {
        let devices = parse_devices(
            "Available devices:\n  Vulkan0: Intel(R) UHD Graphics 770 (16248 MiB, 15480 MiB free)\n  Vulkan1: NVIDIA GeForce RTX 4070 Laptop GPU (7948 MiB, 7180 MiB free)",
        );
        assert_eq!(devices.len(), 2);
        assert_eq!(recommended_vulkan_device(&devices).unwrap().id, "Vulkan1");
    }

    #[test]
    fn backend_profiles_reduce_resources_during_fallback() {
        let cuda = runtime_profile(
            RuntimeBackend::Cuda,
            Some("CUDA0".to_owned()),
            PerformancePreset::Balanced,
            2_500_000_000,
        );
        let vulkan = runtime_profile(
            RuntimeBackend::Vulkan,
            Some("Vulkan0".to_owned()),
            PerformancePreset::Balanced,
            2_500_000_000,
        );
        let cpu = runtime_profile(
            RuntimeBackend::Cpu,
            None,
            PerformancePreset::Balanced,
            2_500_000_000,
        );
        assert!(cuda.context_size > vulkan.context_size);
        assert!(vulkan.context_size > cpu.context_size);
        assert_eq!(cpu.gpu_layers, 0);
        assert_eq!(cpu.device.as_deref(), Some("none"));
    }

    #[test]
    fn performance_presets_map_to_monotonic_resource_profiles() {
        let saver = runtime_profile(
            RuntimeBackend::Cuda,
            None,
            PerformancePreset::EnergySaver,
            1_000_000_000,
        );
        let balanced = runtime_profile(
            RuntimeBackend::Cuda,
            None,
            PerformancePreset::Balanced,
            1_000_000_000,
        );
        let fast = runtime_profile(
            RuntimeBackend::Cuda,
            None,
            PerformancePreset::HighPerformance,
            1_000_000_000,
        );
        assert!(saver.context_size < balanced.context_size);
        assert!(balanced.context_size < fast.context_size);
        assert!(saver.estimated_memory_mib < fast.estimated_memory_mib);
    }
}
