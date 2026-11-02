//! Hardware detection and engine/backend recommendation (UP.045).
//!
//! Pure decision logic over facts the caller supplies, so it can be unit tested without hardware.
//! GPUs are **not probed here**: a GPU is only reported when the caller passes it in, because probing
//! needs per-platform driver calls that have not been written. Unknown is shown as unknown, never as zero.

use serde::{Deserialize, Serialize};

use crate::fit::MemoryInfo;

/// Operating system family the engines are built for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Macos,
    Windows,
    Linux,
}

/// CPU facts that decide which prebuilt engine can start.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct CpuInfo {
    /// `x86_64`, `aarch64` or whatever `std::env::consts::ARCH` says.
    pub arch: String,
    /// `None` when the OS did not say.
    pub physical_cores: Option<u32>,
    pub logical_cores: u32,
    /// x86 only; always `false` elsewhere.
    pub avx: bool,
    pub avx2: bool,
    pub fma: bool,
    /// Apple Silicon and other ARM64 CPUs always have NEON.
    pub neon: bool,
}

/// A graphics device the caller already knows about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct GpuInfo {
    pub vendor: GpuVendor,
    pub name: String,
    /// `None` = unknown.
    pub vram_bytes: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Apple,
    Nvidia,
    Amd,
    Intel,
    Other,
}

/// Prebuilt chat-engine variants. Names match the packaging plan, not upstream release names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Apple Silicon GPU.
    Metal,
    /// Needs an NVIDIA driver on the host.
    Cuda,
    /// Any Vulkan 1.2 driver (NVIDIA, AMD, Intel).
    Vulkan,
    /// AVX2 CPU build.
    Cpu,
    /// Conservative build for CPUs without AVX2 (older PCs). Slow but starts.
    CpuBaseline,
}

/// Why no engine can be offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Unsupported {
    /// An architecture/OS pair that has no prebuilt engine in the plan.
    Platform,
    /// Not enough memory to run even a small model.
    TooLittleMemory,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Recommendation {
    /// `None` when `unsupported` is set.
    pub backend: Option<Backend>,
    pub unsupported: Option<Unsupported>,
    /// Suggested `--threads`: physical cores when known, otherwise half the logical cores, at least 1.
    pub threads: u32,
    /// Suggested context size for the amount of memory.
    pub ctx_size: u32,
    /// Plain-language reasons and limits, in the order to show them.
    pub notes: Vec<String>,
}

/// Parse `llama-server --list-devices` output (ported from the reference `parseLlamaDeviceList`), e.g.
/// `Vulkan0: NVIDIA GeForce RTX 3060 (12288 MiB, 11000 MiB free)`. Largest memory first.
pub fn parse_llama_devices(output: &str) -> Vec<GpuInfo> {
    let Ok(re) =
        regex::Regex::new(r"(?i)(CUDA|Vulkan|SYCL|HIP|ROCm|Metal)\d*\s*:\s*(.+?)\s+\(([\d.]+)\s+MiB")
    else {
        return Vec::new();
    };
    let mut out: Vec<GpuInfo> = re
        .captures_iter(output)
        .filter_map(|c| {
            let mib: f64 = c[3].parse().ok().filter(|m: &f64| *m > 0.0)?;
            let name = c[2].trim().to_owned();
            let lower = name.to_lowercase();
            let vendor = match c[1].to_ascii_lowercase().as_str() {
                "cuda" => GpuVendor::Nvidia,
                "hip" | "rocm" => GpuVendor::Amd,
                "sycl" => GpuVendor::Intel,
                "metal" => GpuVendor::Apple,
                _ if lower.contains("nvidia") || lower.contains("geforce") || lower.contains("rtx") => {
                    GpuVendor::Nvidia
                }
                _ if lower.contains("amd") || lower.contains("radeon") => GpuVendor::Amd,
                _ if lower.contains("intel") => GpuVendor::Intel,
                _ if lower.contains("apple") => GpuVendor::Apple,
                _ => GpuVendor::Other,
            };
            Some(GpuInfo {
                vendor,
                name,
                vram_bytes: Some((mib * 1024.0 * 1024.0) as u64),
            })
        })
        .collect();
    out.sort_by(|a, b| b.vram_bytes.cmp(&a.vram_bytes));
    out
}

/// Ask an engine binary which graphics devices it can use. Failure or timeout means "unknown": an empty list,
/// never a guessed GPU. The binary runs with a cleared environment.
pub async fn probe_gpus(engine_bin: &std::path::Path) -> Vec<GpuInfo> {
    let run = tokio::process::Command::new(engine_bin)
        .arg("--list-devices")
        .env_clear()
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output();
    match tokio::time::timeout(std::time::Duration::from_secs(10), run).await {
        Ok(Ok(o)) => parse_llama_devices(&format!(
            "{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        )),
        _ => Vec::new(),
    }
}

/// Live load figures; `None` means the OS did not report it (shown as unavailable, never as zero).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Telemetry {
    pub cpu_percent: Option<f32>,
    pub ram_used_bytes: u64,
    pub ram_total_bytes: u64,
    pub swap_used_bytes: u64,
}

/// Sample CPU and memory. CPU usage needs two samples, so the first call after start reports `None`.
pub fn sample_telemetry() -> Telemetry {
    use std::sync::{Mutex, OnceLock};
    static SYS: OnceLock<Mutex<(sysinfo::System, bool)>> = OnceLock::new();
    let cell = SYS.get_or_init(|| Mutex::new((sysinfo::System::new(), false)));
    let mut guard = cell.lock().unwrap_or_else(|e| e.into_inner());
    let (sys, primed) = &mut *guard;
    sys.refresh_cpu_usage();
    sys.refresh_memory();
    let cpu = (*primed).then(|| sys.global_cpu_usage());
    *primed = true;
    Telemetry {
        cpu_percent: cpu,
        ram_used_bytes: sys.total_memory().saturating_sub(sys.available_memory()),
        ram_total_bytes: sys.total_memory(),
        swap_used_bytes: sys.used_swap(),
    }
}

/// Memory below which no useful chat model fits.
pub const MIN_MEMORY_BYTES: u64 = 3 * 1024 * 1024 * 1024;
const GIB: u64 = 1024 * 1024 * 1024;

/// Read CPU facts from the running machine.
pub fn detect_cpu() -> CpuInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_cpu_all();
    let logical = u32::try_from(sys.cpus().len()).unwrap_or(1).max(1);
    #[cfg(target_arch = "x86_64")]
    let (avx, avx2, fma) = (
        std::is_x86_feature_detected!("avx"),
        std::is_x86_feature_detected!("avx2"),
        std::is_x86_feature_detected!("fma"),
    );
    #[cfg(not(target_arch = "x86_64"))]
    let (avx, avx2, fma) = (false, false, false);
    CpuInfo {
        arch: std::env::consts::ARCH.to_owned(),
        physical_cores: sysinfo::System::physical_core_count().and_then(|n| u32::try_from(n).ok()),
        logical_cores: logical,
        avx,
        avx2,
        fma,
        neon: cfg!(target_arch = "aarch64"),
    }
}

/// The OS this build runs on, if it is one the plan covers.
pub fn current_os() -> Option<Os> {
    match std::env::consts::OS {
        "macos" => Some(Os::Macos),
        "windows" => Some(Os::Windows),
        "linux" => Some(Os::Linux),
        _ => None,
    }
}

/// Context size by memory budget: small machines get a small window so the KV cache cannot starve the weights.
fn ctx_for(budget_bytes: u64) -> u32 {
    match budget_bytes / GIB {
        0..=3 => 2048,
        4..=7 => 4096,
        8..=15 => 8192,
        _ => 16384,
    }
}

/// Choose an engine variant and conservative defaults.
///
/// `gpus` lists devices the caller has verified; an empty list means "none known", which falls back to the CPU
/// build rather than assuming a GPU. CUDA is never chosen automatically: it needs a host driver that this
/// function cannot confirm, so Vulkan is offered instead and CUDA remains a manual choice.
pub fn recommend(os: Os, cpu: &CpuInfo, mem: &MemoryInfo, gpus: &[GpuInfo]) -> Recommendation {
    let threads = cpu.physical_cores.unwrap_or_else(|| cpu.logical_cores / 2).max(1);
    let budget = crate::fit::budget(mem);
    let mut notes = Vec::new();

    let arm = cpu.arch == "aarch64";
    let x86 = cpu.arch == "x86_64";
    let platform_ok = matches!((os, arm, x86), (Os::Macos, true, _) | (Os::Windows, _, true));
    if !platform_ok {
        notes.push(format!(
            "No prebuilt engine is planned for {} on {}.",
            cpu.arch,
            match os {
                Os::Macos => "macOS",
                Os::Windows => "Windows",
                Os::Linux => "Linux",
            }
        ));
        return Recommendation {
            backend: None,
            unsupported: Some(Unsupported::Platform),
            threads,
            ctx_size: 2048,
            notes,
        };
    }
    if mem.total_bytes < MIN_MEMORY_BYTES {
        notes
            .push("This computer has less than 3 GB of memory, which is too little for a chat model.".into());
        return Recommendation {
            backend: None,
            unsupported: Some(Unsupported::TooLittleMemory),
            threads,
            ctx_size: 2048,
            notes,
        };
    }

    let known_gpu = gpus
        .iter()
        .find(|g| g.vendor != GpuVendor::Other || g.vram_bytes.is_some());
    let backend = if os == Os::Macos {
        notes.push(
            "Apple Silicon: Metal uses the shared memory; about 70% of it is available to the model.".into(),
        );
        Backend::Metal
    } else if let Some(g) = known_gpu {
        notes.push(format!(
            "Graphics device found ({}); Vulkan needs a current driver and falls back to the CPU build if it does not start.",
            g.name
        ));
        if g.vram_bytes.is_none() {
            notes.push("Graphics memory could not be read, so the model may not fit on it.".into());
        }
        Backend::Vulkan
    } else if cpu.avx2 && cpu.fma {
        notes.push("No graphics device was reported; using the CPU build.".into());
        Backend::Cpu
    } else {
        notes.push(
            "This CPU lacks AVX2, so the conservative CPU build is used. Expect slow replies; prefer small models."
                .into(),
        );
        Backend::CpuBaseline
    };

    // A GPU path still needs the CPU build to exist as the fallback.
    if matches!(backend, Backend::Vulkan) && !(cpu.avx2 && cpu.fma) {
        notes.push("The CPU fallback for this machine is the conservative build.".into());
    }
    if budget / GIB < 8 {
        notes.push("Under 8 GB usable: choose a model of about 2B parameters or smaller.".into());
    }
    if os == Os::Windows {
        notes.push(
            "Image and speech workers are not available on older PCs without enough free memory.".into(),
        );
    }

    Recommendation {
        backend: Some(backend),
        unsupported: None,
        threads,
        ctx_size: ctx_for(budget),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn x86(avx2: bool) -> CpuInfo {
        CpuInfo {
            arch: "x86_64".into(),
            physical_cores: Some(4),
            logical_cores: 8,
            avx: true,
            avx2,
            fma: avx2,
            neon: false,
        }
    }
    fn mem(total_gib: u64, unified: bool) -> MemoryInfo {
        MemoryInfo {
            total_bytes: total_gib * GIB,
            available_bytes: total_gib * GIB,
            unified,
        }
    }
    fn gpu() -> GpuInfo {
        GpuInfo {
            vendor: GpuVendor::Nvidia,
            name: "Test GPU".into(),
            vram_bytes: Some(8 * GIB),
        }
    }

    #[test]
    fn device_lines_are_parsed_largest_first() {
        let out = "Available devices:\n  Vulkan0: Intel(R) UHD Graphics (1024 MiB, 900 MiB free)\n  CUDA0: NVIDIA GeForce RTX 3060 (12288 MiB, 11000 MiB free)\n";
        let g = parse_llama_devices(out);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].vendor, GpuVendor::Nvidia);
        assert_eq!(g[0].vram_bytes, Some(12288 * 1024 * 1024));
        assert_eq!(g[1].vendor, GpuVendor::Intel);
        assert!(parse_llama_devices("no devices").is_empty());
    }

    #[test]
    fn apple_silicon_uses_metal() {
        let cpu = CpuInfo {
            arch: "aarch64".into(),
            physical_cores: Some(8),
            logical_cores: 8,
            avx: false,
            avx2: false,
            fma: false,
            neon: true,
        };
        let r = recommend(Os::Macos, &cpu, &mem(16, true), &[]);
        assert_eq!(r.backend, Some(Backend::Metal));
        assert_eq!(r.threads, 8);
    }

    #[test]
    fn windows_without_a_gpu_or_avx2_gets_the_baseline_cpu_build() {
        let r = recommend(Os::Windows, &x86(false), &mem(8, false), &[]);
        assert_eq!(r.backend, Some(Backend::CpuBaseline));
        assert!(r.unsupported.is_none());
    }

    #[test]
    fn windows_with_avx2_and_no_gpu_gets_cpu() {
        let r = recommend(Os::Windows, &x86(true), &mem(16, false), &[]);
        assert_eq!(r.backend, Some(Backend::Cpu));
    }

    #[test]
    fn a_reported_gpu_prefers_vulkan_never_cuda() {
        let r = recommend(Os::Windows, &x86(true), &mem(16, false), &[gpu()]);
        assert_eq!(r.backend, Some(Backend::Vulkan));
    }

    #[test]
    fn unknown_platforms_and_tiny_memory_are_unsupported() {
        let r = recommend(Os::Linux, &x86(true), &mem(16, false), &[]);
        assert_eq!(r.unsupported, Some(Unsupported::Platform));
        assert!(r.backend.is_none());
        let r = recommend(Os::Windows, &x86(true), &mem(2, false), &[]);
        assert_eq!(r.unsupported, Some(Unsupported::TooLittleMemory));
    }

    #[test]
    fn threads_fall_back_to_half_the_logical_cores_and_context_scales_with_memory() {
        let mut cpu = x86(true);
        cpu.physical_cores = None;
        let r = recommend(Os::Windows, &cpu, &mem(32, false), &[]);
        assert_eq!(r.threads, 4);
        assert_eq!(r.ctx_size, 16384);
        let small = recommend(Os::Windows, &cpu, &mem(4, false), &[]);
        assert_eq!(small.ctx_size, 2048);
        assert!(small.notes.iter().any(|n| n.contains("2B")));
    }
}
