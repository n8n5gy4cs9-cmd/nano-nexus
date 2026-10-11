//! Detect which compute backend the server picked from its startup log (shown in the runtime panel).

/// Compute backend in use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Metal,
    Vulkan,
    Cuda,
    Cpu,
}

impl Backend {
    /// Human label for the UI.
    pub fn label(self) -> &'static str {
        match self {
            Backend::Metal => "GPU (Metal)",
            Backend::Vulkan => "GPU (Vulkan)",
            Backend::Cuda => "GPU (CUDA)",
            Backend::Cpu => "CPU",
        }
    }
}

/// Inspect log lines; `None` if there is no evidence yet.
pub fn detect_backend<'a>(lines: impl IntoIterator<Item = &'a str>) -> Option<Backend> {
    let mut seen_any = false;
    for line in lines {
        seen_any = true;
        let l = line.to_lowercase();
        if l.contains("ggml_metal") && !l.contains("no devices") {
            return Some(Backend::Metal);
        }
        if let Some(rest) = l.split("ggml_vulkan: found ").nth(1) {
            let n: u32 = rest
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            return Some(if n > 0 { Backend::Vulkan } else { Backend::Cpu });
        }
        if l.contains("using device vulkan") {
            return Some(Backend::Vulkan);
        }
        if l.contains("ggml_cuda_init: found") {
            return Some(Backend::Cuda);
        }
    }
    let _ = seen_any;
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_backends() {
        assert_eq!(
            detect_backend(["ggml_vulkan: Found 1 Vulkan devices:"]),
            Some(Backend::Vulkan)
        );
        assert_eq!(
            detect_backend(["ggml_vulkan: Found 0 Vulkan devices"]),
            Some(Backend::Cpu)
        );
        assert_eq!(
            detect_backend(["llama: using device Vulkan0 (AMD)"]),
            Some(Backend::Vulkan)
        );
        assert_eq!(
            detect_backend(["ggml_metal_init: found device: Apple M2"]),
            Some(Backend::Metal)
        );
        assert_eq!(detect_backend(["loading model"]), None);
        assert_eq!(Backend::Vulkan.label(), "GPU (Vulkan)");
    }
}
