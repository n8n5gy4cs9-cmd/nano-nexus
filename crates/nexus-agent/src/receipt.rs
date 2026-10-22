//! Factual confirmations for completed tool actions. Never ask a model to invent their status.

use crate::router::{ToolRun, file_change_requested, is_file_change, simple_consultation};

/// Return actual action receipts for simple requests; explanation and open-ended answers still
/// use inference. Tool traces and change cards remain the authority for Apply and Undo.
pub fn tool_receipt(request: &str, runs: &[ToolRun]) -> Option<String> {
    if simple_consultation(request) && runs.len() == 1 && runs[0].tool == "ask_assistant" {
        let run = &runs[0];
        if !run.ok {
            return Some(format!("Specialist consultation failed: {}", run.llm_text));
        }
        if let Some((_, body)) = run.llm_text.split_once('\n')
            && let Some(body) = body.strip_suffix("\n</tool_result>")
        {
            return Some(body.to_owned());
        }
    }
    if runs.is_empty()
        || request
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| word.eq_ignore_ascii_case("explain") || word.eq_ignore_ascii_case("why"))
    {
        return None;
    }
    let changes: Vec<&ToolRun> = runs.iter().filter(|run| is_file_change(&run.tool)).collect();
    let allowed: Vec<String> = changes.iter().map(|run| run.tool.clone()).collect();
    if !changes.is_empty() && file_change_requested(request, &allowed) {
        let mut lines = Vec::new();
        let mut pending = false;
        let mut applied = false;
        for run in changes {
            let args: serde_json::Value = serde_json::from_str(&run.args_json).unwrap_or_default();
            let path = args["path"].as_str().unwrap_or("");
            let destination = args["to"].as_str();
            let mut line = format!("- {}", run.summary);
            if !path.is_empty() {
                line.push_str(&format!(" — {}", path.replace(['\r', '\n'], " ")));
            }
            if let Some(to) = destination {
                line.push_str(&format!(" → {}", to.replace(['\r', '\n'], " ")));
            }
            lines.push(line);
            pending |= run.ok && run.llm_text.contains("NOT written yet");
            applied |= run.ok && run.llm_text.starts_with("DONE:");
        }
        if pending {
            lines.push("Press Apply on the change card to write the proposed changes.".into());
        }
        if applied {
            lines.push("You can undo applied changes in Changes.".into());
        }
        return Some(lines.join("\n\n"));
    }
    if runs
        .iter()
        .all(|run| run.ok && matches!(run.tool.as_str(), "calculate" | "now"))
    {
        return Some(
            runs.iter()
                .map(|run| run.summary.as_str())
                .collect::<Vec<_>>()
                .join("\n\n"),
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn change(summary: &str, text: &str, ok: bool) -> ToolRun {
        ToolRun {
            tool: "write_file".into(),
            args_json: r#"{"path":"/work/notes.md"}"#.into(),
            ok,
            summary: summary.into(),
            llm_text: text.into(),
        }
    }

    #[test]
    fn receipts_preserve_applied_pending_and_failed_states() {
        let applied = tool_receipt(
            "Create /work/notes.md",
            &[change("Applied: Create notes.md", "DONE: written", true)],
        )
        .unwrap();
        assert!(
            applied.contains("Applied:") && applied.contains("/work/notes.md") && applied.contains("undo")
        );
        assert!(!applied.contains("Press Apply"));
        let pending = tool_receipt(
            "Save that as notes.md",
            &[change("Create notes.md (press Apply)", "NOT written yet", true)],
        )
        .unwrap();
        assert!(pending.contains("Press Apply"));
        assert!(!pending.contains("Applied:") && !pending.contains("undo"));
        let failed = tool_receipt(
            "Create notes.md",
            &[change(
                "Create notes.md (not applied: denied)",
                "NOT written: denied",
                true,
            )],
        )
        .unwrap();
        assert!(failed.contains("not applied: denied") && !failed.contains("undo"));
    }

    #[test]
    fn explicit_consultation_receipts_preserve_the_child_answer() {
        let run = ToolRun { tool: "ask_assistant".into(), args_json: "{}".into(), ok: true,
            summary: "Consulted reviewer".into(),
            llm_text: "<tool_result name=\"ask_assistant\" assistant=\"reviewer\">\nEmpty input divides by zero.\n</tool_result>".into() };
        assert_eq!(
            tool_receipt("Ask Reviewer to review this code", &[run]).unwrap(),
            "Empty input divides by zero."
        );
    }

    #[test]
    fn receipts_use_facts_and_keep_explanatory_answers_generative() {
        let facts = vec![ToolRun {
            tool: "calculate".into(),
            args_json: "{}".into(),
            ok: true,
            summary: "18/100*2450 = 441".into(),
            llm_text: "<tool_result>internal</tool_result>".into(),
        }];
        assert_eq!(
            tool_receipt("What is 18% of 2450?", &facts).unwrap(),
            "18/100*2450 = 441"
        );
        assert!(tool_receipt("Explain why 18% of 2450 is 441", &facts).is_none());
        assert!(
            tool_receipt(
                "Explain how to write notes.md",
                &[change("Applied: Create notes.md", "DONE:", true)]
            )
            .is_none()
        );
        assert!(tool_receipt("Hello", &[]).is_none());
    }
}
