//! Typed string identifiers. UUIDv7 ids sort by creation time.

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            /// Generate a fresh UUIDv7 id.
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7().to_string())
            }

            /// Borrow the id as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(s: String) -> Self {
                Self(s)
            }
        }

        impl From<&str> for $name {
            fn from(s: &str) -> Self {
                Self(s.to_owned())
            }
        }
    };
}

id_type!(
    /// A chat conversation.
    ConversationId
);
id_type!(
    /// A chat message.
    MessageId
);
id_type!(
    /// An assistant's kebab-case id (user-chosen, not a UUID); `new()` still yields a UUID.
    AssistantId
);
id_type!(
    /// A filesystem permission grant.
    GrantId
);
id_type!(
    /// A journaled change set.
    ChangeSetId
);
id_type!(
    /// A background job.
    JobId
);
id_type!(
    /// An analyzer report.
    ReportId
);
id_type!(
    /// An installed model.
    ModelId
);
id_type!(
    /// A model download.
    DownloadId
);
id_type!(
    /// A notification.
    NotificationId
);
id_type!(
    /// A folder watch.
    WatchId
);
id_type!(
    /// A Rename Forge session.
    RenameSessionId
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn ids_are_unique() {
        let set: HashSet<_> = (0..1000).map(|_| MessageId::new()).collect();
        assert_eq!(set.len(), 1000);
    }

    #[test]
    fn v7_ids_sort_by_creation_time() {
        let a = ConversationId::new();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = ConversationId::new();
        assert!(a < b);
    }

    #[test]
    fn serializes_transparently() {
        let id = JobId::from("abc");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"abc\"");
        let back: JobId = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(back, id);
    }

    #[test]
    fn assistant_id_accepts_kebab() {
        assert_eq!(AssistantId::from("code-helper").to_string(), "code-helper");
    }
}
