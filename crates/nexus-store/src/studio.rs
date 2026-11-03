//! Transactional metadata for content-addressed, shared Studio model assets.

use crate::{Store, StoreError};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

/// Engine-compatible model category, independent of the platform executable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ModelKind {
    Chat,
    Image,
    Speech,
    Tts,
}

impl ModelKind {
    /// Stable database representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Image => "image",
            Self::Speech => "speech",
            Self::Tts => "tts",
        }
    }
}

/// One physical file shared by any number of models and platform applications.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct StudioAsset {
    pub sha256: String,
    /// Validated DIST-relative path, resolved only at the service boundary.
    pub path: String,
    pub bytes: u64,
}

/// Named role of a shared asset (weights, projector, voices, tokenizer, etc.).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AssetRole {
    pub role: String,
    pub asset: StudioAsset,
}

/// Model identity, asset set and optional backend-specific conversion variant.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct StudioModel {
    pub id: String,
    pub name: String,
    pub kind: ModelKind,
    /// `universal` for shared weights, or an explicit backend/target variant.
    pub variant: String,
    /// Bounded JSON of provenance/license/configuration; never credentials.
    pub metadata: String,
    pub assets: Vec<AssetRole>,
}

impl Store {
    /// Persist or clear the selected model for one workspace.
    pub fn studio_select(&self, kind: ModelKind, id: Option<&str>) -> Result<(), StoreError> {
        self.with_conn(|c| {
            if let Some(id)=id {
                let matches:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM studio_models WHERE id=?1 AND kind=?2)",params![id,kind.as_str()],|r|r.get(0))?;
                if !matches { return Err(StoreError::NotFound("workspace model")); }
                c.execute("INSERT INTO studio_selections(kind,model_id) VALUES(?1,?2) ON CONFLICT(kind) DO UPDATE SET model_id=?2",params![kind.as_str(),id])?;
            } else { c.execute("DELETE FROM studio_selections WHERE kind=?1",[kind.as_str()])?; }
            Ok(())
        })
    }

    /// Selected model per workspace; selection is separate from engine running state.
    pub fn list_studio_selections(&self) -> Result<Vec<(String, String)>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT kind,model_id FROM studio_selections ORDER BY kind")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Record a verified physical asset before a model references it.
    pub fn studio_publish_asset(&self, asset: &StudioAsset) -> Result<(), StoreError> {
        self.with_conn(|c| {
            c.execute(
                "INSERT OR IGNORE INTO studio_assets(sha256,path,bytes) VALUES(?1,?2,?3)",
                params![asset.sha256, asset.path, asset.bytes as i64],
            )?;
            let matches: bool = c.query_row(
                "SELECT path=?2 AND bytes=?3 FROM studio_assets WHERE sha256=?1",
                params![asset.sha256, asset.path, asset.bytes as i64],
                |r| r.get(0),
            )?;
            if !matches {
                return Err(StoreError::Invalid {
                    key: "studio asset".into(),
                    reason: "digest metadata conflict".into(),
                });
            }
            Ok(())
        })
    }

    /// Find a digest without reading/copying its bytes again.
    pub fn studio_asset(&self, sha: &str) -> Result<Option<StudioAsset>, StoreError> {
        self.with_conn(|c| {
            Ok(c.query_row(
                "SELECT sha256,path,bytes FROM studio_assets WHERE sha256=?1",
                [sha],
                |r| {
                    Ok(StudioAsset {
                        sha256: r.get(0)?,
                        path: r.get(1)?,
                        bytes: r.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .optional()?)
        })
    }

    /// Publish the entire verified asset set atomically. A digest cannot be rebound to other bytes.
    pub fn studio_upsert(&self, model: &StudioModel) -> Result<(), StoreError> {
        self.with_conn(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO studio_models(id,name,kind,variant,metadata) VALUES(?1,?2,?3,?4,?5)
                ON CONFLICT(id) DO UPDATE SET name=?2,kind=?3,variant=?4,metadata=?5",
                params![
                    model.id,
                    model.name,
                    model.kind.as_str(),
                    model.variant,
                    model.metadata
                ],
            )?;
            tx.execute("DELETE FROM studio_model_assets WHERE model_id=?1", [&model.id])?;
            for role in &model.assets {
                let a = &role.asset;
                tx.execute(
                    "INSERT OR IGNORE INTO studio_assets(sha256,path,bytes) VALUES(?1,?2,?3)",
                    params![a.sha256, a.path, a.bytes as i64],
                )?;
                let matches: bool = tx.query_row(
                    "SELECT path=?2 AND bytes=?3 FROM studio_assets WHERE sha256=?1",
                    params![a.sha256, a.path, a.bytes as i64],
                    |r| r.get(0),
                )?;
                if !matches {
                    return Err(StoreError::Invalid {
                        key: "studio asset".into(),
                        reason: "digest metadata conflict".into(),
                    });
                }
                tx.execute(
                    "INSERT INTO studio_model_assets(model_id,role,sha256) VALUES(?1,?2,?3)",
                    params![model.id, role.role, a.sha256],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
    }

    /// List models and their shared asset roles in deterministic order.
    pub fn studio_models(&self) -> Result<Vec<StudioModel>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare("SELECT id,name,kind,variant,metadata FROM studio_models ORDER BY name,id")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)))?.collect::<Result<Vec<_>,_>>()?;
            let mut out = Vec::new();
            for (id,name,kind,variant,metadata) in rows {
                let kind = match kind.as_str() { "chat" => ModelKind::Chat, "image" => ModelKind::Image, "speech" => ModelKind::Speech, "tts" => ModelKind::Tts, _ => return Err(StoreError::Invalid { key: "model kind".into(), reason: "unknown kind".into() }) };
                let mut roles = c.prepare("SELECT r.role,a.sha256,a.path,a.bytes FROM studio_model_assets r JOIN studio_assets a ON a.sha256=r.sha256 WHERE r.model_id=?1 ORDER BY r.role")?;
                let assets = roles.query_map([&id], |r| Ok(AssetRole { role:r.get(0)?, asset: StudioAsset { sha256:r.get(1)?,path:r.get(2)?,bytes:r.get::<_,i64>(3)? as u64 } }))?.collect::<Result<Vec<_>,_>>()?;
                out.push(StudioModel { id,name,kind,variant,metadata,assets });
            }
            Ok(out)
        })
    }

    /// Assets no model references, largest first. These are the only reclaim candidates.
    pub fn studio_orphan_assets(&self) -> Result<Vec<StudioAsset>, StoreError> {
        self.with_conn(|c| {
            let mut stmt = c.prepare(
                "SELECT sha256,path,bytes FROM studio_assets
                 WHERE sha256 NOT IN (SELECT sha256 FROM studio_model_assets)
                 ORDER BY bytes DESC, sha256",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    Ok(StudioAsset {
                        sha256: r.get(0)?,
                        path: r.get(1)?,
                        bytes: r.get::<_, i64>(2)? as u64,
                    })
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
    }

    /// Forget one asset row, but only while no model references it. Returns whether a row was removed.
    pub fn studio_forget_orphan(&self, sha: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| {
            Ok(c.execute(
                "DELETE FROM studio_assets WHERE sha256=?1
                 AND sha256 NOT IN (SELECT sha256 FROM studio_model_assets)",
                [sha],
            )? > 0)
        })
    }

    /// Remove a logical model. Physical assets are retained for shared references and safe recovery.
    pub fn studio_remove(&self, id: &str) -> Result<bool, StoreError> {
        self.with_conn(|c| Ok(c.execute("DELETE FROM studio_models WHERE id=?1", [id])? > 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(n: u8) -> StudioAsset {
        StudioAsset {
            sha256: format!("{n:02x}").repeat(32),
            path: format!("models/{n}.gguf"),
            bytes: u64::from(n) * 10,
        }
    }

    fn model(id: &str, assets: &[StudioAsset]) -> StudioModel {
        StudioModel {
            id: id.into(),
            name: id.into(),
            kind: ModelKind::Chat,
            variant: "universal".into(),
            metadata: "{}".into(),
            assets: assets
                .iter()
                .map(|a| AssetRole {
                    role: a.path.clone(),
                    asset: a.clone(),
                })
                .collect(),
        }
    }

    #[test]
    fn orphans_exclude_shared_assets_and_forget_is_guarded() {
        let store = Store::open_in_memory().expect("store");
        let (shared, solo) = (asset(1), asset(2));
        store
            .studio_upsert(&model("a", &[shared.clone(), solo.clone()]))
            .expect("a");
        store
            .studio_upsert(&model("b", std::slice::from_ref(&shared)))
            .expect("b");
        assert!(store.studio_orphan_assets().expect("none").is_empty());

        store.studio_remove("a").expect("remove a");
        let orphans = store.studio_orphan_assets().expect("orphans");
        assert_eq!(orphans, vec![solo.clone()]);
        // The shared asset is still referenced by b and can never be forgotten.
        assert!(!store.studio_forget_orphan(&shared.sha256).expect("guard"));
        assert!(store.studio_forget_orphan(&solo.sha256).expect("forget"));
        assert!(!store.studio_forget_orphan(&solo.sha256).expect("again"));
        assert!(store.studio_asset(&solo.sha256).expect("lookup").is_none());
    }
}
