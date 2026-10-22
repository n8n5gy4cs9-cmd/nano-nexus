//! Isolated text-only specialists, porting REFE Pi's task/result delegation pattern.
//! Children use the active model and receive no history, tools, or file authority.

use nexus_agent::tools::{CONSULTANTS, ConsultationSource};
use nexus_agent::{AssistantSpec, PromptContext, prompt, render_system_prompt};
use nexus_llm::{ChatMessage, ChatRequest, Priority};
use tokio_util::sync::CancellationToken;

use crate::state::Ctx;

/// Specialist backend shared with the parent router.
pub(super) struct AppConsultations(pub Ctx);

fn specialist(ctx: &Ctx, id: &str) -> Result<AssistantSpec, String> {
    if !CONSULTANTS.contains(&id) {
        return Err("Only planner and reviewer may be consulted.".into());
    }
    let registry = ctx.registry.read().map_err(|e| e.to_string())?;
    let entry = registry.get(id).ok_or("Specialist not found.")?;
    if !entry.enabled {
        return Err(format!("{id} is disabled. Enable it in Assistants."));
    }
    Ok(entry.spec.clone())
}

fn isolated_request(spec: &AssistantSpec, task: &str) -> Result<ChatRequest, String> {
    if task.trim().is_empty() || task.chars().count() > 6000 {
        return Err("Supply a task of 1–6000 characters.".into());
    }
    let context = PromptContext::new(prompt::os_display_name(std::env::consts::OS), prompt::today_iso());
    let system = render_system_prompt(spec, &context).map_err(|e| e.to_string())?;
    let mut request = ChatRequest::new(vec![
        ChatMessage::system(format!(
            "{system}\nYou are an isolated consultant. Respond only from the supplied task. You have no tools and cannot delegate or change files."
        )),
        ChatMessage::user(task),
    ]);
    request.max_tokens = spec.model.max_tokens.min(768);
    request.temperature = spec.model.temperature;
    request.top_p = spec.model.top_p;
    request.top_k = Some(spec.model.top_k);
    request.min_p = spec.model.min_p;
    request.repeat_penalty = Some(spec.model.repeat_penalty);
    Ok(request)
}

#[async_trait::async_trait]
impl ConsultationSource for AppConsultations {
    async fn consult(
        &self,
        assistant: &str,
        task: &str,
        cancel: CancellationToken,
    ) -> Result<String, String> {
        let spec = specialist(&self.0, assistant)?;
        let request = isolated_request(&spec, task)?;
        self.0
            .scheduler
            .complete(Priority::Interactive, request, cancel)
            .await
            .map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    #[tokio::test]
    async fn specialist_request_is_isolated_and_bounded() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        let spec = specialist(&ctx, "reviewer").unwrap();
        let req = isolated_request(&spec, "Review this supplied excerpt.").unwrap();
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.messages[1].content, "Review this supplied excerpt.");
        assert!(req.messages[0].content.contains("no tools"));
        assert!(!req.messages[0].content.contains(dir.path().to_str().unwrap()));
        assert!(req.max_tokens <= 768);
        assert_eq!(req.chat_template_kwargs["enable_thinking"], false);
        assert!(isolated_request(&spec, &"x".repeat(6001)).is_err());
        assert!(isolated_request(&spec, " ").is_err());
    }

    #[tokio::test]
    async fn disabled_and_recursive_targets_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppState::for_tests(dir.path());
        assert!(specialist(&ctx, "agent").is_err());
        assert!(specialist(&ctx, "duo").is_err());
        ctx.registry.write().unwrap().apply_overrides(
            "planner",
            nexus_agent::registry::Overrides {
                enabled: false,
                sort: 0,
            },
        );
        assert!(specialist(&ctx, "planner").unwrap_err().contains("disabled"));
    }
}
