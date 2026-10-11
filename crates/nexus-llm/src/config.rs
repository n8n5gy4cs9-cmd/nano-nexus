//! Launch configuration for `llama-server` (INFERENCE-CONTRACT §1).

use std::path::PathBuf;
use std::time::Duration;

use nexus_core::settings::Settings;

/// Everything needed to start the server, minus the port and token.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeConfig {
    pub server_bin: PathBuf,
    pub model_path: PathBuf,
    /// Id reported to the UI (setting `models.active_model_id`).
    pub model_id: String,
    pub ctx_size: u32,
    /// `"all"` or a layer count.
    pub gpu_layers: String,
    /// `"auto"`, `"on"` or `"off"`.
    pub flash_attn: String,
    pub threads: Option<u32>,
    /// Enables `--metrics`.
    pub metrics: bool,
    /// How long to wait for `/health` + `/v1/models` before giving up.
    pub ready_timeout: Duration,
    /// Spawn attempts (new port each time) before giving up; the contract says 16.
    pub max_attempts: u8,
}

impl RuntimeConfig {
    /// Defaults from the contract for the given binary and model.
    pub fn new(server_bin: impl Into<PathBuf>, model_path: impl Into<PathBuf>) -> Self {
        Self {
            server_bin: server_bin.into(),
            model_path: model_path.into(),
            model_id: "builtin".into(),
            ctx_size: 8192,
            gpu_layers: "all".into(),
            flash_attn: "auto".into(),
            threads: None,
            metrics: false,
            ready_timeout: Duration::from_secs(90),
            max_attempts: 16,
        }
    }

    /// Apply the `runtime.*` / `models.active_model_id` settings.
    pub fn from_settings(
        settings: &Settings,
        server_bin: impl Into<PathBuf>,
        model_path: impl Into<PathBuf>,
    ) -> Self {
        let mut c = Self::new(server_bin, model_path);
        c.model_id = settings.models_active_model_id.clone();
        c.ctx_size = settings.runtime_ctx_size.clamp(4096, 32768);
        c.gpu_layers = settings.runtime_gpu_layers.clone();
        c.flash_attn = settings.runtime_flash_attn.clone();
        c.threads = settings.runtime_threads;
        c
    }

    /// Command-line arguments for the given port. The API key is passed via the environment, never here.
    pub fn to_args(&self, port: u16) -> Vec<String> {
        let mut a: Vec<String> = vec![
            "-m".into(),
            self.model_path.to_string_lossy().into_owned(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            port.to_string(),
            "--ctx-size".into(),
            self.ctx_size.to_string(),
            "--parallel".into(),
            "1".into(),
            "--n-gpu-layers".into(),
            self.gpu_layers.clone(),
            "--flash-attn".into(),
            self.flash_attn.clone(),
            "--jinja".into(),
            "--reasoning-format".into(),
            "deepseek".into(),
            "--no-webui".into(),
            "--timeout".into(),
            "120".into(),
            "--cache-reuse".into(),
            "256".into(),
        ];
        if let Some(t) = self.threads {
            a.extend(["--threads".into(), t.to_string()]);
        }
        if self.metrics {
            a.push("--metrics".into());
        }
        a
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .map(String::as_str)
    }

    #[test]
    fn default_args_match_contract() {
        let c = RuntimeConfig::new("/bin/llama-server", "/m/model.gguf");
        let a = c.to_args(5555);
        assert_eq!(pair(&a, "-m"), Some("/m/model.gguf"));
        assert_eq!(pair(&a, "--host"), Some("127.0.0.1"));
        assert_eq!(pair(&a, "--port"), Some("5555"));
        assert_eq!(pair(&a, "--ctx-size"), Some("8192"));
        assert_eq!(pair(&a, "--parallel"), Some("1"));
        assert_eq!(pair(&a, "--n-gpu-layers"), Some("all"));
        assert_eq!(pair(&a, "--reasoning-format"), Some("deepseek"));
        assert!(a.contains(&"--jinja".to_owned()) && a.contains(&"--no-webui".to_owned()));
        assert!(!a.contains(&"--metrics".to_owned()) && !a.contains(&"--threads".to_owned()));
        assert!(
            !a.iter()
                .any(|x| x.to_lowercase().contains("key") || x.to_lowercase().contains("token"))
        );
    }

    #[test]
    fn settings_are_applied_and_clamped() {
        let mut s = Settings::default();
        s.runtime_ctx_size = 100_000;
        s.runtime_threads = Some(4);
        s.runtime_gpu_layers = "20".into();
        let c = RuntimeConfig::from_settings(&s, "/b", "/m");
        assert_eq!(c.ctx_size, 32768);
        let a = c.to_args(1);
        assert_eq!(pair(&a, "--threads"), Some("4"));
        assert_eq!(pair(&a, "--n-gpu-layers"), Some("20"));
    }
}
