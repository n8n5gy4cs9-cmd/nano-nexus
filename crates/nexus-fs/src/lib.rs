//! `nexus-fs`: permissions and safe file operations (PERMISSIONS.md).
//!
//! > Invariant: no byte on disk changes unless a grant allows it, the user saw a preview and clicked
//! > Apply, and the original is in the journal. Deletion is always "move to Trash".

pub mod audit;
pub mod canon;
pub mod deny;
pub mod engine;
pub mod journal;
pub mod os;
pub mod preview;

pub use audit::{AuditEntry, AuditSink, MemoryAudit, NullAudit};
pub use canon::{CanonError, CanonicalPath, canonicalize, is_direct_child, is_within, same_path};
pub use deny::{DenyList, DenyRule, builtin_rules};
pub use engine::{FsError, NewGrant, PermissionEngine};
pub use journal::{
    AppliedChangeSet, ApplyError, ConflictDecision, Journal, JournalOp, Manifest, MoveToDirTrash, SystemTrash, Trasher,
    UndoConflict, UndoReport, move_path, write_atomic,
};
pub use preview::{Analysis, ResolvedOp, analyze, apply_edits, hash_bytes, hash_file, preview};
