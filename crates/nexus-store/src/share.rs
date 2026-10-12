//! Paired LAN devices (`share_devices`). Only the SHA-256 of a device token is stored.

use nexus_core::settings::SharePolicy;
use nexus_core::share::ShareDevice;
use rusqlite::{OptionalExtension, Row, params};

use crate::{Store, StoreError, now_ms};

fn policy_str(p: SharePolicy) -> &'static str {
    match p {
        SharePolicy::ChatOnly => "chat_only",
        SharePolicy::ChatRead => "chat_read",
        SharePolicy::Full => "full",
    }
}

fn parse_policy(s: &str) -> SharePolicy {
    match s {
        "chat_only" => SharePolicy::ChatOnly,
        "full" => SharePolicy::Full,
        _ => SharePolicy::ChatRead,
    }
}

fn row(r: &Row<'_>) -> rusqlite::Result<ShareDevice> {
    Ok(ShareDevice {
        id: r.get("id")?,
        name: r.get("name")?,
        policy: parse_policy(&r.get::<_, String>("policy")?),
        user_agent: r.get("user_agent")?,
        last_ip: r.get("last_ip")?,
        created_at: r.get("created_at")?,
        last_seen_at: r.get("last_seen_at")?,
        expires_at: r.get("expires_at")?,
    })
}

const COLS: &str = "id, name, policy, user_agent, last_ip, created_at, last_seen_at, expires_at";

impl Store {
    /// Store a newly paired device under the hash of its token.
    pub fn insert_share_device(&self, d: &ShareDevice, token_hash: &str) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT INTO share_devices (id, name, token_hash, policy, user_agent, last_ip, created_at, last_seen_at, expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    d.id,
                    d.name,
                    token_hash,
                    policy_str(d.policy),
                    d.user_agent,
                    d.last_ip,
                    d.created_at,
                    d.last_seen_at,
                    d.expires_at
                ],
            )?;
            Ok(())
        })
    }

    /// The device owning this token hash, unless its session has expired.
    pub fn share_device_by_hash(&self, token_hash: &str) -> Result<Option<ShareDevice>, StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM share_devices WHERE token_hash = ?1 AND expires_at > ?2"),
                params![token_hash, now],
                row,
            )
            .optional()?)
        })
    }

    /// One device by id.
    pub fn share_device(&self, id: &str) -> Result<Option<ShareDevice>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                &format!("SELECT {COLS} FROM share_devices WHERE id = ?1"),
                params![id],
                row,
            )
            .optional()?)
        })
    }

    /// Every non-expired device, newest first.
    pub fn list_share_devices(&self) -> Result<Vec<ShareDevice>, StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {COLS} FROM share_devices WHERE expires_at > ?1 ORDER BY created_at DESC"
            ))?;
            let rows = stmt.query_map(params![now], row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Record activity (last seen time and IP).
    pub fn touch_share_device(&self, id: &str, ip: Option<&str>) -> Result<(), StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            c.execute(
                "UPDATE share_devices SET last_seen_at = ?2, last_ip = COALESCE(?3, last_ip) WHERE id = ?1",
                params![id, now, ip],
            )?;
            Ok(())
        })
    }

    /// Rename a device and/or change its policy.
    pub fn update_share_device(
        &self,
        id: &str,
        name: Option<&str>,
        policy: Option<SharePolicy>,
    ) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let n = c.execute(
                "UPDATE share_devices SET name = COALESCE(?2, name), policy = COALESCE(?3, policy) WHERE id = ?1",
                params![id, name, policy.map(policy_str)],
            )?;
            if n == 0 {
                return Err(StoreError::NotFound("device"));
            }
            Ok(())
        })
    }

    /// Delete a device: its token stops working immediately. `false` if it did not exist.
    pub fn revoke_share_device(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM share_devices WHERE id = ?1", params![id])? > 0))
    }

    /// Tag a conversation with where it was started (`desktop` or `remote:<device_id>`).
    pub fn set_conversation_origin(&self, conversation_id: &str, origin: &str) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "UPDATE conversations SET origin = ?2 WHERE id = ?1",
                params![conversation_id, origin],
            )?;
            Ok(())
        })
    }

    /// Number of non-expired devices.
    pub fn count_share_devices(&self) -> Result<u32, StoreError> {
        let now = now_ms();
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT COUNT(*) FROM share_devices WHERE expires_at > ?1",
                params![now],
                |r| r.get::<_, u32>(0),
            )?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(id: &str) -> ShareDevice {
        ShareDevice {
            id: id.into(),
            name: "Kim's iPhone".into(),
            policy: SharePolicy::ChatRead,
            user_agent: None,
            last_ip: Some("192.168.1.5".into()),
            created_at: now_ms(),
            last_seen_at: None,
            expires_at: now_ms() + 60_000,
        }
    }

    #[test]
    fn device_lifecycle() {
        let s = Store::open_in_memory().unwrap();
        s.insert_share_device(&device("d1"), "hash1").unwrap();
        assert_eq!(s.share_device_by_hash("hash1").unwrap().unwrap().id, "d1");
        assert!(s.share_device_by_hash("nope").unwrap().is_none());
        s.update_share_device("d1", Some("iPad"), Some(SharePolicy::Full))
            .unwrap();
        let d = s.share_device("d1").unwrap().unwrap();
        assert_eq!((d.name.as_str(), d.policy), ("iPad", SharePolicy::Full));
        assert_eq!(s.count_share_devices().unwrap(), 1);
        assert!(s.revoke_share_device("d1").unwrap());
        assert!(s.share_device_by_hash("hash1").unwrap().is_none());
    }

    #[test]
    fn expired_devices_do_not_authenticate() {
        let s = Store::open_in_memory().unwrap();
        let mut d = device("d2");
        d.expires_at = now_ms() - 1;
        s.insert_share_device(&d, "h2").unwrap();
        assert!(s.share_device_by_hash("h2").unwrap().is_none());
    }
}
