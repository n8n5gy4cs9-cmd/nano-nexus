//! Background chat jobs: conversation titles and rolling history summaries. Both run at Background
//! priority so they never delay a chat answer, and both have deterministic fallbacks.

use nexus_llm::{
    ChatMessage, ChatRequest, LlmError, Priority, Scheduler, Structured, StructuredSpec, Validate,
    ValidationCtx, chat_json,
};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::internal::{HISTORY_SUMMARY_PROMPT, TITLE_PROMPT, TITLE_SCHEMA, render_pair};

const TITLE_TEXT_CHARS: usize = 600;

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// First six words of the user's message, Title Cased (the fallback title).
pub fn fallback_title(user_text: &str) -> String {
    let words: Vec<String> = user_text
        .split_whitespace()
        .take(6)
        .map(|w| {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric());
            let mut cs = w.chars();
            match cs.next() {
                Some(f) => f.to_uppercase().collect::<String>() + &cs.as_str().to_lowercase(),
                None => String::new(),
            }
        })
        .filter(|w| !w.is_empty())
        .collect();
    if words.is_empty() {
        "New Chat".to_owned()
    } else {
        words.join(" ")
    }
}

#[derive(Debug, Deserialize)]
struct TitleOut {
    title: String,
}

impl Validate for TitleOut {
    fn validate(&self, _: &ValidationCtx) -> Result<(), Vec<String>> {
        let t = self.title.trim();
        let n = t.chars().count();
        let mut errs = Vec::new();
        if !(3..=48).contains(&n) {
            errs.push(format!("the title must be 3 to 48 characters, got {n}"));
        }
        if t.contains('\n') {
            errs.push("the title must be one line".to_owned());
        }
        if errs.is_empty() { Ok(()) } else { Err(errs) }
    }
}

#[derive(Serialize)]
struct TitleVars<'a> {
    user_text: &'a str,
    assistant_text: &'a str,
}

/// A short title for a conversation. Any failure (model busy, invalid JSON twice, runtime down)
/// yields [`fallback_title`], so a conversation always gets a title.
pub async fn generate_title(
    sched: &Scheduler,
    user_text: &str,
    assistant_text: &str,
    cancel: CancellationToken,
) -> String {
    let fallback = fallback_title(user_text);
    let Ok(schema) = serde_json::from_str::<serde_json::Value>(TITLE_SCHEMA) else {
        return fallback;
    };
    let vars = TitleVars {
        user_text: &clip(user_text, TITLE_TEXT_CHARS),
        assistant_text: &clip(assistant_text, TITLE_TEXT_CHARS),
    };
    let Ok(pair) = render_pair(TITLE_PROMPT, &vars) else {
        return fallback;
    };
    let spec = StructuredSpec {
        name: "title",
        schema: &schema,
        temperature: 0.3,
        max_tokens: 40,
    };
    let msgs = vec![ChatMessage::system(pair.system), ChatMessage::user(pair.user)];
    match chat_json::<TitleOut>(
        sched,
        Priority::Background,
        msgs,
        spec,
        &ValidationCtx::default(),
        cancel,
    )
    .await
    {
        Ok(Structured::Valid(t)) => t.title.trim().trim_matches('"').to_owned(),
        Ok(Structured::Fallback { .. }) | Err(_) => fallback,
    }
}

/// One past message for summarization.
#[derive(Clone, Debug, Serialize)]
pub struct SummaryTurn {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct SummaryVars<'a> {
    previous_summary: &'a str,
    turns: &'a [SummaryTurn],
}

/// Fold `turns` (the messages that no longer fit) into the rolling summary. Errors are returned so the
/// caller can simply try again after a later turn.
pub async fn summarize_history(
    sched: &Scheduler,
    previous_summary: Option<&str>,
    turns: &[SummaryTurn],
    cancel: CancellationToken,
) -> Result<String, LlmError> {
    let vars = SummaryVars {
        previous_summary: previous_summary.unwrap_or(""),
        turns,
    };
    let pair = render_pair(HISTORY_SUMMARY_PROMPT, &vars).map_err(|e| LlmError::Protocol(e.to_string()))?;
    let req = ChatRequest::new(vec![
        ChatMessage::system(pair.system),
        ChatMessage::user(pair.user),
    ])
    .temperature(0.2)
    .max_tokens(220);
    let text = sched.complete_wait(Priority::Background, req, cancel).await?;
    let text = text.trim().to_owned();
    if text.is_empty() {
        Err(LlmError::Protocol("the model returned an empty summary".into()))
    } else {
        Ok(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_title_is_six_title_cased_words() {
        assert_eq!(
            fallback_title("how do i rotate a quaternion in godot please"),
            "How Do I Rotate A Quaternion"
        );
        assert_eq!(fallback_title("  \"quoted\" WORDS!! "), "Quoted Words");
        assert_eq!(fallback_title("   "), "New Chat");
        assert_eq!(fallback_title("héllo wörld"), "Héllo Wörld");
    }

    #[test]
    fn title_validation() {
        let ok = TitleOut {
            title: "Godot Signals".into(),
        };
        assert!(ok.validate(&ValidationCtx::default()).is_ok());
        assert!(
            TitleOut { title: "no".into() }
                .validate(&ValidationCtx::default())
                .is_err()
        );
        assert!(
            TitleOut {
                title: "two\nlines".into()
            }
            .validate(&ValidationCtx::default())
            .is_err()
        );
        assert!(
            TitleOut {
                title: "x".repeat(60)
            }
            .validate(&ValidationCtx::default())
            .is_err()
        );
    }
}
