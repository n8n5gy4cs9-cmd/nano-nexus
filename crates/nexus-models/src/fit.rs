//! "Will it run?" estimate (MODEL-MANAGER §6): weights + KV cache + runtime overhead against the memory
//! the GPU/CPU can really use.

use serde::{Deserialize, Serialize};

use crate::gguf::GgufMeta;

/// Compute buffers and runtime overhead.
pub const OVERHEAD_BYTES: u64 = 350 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    /// Comfortably fits.
    Great,
    /// Fits, with little headroom.
    Tight,
    /// Will not fit.
    TooBig,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MemoryInfo {
    pub total_bytes: u64,
    pub available_bytes: u64,
    /// Apple Silicon: GPU and CPU share RAM, and Metal may use about 70% of it.
    pub unified: bool,
}

/// Read total/available RAM from the OS.
pub fn system_memory() -> MemoryInfo {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    MemoryInfo {
        total_bytes: sys.total_memory(),
        available_bytes: sys.available_memory(),
        unified: cfg!(all(target_os = "macos", target_arch = "aarch64")),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Fit {
    pub weights_bytes: u64,
    pub kv_cache_bytes: u64,
    pub need_bytes: u64,
    pub budget_bytes: u64,
    pub badge: Badge,
}

/// The memory the model may use: 70% of unified memory on a Mac, 85% of what is free elsewhere.
pub fn budget(mem: &MemoryInfo) -> u64 {
    if mem.unified {
        mem.total_bytes / 100 * 70
    } else {
        mem.available_bytes / 100 * 85
    }
}

/// `2 (K and V) × layers × ctx × kv_heads × head_dim × 2 bytes (f16)`.
pub fn kv_cache_bytes(n_layers: u64, ctx: u64, n_kv_heads: u64, head_dim: u64) -> u64 {
    2u64.saturating_mul(n_layers)
        .saturating_mul(ctx)
        .saturating_mul(n_kv_heads)
        .saturating_mul(head_dim)
        .saturating_mul(2)
}

/// Estimate whether a model file runs with `ctx` tokens of context. Missing header fields fall back to a
/// rough rule (KV cache ≈ 12% of the weights at 8k context, scaled with `ctx`).
pub fn estimate(meta: Option<&GgufMeta>, file_bytes: u64, ctx: u32, mem: &MemoryInfo) -> Fit {
    let kv = meta
        .and_then(|m| {
            let layers = m.block_count?;
            let heads = m.head_count?.max(1);
            let kv_heads = m.head_count_kv.unwrap_or(heads);
            let head_dim = m.embedding_length? / heads;
            Some(kv_cache_bytes(layers, u64::from(ctx), kv_heads, head_dim))
        })
        .unwrap_or_else(|| (file_bytes as f64 * 0.12 * (f64::from(ctx) / 8192.0)) as u64);
    let need = file_bytes.saturating_add(kv).saturating_add(OVERHEAD_BYTES);
    let budget = budget(mem);
    let badge = if need as f64 <= budget as f64 * 0.6 {
        Badge::Great
    } else if need <= budget {
        Badge::Tight
    } else {
        Badge::TooBig
    };
    Fit {
        weights_bytes: file_bytes,
        kv_cache_bytes: kv,
        need_bytes: need,
        budget_bytes: budget,
        badge,
    }
}

/// "needs ≈ 1.6 GB of 11.2 GB usable"
pub fn describe(f: &Fit) -> String {
    let gb = |b: u64| b as f64 / 1_073_741_824.0;
    format!(
        "needs ≈ {:.1} GB of {:.1} GB usable",
        gb(f.need_bytes),
        gb(f.budget_bytes)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB: u64 = 1_073_741_824;

    fn mac(total_gb: u64) -> MemoryInfo {
        MemoryInfo {
            total_bytes: total_gb * GB,
            available_bytes: total_gb * GB / 2,
            unified: true,
        }
    }

    fn meta() -> GgufMeta {
        GgufMeta {
            block_count: Some(28),
            embedding_length: Some(2048),
            head_count: Some(16),
            head_count_kv: Some(8),
            ..GgufMeta::default()
        }
    }

    #[test]
    fn kv_cache_formula() {
        // 2 * 28 layers * 8192 ctx * 8 kv heads * 128 head_dim * 2 bytes = 939,524,096
        assert_eq!(kv_cache_bytes(28, 8192, 8, 128), 939_524_096);
        let f = estimate(Some(&meta()), 1_100_000_000, 8192, &mac(16));
        assert_eq!(f.kv_cache_bytes, 939_524_096);
        assert_eq!(f.need_bytes, 1_100_000_000 + 939_524_096 + OVERHEAD_BYTES);
    }

    #[test]
    fn badges_follow_the_budget() {
        // 16 GB Mac: budget 11.2 GB.
        assert_eq!(
            estimate(Some(&meta()), 1_100_000_000, 8192, &mac(16)).badge,
            Badge::Great
        );
        assert_eq!(
            estimate(Some(&meta()), 7 * GB, 8192, &mac(16)).badge,
            Badge::Tight
        );
        assert_eq!(
            estimate(Some(&meta()), 12 * GB, 8192, &mac(16)).badge,
            Badge::TooBig
        );
        // 8 GB Mac: budget 5.6 GB; a 4 GB model with its cache is tight.
        assert_eq!(estimate(Some(&meta()), 3 * GB, 8192, &mac(8)).badge, Badge::Tight);
    }

    #[test]
    fn budget_rules() {
        assert_eq!(budget(&mac(16)), 16 * GB / 100 * 70);
        let pc = MemoryInfo {
            total_bytes: 32 * GB,
            available_bytes: 20 * GB,
            unified: false,
        };
        assert_eq!(budget(&pc), 20 * GB / 100 * 85);
    }

    #[test]
    fn missing_header_fields_use_the_fallback_and_ctx_scales_the_cache() {
        let small = estimate(None, GB, 8192, &mac(16));
        let big = estimate(None, GB, 32768, &mac(16));
        assert!(big.kv_cache_bytes > small.kv_cache_bytes * 3);
        assert!(small.need_bytes > GB);
    }

    #[test]
    fn describe_reads_well() {
        let f = estimate(Some(&meta()), 1_100_000_000, 8192, &mac(16));
        assert!(
            describe(&f).starts_with("needs ≈ 2.2 GB of 11.2 GB"),
            "{}",
            describe(&f)
        );
    }

    #[test]
    fn system_memory_is_positive() {
        assert!(system_memory().total_bytes > 0);
    }
}
