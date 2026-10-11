//! Change sets (ADR-005): the only way files change. Assistants and tools plan; the user previews and
//! applies. Paths are strings (canonical, absolute) so the types can cross IPC.

use serde::{Deserialize, Serialize};

use crate::fs::Access;
use crate::ids::ChangeSetId;

/// A byte-range replacement. Offsets are UTF-8 byte offsets into the file as it is on disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TextEdit {
    pub start: u32,
    pub end: u32,
    pub new_text: String,
}

/// One planned operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum FileOp {
    /// Edit an existing text file. `base_hash` is the blake3 hex of the file the plan was made from.
    Edit {
        path: String,
        base_hash: String,
        edits: Vec<TextEdit>,
    },
    /// Replace a whole file (editor save), or create it when it does not exist.
    Write {
        path: String,
        base_hash: Option<String>,
        contents: String,
    },
    /// Create a new file; fails if it exists.
    Create {
        path: String,
        contents: String,
    },
    Mkdir {
        path: String,
    },
    Move {
        from: String,
        to: String,
    },
    /// Move to the system Trash. Never a hard delete.
    Trash {
        path: String,
    },
}

impl FileOp {
    /// Short name for UI and audit.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Edit { .. } => "edit",
            Self::Write { .. } => "write",
            Self::Create { .. } => "create",
            Self::Mkdir { .. } => "mkdir",
            Self::Move { .. } => "move",
            Self::Trash { .. } => "trash",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ChangeSet {
    pub id: ChangeSetId,
    pub title: String,
    /// Assistant id, `"editor"`, or `"user"`.
    pub origin: String,
    pub ops: Vec<FileOp>,
    /// Unix milliseconds (UTC).
    pub created_at: i64,
}

/// What the Preview shows for one operation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct FilePreview {
    pub op_index: u32,
    pub kind: String,
    pub path: String,
    /// Destination of a move.
    pub to: Option<String>,
    pub needs: Access,
    pub size_before: Option<u64>,
    pub size_after: Option<u64>,
    pub insertions: u32,
    pub deletions: u32,
    /// Unified diff (3 lines of context) for text edits, writes, and creates.
    pub diff: Option<String>,
    /// Binary content: no diff, sizes only.
    pub binary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// The file changed since the plan was made.
    HashMismatch,
    /// The file (or its parent) does not exist.
    Missing,
    /// The target already exists.
    Exists,
    /// A protected location.
    Denied,
    /// Malformed: overlapping edits, bad offsets, the same path twice, and so on.
    Invalid,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Conflict {
    pub op_index: u32,
    pub path: String,
    pub kind: ConflictKind,
    pub message: String,
}

/// Access the plan needs but no grant gives. The UI asks for permission before Apply is enabled.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct MissingGrant {
    pub op_index: u32,
    pub path: String,
    pub needed: Access,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Totals {
    pub files: u32,
    pub insertions: u32,
    pub deletions: u32,
    pub creates: u32,
    pub moves: u32,
    pub trashes: u32,
    pub bytes: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ChangePreview {
    pub files: Vec<FilePreview>,
    pub totals: Totals,
    pub conflicts: Vec<Conflict>,
    pub missing_grants: Vec<MissingGrant>,
}

impl ChangePreview {
    /// Safe to apply: nothing conflicts and every needed grant exists.
    pub fn can_apply(&self) -> bool {
        self.conflicts.is_empty() && self.missing_grants.is_empty() && !self.files.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_serialize_with_an_op_tag() {
        let op = FileOp::Move {
            from: "/a".into(),
            to: "/b".into(),
        };
        let v = serde_json::to_value(&op).unwrap();
        assert_eq!(v, serde_json::json!({"op": "move", "from": "/a", "to": "/b"}));
        assert_eq!(serde_json::from_value::<FileOp>(v).unwrap(), op);
        assert_eq!(op.kind(), "move");
    }

    #[test]
    fn can_apply_needs_files_and_no_problems() {
        let mut p = ChangePreview::default();
        assert!(!p.can_apply());
        p.files.push(FilePreview {
            op_index: 0,
            kind: "create".into(),
            path: "/a".into(),
            to: None,
            needs: Access::Create,
            size_before: None,
            size_after: Some(1),
            insertions: 1,
            deletions: 0,
            diff: None,
            binary: false,
        });
        assert!(p.can_apply());
        p.missing_grants.push(MissingGrant {
            op_index: 0,
            path: "/a".into(),
            needed: Access::Create,
        });
        assert!(!p.can_apply());
    }
}
