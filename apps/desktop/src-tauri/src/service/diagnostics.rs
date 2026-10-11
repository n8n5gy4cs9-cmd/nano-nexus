//! `/nano-nexus`: a plain-text health report the user (and the model) can read to sort out problems.

use std::fmt::Write as _;
use std::path::PathBuf;

use nexus_core::events::RuntimeState;

use crate::runtime_paths;
use crate::state::AppCtx;

/// Short answers to the questions people ask most. The model gets these with the report.
const HELP: &str = "\
## Quick help
- Models: Models page > download or import a GGUF; the status pill (top right) shows if the model is Ready.
- The model will not start: open the status pill, read its log, press Restart. No model installed: download one in Models.
- Stop an answer: the Stop button; it cancels immediately.
- Activity console: Cmd/Ctrl+J shows every step (tools, commands, errors) with filters.
- Files: assistants only read folders you allow in Permissions; changes are never written without Apply in Changes, and Undo reverts them.
- Phone access: the Share pill > switch on > scan the QR code. Turn it off and the port closes.
- Quitting NanoNexus stops the model server. If one was left behind after a crash, it is cleaned up on the next launch.
- Data lives in the data folder listed above; deleting it resets the app.
";

fn exists(p: &std::path::Path) -> &'static str {
    if p.exists() { "found" } else { "MISSING" }
}

/// Build the report. Reads state only; changes nothing.
pub async fn report(ctx: &AppCtx) -> String {
    let mut out = String::from("# NanoNexus diagnostics\n\n");
    let _ = writeln!(
        out,
        "- Version {} on {} ({})",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(out, "- Data folder: {}", ctx.paths.data.display());
    let _ = writeln!(out, "- Log folder: {}", ctx.paths.logs.display());

    let mut hints: Vec<&str> = Vec::new();

    out.push_str("\n## Model runtime\n");
    let state = ctx.supervisor.state();
    match &state {
        RuntimeState::Ready { model_id, ctx_size, gpu_layers, .. } => {
            let _ = writeln!(out, "- Ready: {model_id} (context {ctx_size}, GPU layers {gpu_layers})");
        }
        RuntimeState::Failed { reason, log_tail, .. } => {
            let _ = writeln!(out, "- FAILED: {reason}");
            for l in log_tail.iter().rev().take(8).rev() {
                let _ = writeln!(out, "    {l}");
            }
            hints.push("The model server failed to start. Press Restart in the status pill; if it fails again, check the log lines above.");
        }
        other => {
            let _ = writeln!(out, "- State: {other:?}");
        }
    }
    let binary = {
        let mut b = runtime_paths::server_binary(&ctx.resource_dir, &ctx.exe_dir);
        if cfg!(debug_assertions) && !b.is_file() {
            let vendor = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../vendor");
            if let Some(dev) = runtime_paths::vendor_server_binary(&vendor) {
                b = dev;
            }
        }
        b
    };
    let _ = writeln!(out, "- llama-server: {} ({})", binary.display(), exists(&binary));
    if !binary.exists() {
        hints.push("llama-server is missing. Run scripts/setup-runtime.sh (macOS/Linux dev builds).");
    }

    out.push_str("\n## Models\n");
    match super::models::list_installed(ctx) {
        Ok(models) if models.is_empty() => {
            out.push_str("- No models installed.\n");
            hints.push("No model is installed. Download one in Models, or run: node scripts/fetch-model.mjs");
        }
        Ok(models) => {
            for m in models.iter().take(10) {
                let _ = writeln!(
                    out,
                    "- {}{} [{}] {}",
                    m.name,
                    if m.active { " (active)" } else { "" },
                    m.source,
                    if m.file_exists { "file found" } else { "FILE MISSING" }
                );
            }
        }
        Err(e) => {
            let _ = writeln!(out, "- Could not list models: {e}");
        }
    }

    out.push_str("\n## LAN Share\n");
    let share = ctx.share.status();
    if share.running {
        let _ = writeln!(out, "- On, port {}, {} device(s): {}", share.port, share.devices, share.urls.join(", "));
    } else {
        let _ = writeln!(out, "- Off{}", share.error.map(|e| format!(" (last error: {e})")).unwrap_or_default());
    }

    let tail = ctx.supervisor.log_tail(12).await;
    if !tail.is_empty() {
        out.push_str("\n## Recent llama-server log\n");
        for l in &tail {
            let _ = writeln!(out, "    {l}");
        }
    }

    out.push_str("\n## What to try\n");
    if hints.is_empty() {
        out.push_str("- Nothing looks wrong.\n");
    } else {
        for h in hints {
            let _ = writeln!(out, "- {h}");
        }
    }
    out.push('\n');
    out.push_str(HELP);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn report_names_the_problems_it_finds() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = crate::state::AppState::for_tests(dir.path());
        let text = report(&ctx).await;
        assert!(text.contains("# NanoNexus diagnostics"));
        assert!(text.contains("No models installed"));
        assert!(text.contains("## Quick help"));
    }
}
