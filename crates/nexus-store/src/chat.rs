//! Conversations, messages, and attachments.

use nexus_core::chat::{
    Attachment, AttachmentInput, AttachmentKind, Conversation, ConversationSummary, Message, MessageRole,
    MessageStatus,
};
use nexus_core::ids::{AssistantId, ConversationId, MessageId};
use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Store, StoreError, now_ms};

/// Fields of a conversation that can change; `None` leaves a field alone.
#[derive(Clone, Debug, Default)]
pub struct ConversationPatch {
    pub title: Option<String>,
    pub pinned: Option<bool>,
    pub assistant_id: Option<AssistantId>,
    pub archived: Option<bool>,
}

/// Final state of an assistant message.
#[derive(Clone, Debug)]
pub struct MessageFinish {
    pub status: MessageStatus,
    pub content: String,
    pub reasoning: Option<String>,
    pub error: Option<String>,
    pub model_id: Option<String>,
    pub tool_trace: Option<String>,
    pub tokens_prompt: Option<u32>,
    pub tokens_out: Option<u32>,
    pub tps: Option<f32>,
    pub ttft_ms: Option<u32>,
}

impl MessageFinish {
    /// A finish with just a status and text.
    pub fn new(status: MessageStatus, content: impl Into<String>) -> Self {
        Self {
            status,
            content: content.into(),
            reasoning: None,
            error: None,
            model_id: None,
            tool_trace: None,
            tokens_prompt: None,
            tokens_out: None,
            tps: None,
            ttft_ms: None,
        }
    }
}

fn conversation_row(r: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: ConversationId(r.get("id")?),
        title: r.get("title")?,
        assistant_id: AssistantId(r.get("assistant_id")?),
        pinned: r.get::<_, i64>("pinned")? != 0,
        summary: r.get("summary")?,
        summary_upto: r.get::<_, Option<String>>("summary_upto")?.map(MessageId),
        created_at: r.get("created_at")?,
        updated_at: r.get("updated_at")?,
        archived: r.get::<_, i64>("archived")? != 0,
    })
}

fn message_row(r: &Row<'_>) -> rusqlite::Result<Message> {
    let role: String = r.get("role")?;
    let status: String = r.get("status")?;
    Ok(Message {
        id: MessageId(r.get("id")?),
        conversation_id: ConversationId(r.get("conversation_id")?),
        role: MessageRole::parse(&role).unwrap_or(MessageRole::Event),
        content: r.get("content")?,
        reasoning: r.get("reasoning")?,
        assistant_id: r.get::<_, Option<String>>("assistant_id")?.map(AssistantId),
        model_id: r.get("model_id")?,
        tool_trace: r.get("tool_trace")?,
        status: MessageStatus::parse(&status).unwrap_or(MessageStatus::Done),
        error: r.get("error")?,
        tokens_prompt: r.get("tokens_prompt")?,
        tokens_out: r.get("tokens_out")?,
        tps: r.get::<_, Option<f64>>("tps")?.map(|v| v as f32),
        ttft_ms: r.get("ttft_ms")?,
        created_at: r.get("created_at")?,
        attachments: Vec::new(),
    })
}

fn attachments_for(c: &Connection, message_id: &str) -> Result<Vec<Attachment>, StoreError> {
    let mut stmt = c.prepare("SELECT * FROM attachments WHERE message_id = ?1 ORDER BY rowid")?;
    let rows = stmt
        .query_map([message_id], |r| {
            let kind: String = r.get("kind")?;
            Ok(Attachment {
                id: r.get("id")?,
                kind: AttachmentKind::parse(&kind).unwrap_or(AttachmentKind::File),
                path: r.get("path")?,
                label: r.get("label")?,
                tokens: r.get("tokens")?,
                snapshot: r.get("snapshot")?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn load_message(c: &Connection, id: &str) -> Result<Option<Message>, StoreError> {
    let msg = c
        .query_row("SELECT * FROM messages WHERE id = ?1", [id], message_row)
        .optional()?;
    match msg {
        Some(mut m) => {
            m.attachments = attachments_for(c, id)?;
            Ok(Some(m))
        }
        None => Ok(None),
    }
}

/// Turn free text into a safe FTS5 prefix query: each word quoted, the last one a prefix.
pub fn fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\""))
        .collect();
    let (last, rest) = words.split_last()?;
    let mut q = rest.join(" ");
    if !q.is_empty() {
        q.push(' ');
    }
    q.push_str(last);
    q.push('*');
    Some(q)
}

impl Store {
    // ---- conversations --------------------------------------------------------------------

    pub fn create_conversation(&self, assistant_id: &AssistantId) -> Result<Conversation, StoreError> {
        let now = now_ms();
        let conv = Conversation {
            id: ConversationId::new(),
            title: "New chat".into(),
            assistant_id: assistant_id.clone(),
            pinned: false,
            summary: None,
            summary_upto: None,
            created_at: now,
            updated_at: now,
            archived: false,
        };
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO conversations (id, title, assistant_id, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)",
                params![conv.id.as_str(), conv.title, conv.assistant_id.as_str(), now],
            )?;
            Ok(())
        })?;
        Ok(conv)
    }

    pub fn get_conversation(&self, id: &ConversationId) -> Result<Option<Conversation>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT * FROM conversations WHERE id = ?1",
                [id.as_str()],
                conversation_row,
            )
            .optional()?)
        })
    }

    /// The existing conversation, or a new one when `id` is `None`. An unknown id is an error.
    pub fn get_or_create_conversation(
        &self,
        id: Option<&ConversationId>,
        assistant_id: &AssistantId,
    ) -> Result<Conversation, StoreError> {
        match id {
            Some(id) => self
                .get_conversation(id)?
                .ok_or(StoreError::NotFound("conversation")),
            None => self.create_conversation(assistant_id),
        }
    }

    /// Pinned first, then most recently updated. With `query`, matches titles and message text (FTS5
    /// prefix search) and returns a snippet. `before` pages by `updated_at`.
    pub fn list_conversations(
        &self,
        query: Option<&str>,
        limit: u32,
        before: Option<i64>,
    ) -> Result<Vec<ConversationSummary>, StoreError> {
        let before = before.unwrap_or(i64::MAX);
        let fts = query.and_then(fts_query);
        self.with_conn(|c| {
            let summary = |r: &Row<'_>| -> rusqlite::Result<ConversationSummary> {
                Ok(ConversationSummary {
                    id: ConversationId(r.get(0)?),
                    title: r.get(1)?,
                    assistant_id: AssistantId(r.get(2)?),
                    pinned: r.get::<_, i64>(3)? != 0,
                    updated_at: r.get(4)?,
                    snippet: r.get(5)?,
                })
            };
            let Some(fts) = fts else {
                let mut stmt = c.prepare(
                    "SELECT id, title, assistant_id, pinned, updated_at, NULL FROM conversations
                     WHERE archived = 0 AND updated_at < ?1 ORDER BY pinned DESC, updated_at DESC LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(params![before, limit], summary)?
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(rows);
            };
            let like = format!("%{}%", query.unwrap_or_default().replace(['%', '_'], ""));
            let mut stmt = c.prepare(
                "SELECT id, title, assistant_id, pinned, updated_at, snippet FROM (
                   SELECT c.id, c.title, c.assistant_id, c.pinned, c.updated_at,
                          snippet(messages_fts, 0, '', '', '…', 12) AS snippet
                   FROM messages_fts
                   JOIN messages m ON m.rowid = messages_fts.rowid
                   JOIN conversations c ON c.id = m.conversation_id
                   WHERE messages_fts MATCH ?1 AND c.archived = 0 AND c.updated_at < ?3
                   UNION ALL
                   SELECT id, title, assistant_id, pinned, updated_at, NULL FROM conversations
                   WHERE archived = 0 AND title LIKE ?2 AND updated_at < ?3
                 ) GROUP BY id ORDER BY pinned DESC, updated_at DESC LIMIT ?4",
            )?;
            let rows = stmt
                .query_map(params![fts, like, before, limit], summary)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    pub fn update_conversation(
        &self,
        id: &ConversationId,
        patch: &ConversationPatch,
    ) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let tx = c.transaction()?;
            if let Some(t) = &patch.title {
                tx.execute(
                    "UPDATE conversations SET title = ?2 WHERE id = ?1",
                    params![id.as_str(), t.trim()],
                )?;
            }
            if let Some(p) = patch.pinned {
                tx.execute(
                    "UPDATE conversations SET pinned = ?2 WHERE id = ?1",
                    params![id.as_str(), p],
                )?;
            }
            if let Some(a) = &patch.assistant_id {
                tx.execute(
                    "UPDATE conversations SET assistant_id = ?2 WHERE id = ?1",
                    params![id.as_str(), a.as_str()],
                )?;
            }
            if let Some(a) = patch.archived {
                tx.execute(
                    "UPDATE conversations SET archived = ?2 WHERE id = ?1",
                    params![id.as_str(), a],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// Store the rolling summary of history up to (and including) `upto`.
    pub fn set_conversation_summary(
        &self,
        id: &ConversationId,
        summary: &str,
        upto: &MessageId,
    ) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE conversations SET summary = ?2, summary_upto = ?3 WHERE id = ?1",
                params![id.as_str(), summary, upto.as_str()],
            )?;
            Ok(())
        })
    }

    /// Delete a conversation and (by cascade) its messages and attachments.
    pub fn delete_conversation(&self, id: &ConversationId) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute("DELETE FROM conversations WHERE id = ?1", [id.as_str()])?;
            Ok(())
        })
    }

    // ---- messages ---------------------------------------------------------------------------

    /// Insert the user's message (with its attachments) and bump the conversation.
    pub fn insert_user_message(
        &self,
        conversation_id: &ConversationId,
        text: &str,
        attachments: &[AttachmentInput],
    ) -> Result<Message, StoreError> {
        let id = MessageId::new();
        let now = now_ms();
        self.with_conn(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO messages (id, conversation_id, role, content, status, created_at) VALUES (?1, ?2, 'user', ?3, 'done', ?4)",
                params![id.as_str(), conversation_id.as_str(), text, now],
            )?;
            for a in attachments {
                tx.execute(
                    "INSERT INTO attachments (id, message_id, kind, path, label, snapshot) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![MessageId::new().as_str(), id.as_str(), a.kind.as_str(), a.path, a.label, a.text],
                )?;
            }
            tx.execute("UPDATE conversations SET updated_at = ?2 WHERE id = ?1", params![conversation_id.as_str(), now])?;
            tx.commit()?;
            load_message(c, id.as_str())?.ok_or(StoreError::NotFound("message"))
        })
    }

    /// Insert the (empty, `streaming`) assistant message that the turn will fill.
    pub fn insert_assistant_placeholder(
        &self,
        conversation_id: &ConversationId,
        assistant_id: &AssistantId,
        model_id: Option<&str>,
    ) -> Result<Message, StoreError> {
        let id = MessageId::new();
        let now = now_ms();
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO messages (id, conversation_id, role, content, assistant_id, model_id, status, created_at)
                 VALUES (?1, ?2, 'assistant', '', ?3, ?4, 'streaming', ?5)",
                params![id.as_str(), conversation_id.as_str(), assistant_id.as_str(), model_id, now],
            )?;
            load_message(c, id.as_str())?.ok_or(StoreError::NotFound("message"))
        })
    }

    /// Persist partial output while streaming (crash safety).
    pub fn update_message_stream(
        &self,
        id: &MessageId,
        content: &str,
        reasoning: Option<&str>,
    ) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE messages SET content = ?2, reasoning = ?3 WHERE id = ?1",
                params![id.as_str(), content, reasoning],
            )?;
            Ok(())
        })
    }

    /// Write the final state of an assistant message and bump the conversation.
    pub fn finish_message(&self, id: &MessageId, f: &MessageFinish) -> Result<(), StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "UPDATE messages SET status = ?2, content = ?3, reasoning = ?4, error = ?5,
                   model_id = COALESCE(?6, model_id), tool_trace = ?7, tokens_prompt = ?8, tokens_out = ?9,
                   tps = ?10, ttft_ms = ?11 WHERE id = ?1",
                params![
                    id.as_str(),
                    f.status.as_str(),
                    f.content,
                    f.reasoning,
                    f.error,
                    f.model_id,
                    f.tool_trace,
                    f.tokens_prompt,
                    f.tokens_out,
                    f.tps,
                    f.ttft_ms
                ],
            )?;
            tx.execute(
                "UPDATE conversations SET updated_at = ?2 WHERE id = (SELECT conversation_id FROM messages WHERE id = ?1)",
                params![id.as_str(), now],
            )?;
            tx.commit()?;
            Ok(())
        })
    }

    pub fn get_message(&self, id: &MessageId) -> Result<Option<Message>, StoreError> {
        self.with_conn(|c| load_message(c, id.as_str()))
    }

    /// All messages of a conversation, oldest first, with attachments.
    pub fn list_messages(&self, conversation_id: &ConversationId) -> Result<Vec<Message>, StoreError> {
        self.with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT * FROM messages WHERE conversation_id = ?1 ORDER BY created_at, rowid")?;
            let mut msgs = stmt
                .query_map([conversation_id.as_str()], message_row)?
                .collect::<Result<Vec<_>, _>>()?;
            for m in &mut msgs {
                m.attachments = attachments_for(c, m.id.as_str())?;
            }
            Ok(msgs)
        })
    }

    /// Replace the text of a message (edit and resend).
    pub fn set_message_content(&self, id: &MessageId, content: &str) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE messages SET content = ?2 WHERE id = ?1",
                params![id.as_str(), content],
            )?;
            Ok(())
        })
    }

    /// Delete `id` and every message after it in the same conversation (regenerate, edit-resend).
    /// Returns how many were deleted.
    pub fn delete_messages_from(&self, id: &MessageId) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "DELETE FROM messages WHERE conversation_id = (SELECT conversation_id FROM messages WHERE id = ?1)
                   AND (created_at > (SELECT created_at FROM messages WHERE id = ?1)
                        OR (created_at = (SELECT created_at FROM messages WHERE id = ?1)
                            AND rowid >= (SELECT rowid FROM messages WHERE id = ?1)))",
                [id.as_str()],
            )?;
            Ok(n)
        })
    }

    /// Delete every message after `id` in its conversation (edit-resend keeps `id` itself).
    pub fn delete_messages_after(&self, id: &MessageId) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "DELETE FROM messages WHERE conversation_id = (SELECT conversation_id FROM messages WHERE id = ?1)
                   AND (created_at > (SELECT created_at FROM messages WHERE id = ?1)
                        OR (created_at = (SELECT created_at FROM messages WHERE id = ?1)
                            AND rowid > (SELECT rowid FROM messages WHERE id = ?1)))",
                [id.as_str()],
            )?;
            Ok(n)
        })
    }

    /// Record the token cost of an attachment.
    pub fn set_attachment_tokens(&self, attachment_id: &str, tokens: u32) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE attachments SET tokens = ?2 WHERE id = ?1",
                params![attachment_id, tokens],
            )?;
            Ok(())
        })
    }

    /// Startup recovery: a message still `streaming` belongs to a turn that died with the app.
    pub fn recover_interrupted_messages(&self) -> Result<usize, StoreError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "UPDATE messages SET status = 'stopped' WHERE status = 'streaming'",
                [],
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (Store, Conversation) {
        let s = Store::open_in_memory().unwrap();
        let c = s.create_conversation(&AssistantId::from("nexus")).unwrap();
        (s, c)
    }

    fn pause() {
        std::thread::sleep(std::time::Duration::from_millis(3));
    }

    #[test]
    fn conversation_crud_and_ordering() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_conversation(&AssistantId::from("nexus")).unwrap();
        pause();
        let b = s.create_conversation(&AssistantId::from("nexus")).unwrap();
        let list = s.list_conversations(None, 10, None).unwrap();
        assert_eq!(list.iter().map(|c| &c.id).collect::<Vec<_>>(), [&b.id, &a.id]);
        s.update_conversation(
            &a.id,
            &ConversationPatch {
                pinned: Some(true),
                title: Some(" Pinned one ".into()),
                ..Default::default()
            },
        )
        .unwrap();
        let list = s.list_conversations(None, 10, None).unwrap();
        assert_eq!(list[0].id, a.id, "pinned first");
        assert_eq!(list[0].title, "Pinned one");
        s.update_conversation(
            &b.id,
            &ConversationPatch {
                archived: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(s.list_conversations(None, 10, None).unwrap().len(), 1);
        s.delete_conversation(&a.id).unwrap();
        assert!(s.get_conversation(&a.id).unwrap().is_none());
    }

    #[test]
    fn get_or_create() {
        let (s, c) = store();
        assert_eq!(
            s.get_or_create_conversation(Some(&c.id), &AssistantId::from("x"))
                .unwrap()
                .id,
            c.id
        );
        let fresh = s
            .get_or_create_conversation(None, &AssistantId::from("nexus"))
            .unwrap();
        assert_ne!(fresh.id, c.id);
        assert!(matches!(
            s.get_or_create_conversation(Some(&ConversationId::from("nope")), &AssistantId::from("x")),
            Err(StoreError::NotFound(_))
        ));
    }

    #[test]
    fn message_lifecycle_with_attachments() {
        let (s, c) = store();
        let att = AttachmentInput {
            kind: AttachmentKind::File,
            path: Some("/a/b.txt".into()),
            label: "b.txt".into(),
            text: Some("hello".into()),
        };
        let user = s.insert_user_message(&c.id, "Summarize this", &[att]).unwrap();
        assert_eq!(user.attachments.len(), 1);
        assert_eq!(user.attachments[0].snapshot.as_deref(), Some("hello"));
        pause();
        let asst = s
            .insert_assistant_placeholder(&c.id, &AssistantId::from("nexus"), Some("m.gguf"))
            .unwrap();
        assert_eq!(asst.status, MessageStatus::Streaming);
        s.update_message_stream(&asst.id, "partial", Some("thinking"))
            .unwrap();
        assert_eq!(s.get_message(&asst.id).unwrap().unwrap().content, "partial");
        let mut fin = MessageFinish::new(MessageStatus::Done, "final answer");
        fin.reasoning = Some("thoughts".into());
        fin.tokens_out = Some(12);
        fin.tps = Some(55.5);
        fin.ttft_ms = Some(300);
        s.finish_message(&asst.id, &fin).unwrap();
        let msgs = s.list_messages(&c.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, MessageRole::User);
        assert_eq!(msgs[1].content, "final answer");
        assert_eq!(msgs[1].reasoning.as_deref(), Some("thoughts"));
        assert_eq!((msgs[1].tokens_out, msgs[1].ttft_ms), (Some(12), Some(300)));
        assert_eq!(
            msgs[1].model_id.as_deref(),
            Some("m.gguf"),
            "finish keeps the model id when none is given"
        );
        s.set_attachment_tokens(&msgs[0].attachments[0].id, 42).unwrap();
        assert_eq!(s.list_messages(&c.id).unwrap()[0].attachments[0].tokens, Some(42));
    }

    #[test]
    fn delete_from_removes_that_message_and_later_ones() {
        let (s, c) = store();
        let u1 = s.insert_user_message(&c.id, "one", &[]).unwrap();
        pause();
        let a1 = s
            .insert_assistant_placeholder(&c.id, &AssistantId::from("nexus"), None)
            .unwrap();
        pause();
        let u2 = s.insert_user_message(&c.id, "two", &[]).unwrap();
        pause();
        s.insert_assistant_placeholder(&c.id, &AssistantId::from("nexus"), None)
            .unwrap();
        assert_eq!(s.delete_messages_from(&u2.id).unwrap(), 2);
        let ids: Vec<_> = s
            .list_messages(&c.id)
            .unwrap()
            .into_iter()
            .map(|m| m.id)
            .collect();
        assert_eq!(ids, [u1.id.clone(), a1.id.clone()]);
        assert_eq!(s.delete_messages_from(&a1.id).unwrap(), 1);
    }

    #[test]
    fn delete_after_keeps_the_anchor() {
        let (s, c) = store();
        let u1 = s.insert_user_message(&c.id, "one", &[]).unwrap();
        pause();
        s.insert_assistant_placeholder(&c.id, &AssistantId::from("nexus"), None)
            .unwrap();
        pause();
        s.insert_user_message(&c.id, "two", &[]).unwrap();
        assert_eq!(s.delete_messages_after(&u1.id).unwrap(), 2);
        let left = s.list_messages(&c.id).unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].id, u1.id);
    }

    #[test]
    fn cascade_delete_removes_messages_attachments_and_fts_rows() {
        let (s, c) = store();
        let att = AttachmentInput {
            kind: AttachmentKind::Text,
            path: None,
            label: "t".into(),
            text: Some("x".into()),
        };
        s.insert_user_message(&c.id, "zebra crossing", &[att]).unwrap();
        assert_eq!(s.list_conversations(Some("zebra"), 10, None).unwrap().len(), 1);
        s.delete_conversation(&c.id).unwrap();
        s.with_conn(|conn| {
            let n: i64 = conn.query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))?;
            assert_eq!(n, 0);
            Ok(())
        })
        .unwrap();
        assert!(s.list_conversations(Some("zebra"), 10, None).unwrap().is_empty());
    }

    #[test]
    fn full_text_search_finds_messages_and_titles() {
        let s = Store::open_in_memory().unwrap();
        let a = s.create_conversation(&AssistantId::from("nexus")).unwrap();
        let b = s.create_conversation(&AssistantId::from("nexus")).unwrap();
        s.insert_user_message(&a.id, "How do I rotate a quaternion in Unity?", &[])
            .unwrap();
        s.insert_user_message(&b.id, "Plan my week", &[]).unwrap();
        s.update_conversation(
            &b.id,
            &ConversationPatch {
                title: Some("Weekly planning".into()),
                ..Default::default()
            },
        )
        .unwrap();

        let hits = s.list_conversations(Some("quatern"), 10, None).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
        assert!(
            hits[0]
                .snippet
                .as_deref()
                .unwrap_or_default()
                .contains("quaternion")
        );
        let by_title = s.list_conversations(Some("weekly"), 10, None).unwrap();
        assert_eq!(by_title.len(), 1);
        assert_eq!(by_title[0].id, b.id);
        assert!(
            s.list_conversations(Some("nothing-matches-this"), 10, None)
                .unwrap()
                .is_empty()
        );
        // Hostile input must not break the FTS parser.
        assert!(s.list_conversations(Some("\"(*) AND OR NEAR"), 10, None).is_ok());
    }

    #[test]
    fn edits_are_reflected_in_search() {
        let (s, c) = store();
        let m = s.insert_user_message(&c.id, "alpha", &[]).unwrap();
        s.set_message_content(&m.id, "bravo").unwrap();
        assert!(s.list_conversations(Some("alpha"), 10, None).unwrap().is_empty());
        assert_eq!(s.list_conversations(Some("bravo"), 10, None).unwrap().len(), 1);
    }

    #[test]
    fn summary_and_recovery() {
        let (s, c) = store();
        let m = s.insert_user_message(&c.id, "hi", &[]).unwrap();
        s.set_conversation_summary(&c.id, "They said hi.", &m.id).unwrap();
        let got = s.get_conversation(&c.id).unwrap().unwrap();
        assert_eq!(
            (got.summary.as_deref(), got.summary_upto.as_ref()),
            (Some("They said hi."), Some(&m.id))
        );
        let a = s
            .insert_assistant_placeholder(&c.id, &AssistantId::from("nexus"), None)
            .unwrap();
        assert_eq!(s.recover_interrupted_messages().unwrap(), 1);
        assert_eq!(
            s.get_message(&a.id).unwrap().unwrap().status,
            MessageStatus::Stopped
        );
    }

    #[test]
    fn paging_by_updated_at() {
        let s = Store::open_in_memory().unwrap();
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(s.create_conversation(&AssistantId::from("nexus")).unwrap());
            pause();
        }
        let first = s.list_conversations(None, 2, None).unwrap();
        assert_eq!(first.len(), 2);
        let rest = s.list_conversations(None, 2, Some(first[1].updated_at)).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].id, ids[0].id);
    }

    #[test]
    fn fts_query_quotes_words() {
        assert_eq!(fts_query("hello wor"), Some("\"hello\" \"wor\"*".into()));
        assert_eq!(fts_query("  "), None);
        assert_eq!(fts_query("a\"b"), Some("\"a\" \"b\"*".into()));
    }
}
