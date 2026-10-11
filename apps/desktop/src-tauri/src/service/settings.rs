use std::collections::BTreeMap;

use nexus_core::error::NexusError;
use nexus_core::settings::Settings;
use serde::Deserialize;
use serde_json::Value;
use specta::Type;

use crate::state::AppCtx;

use super::Caller;

/// A setting's new value. Typed instead of `serde_json::Value`, which specta rc.25 cannot export
/// (infinite recursion while collecting the recursive type; spike S6 finding).
#[derive(Clone, Debug, Deserialize, Type)]
#[serde(untagged)]
pub enum SettingValue {
    Null(()),
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    List(Vec<String>),
    Flags(BTreeMap<String, bool>),
}

impl From<SettingValue> for Value {
    fn from(v: SettingValue) -> Self {
        match v {
            SettingValue::Null(()) => Value::Null,
            SettingValue::Bool(b) => Value::Bool(b),
            SettingValue::Int(i) => Value::from(i),
            SettingValue::Float(f) => Value::from(f),
            SettingValue::Text(s) => Value::String(s),
            SettingValue::List(l) => Value::from(l),
            SettingValue::Flags(m) => {
                Value::Object(m.into_iter().map(|(k, v)| (k, Value::Bool(v))).collect())
            }
        }
    }
}

pub fn get(ctx: &AppCtx) -> Settings {
    ctx.settings_snapshot()
}

/// Set one key. Only the desktop may change settings.
pub fn set(ctx: &AppCtx, caller: &Caller, key: &str, value: SettingValue) -> Result<Settings, NexusError> {
    if !caller.is_desktop() {
        return Err(NexusError::ForbiddenRemote);
    }
    let current = ctx.settings_snapshot();
    let next = ctx.store.set_setting(&current, key, value.into())?;
    if let Ok(mut guard) = ctx.settings.write() {
        *guard = next.clone();
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nexus_core::error::ErrorCode;
    use nexus_core::settings::SharePolicy;
    use serde_json::json;

    #[test]
    fn values_deserialize_and_convert() {
        let parse = |j: Value| -> Value { serde_json::from_value::<SettingValue>(j).unwrap().into() };
        assert_eq!(parse(json!(null)), json!(null));
        assert_eq!(parse(json!(true)), json!(true));
        assert_eq!(parse(json!(4096)), json!(4096));
        assert_eq!(parse(json!("dark")), json!("dark"));
        assert_eq!(parse(json!(["*"])), json!(["*"]));
        assert_eq!(parse(json!({"download": false})), json!({"download": false}));
    }

    #[tokio::test]
    async fn set_persists_validates_and_is_desktop_only() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = AppCtx::for_tests(dir.path());
        let next = set(
            &ctx,
            &Caller::Desktop,
            "runtime.ctx_size",
            SettingValue::Int(4096),
        )
        .unwrap();
        assert_eq!(next.runtime_ctx_size, 4096);
        assert_eq!(get(&ctx).runtime_ctx_size, 4096);
        assert_eq!(ctx.store.load_settings().unwrap().runtime_ctx_size, 4096);

        let err = set(
            &ctx,
            &Caller::Desktop,
            "runtime.ctx_size",
            SettingValue::Text("big".into()),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::InvalidInput);
        assert_eq!(get(&ctx).runtime_ctx_size, 4096);

        let remote = Caller::Remote {
            device_id: "d".into(),
            policy: SharePolicy::Full,
        };
        let err = set(&ctx, &remote, "ui.theme", SettingValue::Text("light".into())).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ForbiddenRemote);
    }
}
