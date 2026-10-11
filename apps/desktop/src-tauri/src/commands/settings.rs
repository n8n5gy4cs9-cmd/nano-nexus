use std::collections::BTreeMap;

use nexus_core::error::{IpcError, NexusError};
use nexus_core::settings::Settings;
use serde::Deserialize;
use serde_json::Value;
use specta::Type;
use tauri::State;

use super::CmdResult;
use crate::state::AppState;

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

#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> CmdResult<Settings> {
    Ok(state.settings_snapshot())
}

/// Set one key (dotted name, e.g. `"ui.theme"`). Unknown keys and wrongly typed values are rejected.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(
    state: State<'_, AppState>,
    key: String,
    value: SettingValue,
) -> CmdResult<Settings> {
    let current = state.settings_snapshot();
    let next = state
        .store
        .set_setting(&current, &key, value.into())
        .map_err(|e| IpcError::from(NexusError::from(e)))?;
    if let Ok(mut guard) = state.settings.write() {
        *guard = next.clone();
    }
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
