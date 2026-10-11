//! `nexus-fs`: permissions and safe file operations (PERMISSIONS.md).
//!
//! > Invariant: no byte on disk changes unless a grant allows it, the user saw a preview and clicked
//! > Apply, and the original is in the journal. Deletion is always "move to Trash".

pub mod canon;

pub use canon::{CanonError, CanonicalPath, canonicalize, is_direct_child, is_within, same_path};
