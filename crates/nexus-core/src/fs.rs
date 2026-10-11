//! Filesystem permission types shared by the engine, the store, and IPC (PERMISSIONS §2).

use serde::{Deserialize, Serialize};

use crate::ids::GrantId;

/// What a grant allows. `Full` = read, write, create, move, and trash.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GrantLevel {
    Read,
    ReadWrite,
    Full,
}

/// What one operation needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    /// Read a file, list a directory, read metadata.
    Read,
    /// Modify an existing file.
    Write,
    /// Create a new file or directory.
    Create,
    /// Move or rename.
    Move,
    /// Move to the Trash.
    Trash,
}

impl Access {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Write => "write",
            Self::Create => "create",
            Self::Move => "move",
            Self::Trash => "trash",
        }
    }
}

impl GrantLevel {
    /// Does this level permit `access`?
    pub fn allows(self, access: Access) -> bool {
        matches!(
            (self, access),
            (_, Access::Read) | (Self::ReadWrite | Self::Full, Access::Write) | (Self::Full, _)
        )
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::ReadWrite => "read_write",
            Self::Full => "full",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "read" => Self::Read,
            "read_write" => Self::ReadWrite,
            "full" => Self::Full,
            _ => return None,
        })
    }
}

/// Persistent grants live in the database; session grants vanish when the app quits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum GrantScope {
    Persistent,
    Session,
}

/// A folder (or single file) the app may use.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Grant {
    pub id: GrantId,
    /// Canonical, absolute path (no trailing separator).
    pub root: String,
    pub level: GrantLevel,
    /// `false`: only direct children (and the root itself) match.
    pub recursive: bool,
    pub scope: GrantScope,
    /// Unix milliseconds (UTC); `None` = never.
    pub expires_at: Option<i64>,
    pub label: Option<String>,
    pub created_at: i64,
}

impl Grant {
    pub fn is_expired(&self, now_ms: i64) -> bool {
        self.expires_at.is_some_and(|t| t <= now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_matrix_matches_the_spec() {
        use Access::*;
        let table = [
            (GrantLevel::Read, [true, false, false, false, false]),
            (GrantLevel::ReadWrite, [true, true, false, false, false]),
            (GrantLevel::Full, [true, true, true, true, true]),
        ];
        for (level, expected) in table {
            let got = [Read, Write, Create, Move, Trash].map(|a| level.allows(a));
            assert_eq!(got, expected, "{level:?}");
        }
        assert!(GrantLevel::Read < GrantLevel::ReadWrite && GrantLevel::ReadWrite < GrantLevel::Full);
    }

    #[test]
    fn strings_round_trip_and_expiry() {
        for l in [GrantLevel::Read, GrantLevel::ReadWrite, GrantLevel::Full] {
            assert_eq!(GrantLevel::parse(l.as_str()), Some(l));
        }
        let g = Grant {
            id: GrantId::from("g"),
            root: "/a".into(),
            level: GrantLevel::Read,
            recursive: true,
            scope: GrantScope::Session,
            expires_at: Some(1000),
            label: None,
            created_at: 0,
        };
        assert!(!g.is_expired(999) && g.is_expired(1000) && g.is_expired(2000));
        assert!(
            !Grant {
                expires_at: None,
                ..g
            }
            .is_expired(i64::MAX)
        );
    }
}
