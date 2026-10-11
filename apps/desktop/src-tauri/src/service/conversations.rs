//! Conversation list, open, rename/pin/archive, delete, and Markdown export.

use nexus_core::chat::{Conversation, ConversationSummary, Message, MessageRole, MessageStatus};
use nexus_core::error::NexusError;
use nexus_core::ids::{AssistantId, ConversationId};
use nexus_store::ConversationPatch;
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::state::AppCtx;

const MAX_LIMIT: u32 = 200;

#[derive(Clone, Debug, Serialize, Type)]
pub struct ConversationWithMessages {
    pub conversation: Conversation,
    pub messages: Vec<Message>,
}

#[derive(Clone, Debug, Default, Deserialize, Type)]
pub struct ConversationUpdate {
    pub title: Option<String>,
    pub pinned: Option<bool>,
    pub assistant_id: Option<AssistantId>,
    pub archived: Option<bool>,
}

/// Pinned first, then newest. With `query`, matches titles and message text.
pub fn list(
    ctx: &AppCtx,
    query: Option<&str>,
    limit: u32,
    before: Option<i64>,
) -> Result<Vec<ConversationSummary>, NexusError> {
    let query = query.map(str::trim).filter(|q| !q.is_empty());
    Ok(ctx
        .store
        .list_conversations(query, limit.clamp(1, MAX_LIMIT), before)?)
}

pub fn get(ctx: &AppCtx, id: &ConversationId) -> Result<ConversationWithMessages, NexusError> {
    let conversation = ctx
        .store
        .get_conversation(id)?
        .ok_or_else(|| NexusError::NotFound("conversation".into()))?;
    let messages = ctx.store.list_messages(id)?;
    Ok(ConversationWithMessages {
        conversation,
        messages,
    })
}

pub fn update(ctx: &AppCtx, id: &ConversationId, u: ConversationUpdate) -> Result<(), NexusError> {
    if let Some(t) = &u.title
        && (t.trim().is_empty() || t.chars().count() > 120)
    {
        return Err(NexusError::InvalidInput(
            "a title must be 1 to 120 characters".into(),
        ));
    }
    ctx.store
        .get_conversation(id)?
        .ok_or_else(|| NexusError::NotFound("conversation".into()))?;
    let patch = ConversationPatch {
        title: u.title,
        pinned: u.pinned,
        assistant_id: u.assistant_id,
        archived: u.archived,
    };
    Ok(ctx.store.update_conversation(id, &patch)?)
}

/// Delete a conversation, stopping any answer still being written in it.
pub fn delete(ctx: &AppCtx, id: &ConversationId) -> Result<(), NexusError> {
    for m in ctx.store.list_messages(id)? {
        if m.role == MessageRole::Assistant {
            ctx.chats.cancel(&m.id);
        }
    }
    Ok(ctx.store.delete_conversation(id)?)
}

/// Render a conversation as Markdown (thoughts are left out; attachments are listed by name).
pub fn render_markdown(conv: &Conversation, messages: &[Message], assistant_name: &str) -> String {
    let mut out = format!("# {}\n\n", conv.title);
    for m in messages {
        let who = match m.role {
            MessageRole::User => "You",
            MessageRole::Assistant => assistant_name,
            _ => continue,
        };
        out.push_str(&format!("### {who}\n\n"));
        if !m.attachments.is_empty() {
            let names: Vec<_> = m.attachments.iter().map(|a| format!("`{}`", a.label)).collect();
            out.push_str(&format!("_Attached: {}_\n\n", names.join(", ")));
        }
        out.push_str(m.content.trim_end());
        out.push('\n');
        if m.status == MessageStatus::Stopped {
            out.push_str("\n_(stopped)_\n");
        }
        out.push('\n');
    }
    out
}

pub fn export_markdown(ctx: &AppCtx, id: &ConversationId) -> Result<String, NexusError> {
    let c = get(ctx, id)?;
    // M04 only has the built-in assistant; M06 resolves display names from the registry.
    let name = if c.conversation.assistant_id.as_str() == "nexus" {
        "Nexus"
    } else {
        c.conversation.assistant_id.as_str()
    };
    Ok(render_markdown(&c.conversation, &c.messages, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::chat::{AttachmentInput, AttachmentKind};
    use nexus_core::error::ErrorCode;
    use nexus_store::MessageFinish;

    fn seeded() -> (crate::state::Ctx, ConversationId) {
        let dir = tempfile::tempdir().unwrap();
        // Keep the temp dir alive for the test's duration by leaking it (tests are short-lived).
        let path = dir.keep();
        let ctx = crate::state::AppState::for_tests(&path);
        let conv = ctx
            .store
            .create_conversation(&AssistantId::from("nexus"))
            .unwrap();
        let att = AttachmentInput {
            kind: AttachmentKind::File,
            path: Some("/a/n.md".into()),
            label: "n.md".into(),
            text: Some("x".into()),
        };
        ctx.store
            .insert_user_message(&conv.id, "What is a bus?", &[att])
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        let a = ctx
            .store
            .insert_assistant_placeholder(&conv.id, &AssistantId::from("nexus"), None)
            .unwrap();
        let mut fin = MessageFinish::new(MessageStatus::Done, "A bus groups tracks.");
        fin.reasoning = Some("secret thoughts".into());
        ctx.store.finish_message(&a.id, &fin).unwrap();
        (ctx, conv.id)
    }

    #[tokio::test]
    async fn list_get_update_delete() {
        let (ctx, id) = seeded();
        assert_eq!(list(&ctx, None, 50, None).unwrap().len(), 1);
        assert_eq!(list(&ctx, Some("  bus "), 50, None).unwrap().len(), 1);
        assert!(list(&ctx, Some("zzzzqq"), 50, None).unwrap().is_empty());
        let got = get(&ctx, &id).unwrap();
        assert_eq!(got.messages.len(), 2);
        update(
            &ctx,
            &id,
            ConversationUpdate {
                title: Some("Buses".into()),
                pinned: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(get(&ctx, &id).unwrap().conversation.title, "Buses");
        assert_eq!(
            update(
                &ctx,
                &id,
                ConversationUpdate {
                    title: Some("  ".into()),
                    ..Default::default()
                }
            )
            .unwrap_err()
            .code(),
            ErrorCode::InvalidInput
        );
        assert_eq!(
            update(&ctx, &ConversationId::from("nope"), ConversationUpdate::default())
                .unwrap_err()
                .code(),
            ErrorCode::NotFound
        );
        delete(&ctx, &id).unwrap();
        assert_eq!(get(&ctx, &id).unwrap_err().code(), ErrorCode::NotFound);
    }

    #[tokio::test]
    async fn markdown_export_omits_thoughts_and_lists_attachments() {
        let (ctx, id) = seeded();
        let md = export_markdown(&ctx, &id).unwrap();
        assert!(md.starts_with("# New chat\n\n### You\n\n_Attached: `n.md`_\n\nWhat is a bus?\n"));
        assert!(md.contains("### Nexus\n\nA bus groups tracks."));
        assert!(!md.contains("secret thoughts"));
    }
}
