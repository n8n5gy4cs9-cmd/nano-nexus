//! Typed application settings (DATA-AND-IPC §1). Stored one JSON value per dotted key.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::NexusError;

/// UI colour theme.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

/// What a paired LAN device may do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SharePolicy {
    ChatOnly,
    #[default]
    ChatRead,
    Full,
}

/// Every setting, with the defaults from the spec.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(default)]
pub struct Settings {
    #[serde(rename = "ui.theme")]
    pub ui_theme: Theme,
    #[serde(rename = "ui.accent")]
    pub ui_accent: String,
    #[serde(rename = "ui.show_perf_stats")]
    pub ui_show_perf_stats: bool,
    /// Comma-separated ids of UI plugins the user turned off (see `apps/desktop/src/plugins`).
    #[serde(rename = "plugins.disabled")]
    pub plugins_disabled: String,
    #[serde(rename = "runtime.ctx_size")]
    pub runtime_ctx_size: u32,
    /// `"all"` or a number as a string.
    #[serde(rename = "runtime.gpu_layers")]
    pub runtime_gpu_layers: String,
    /// `"auto"`, `"on"` or `"off"`.
    #[serde(rename = "runtime.flash_attn")]
    pub runtime_flash_attn: String,
    /// `None` = auto.
    #[serde(rename = "runtime.threads")]
    pub runtime_threads: Option<u32>,
    #[serde(rename = "app.quit_on_window_close")]
    pub app_quit_on_window_close: bool,
    /// 0 = never unload.
    #[serde(rename = "runtime.idle_unload_minutes")]
    pub runtime_idle_unload_minutes: u32,
    #[serde(rename = "models.active_model_id")]
    pub models_active_model_id: String,
    #[serde(rename = "privacy.allow_network_downloads")]
    pub privacy_allow_network_downloads: bool,
    #[serde(rename = "notifications.os_when_unfocused")]
    pub notifications_os_when_unfocused: bool,
    /// Per-kind switches; a kind that is absent counts as enabled (see [`Settings::notification_kind_enabled`]).
    #[serde(rename = "notifications.kinds")]
    pub notifications_kinds: BTreeMap<String, bool>,
    /// Quiet hours start, local time `"HH:MM"`. Empty = no quiet hours.
    #[serde(rename = "notifications.quiet_start")]
    pub notifications_quiet_start: String,
    /// Quiet hours end, local time `"HH:MM"` (may be earlier than the start: overnight).
    #[serde(rename = "notifications.quiet_end")]
    pub notifications_quiet_end: String,
    #[serde(rename = "journal.retention_days")]
    pub journal_retention_days: u32,
    #[serde(rename = "journal.max_bytes")]
    pub journal_max_bytes: u64,
    #[serde(rename = "models.extra_dirs")]
    pub models_extra_dirs: Vec<String>,
    #[serde(rename = "share.enabled")]
    pub share_enabled: bool,
    #[serde(rename = "share.autostart")]
    pub share_autostart: bool,
    #[serde(rename = "share.port")]
    pub share_port: u16,
    #[serde(rename = "share.bind")]
    pub share_bind: String,
    #[serde(rename = "share.mdns")]
    pub share_mdns: bool,
    #[serde(rename = "share.mdns_name")]
    pub share_mdns_name: String,
    #[serde(rename = "share.https")]
    pub share_https: bool,
    #[serde(rename = "share.default_policy")]
    pub share_default_policy: SharePolicy,
    #[serde(rename = "share.allowed_assistants")]
    pub share_allowed_assistants: Vec<String>,
    #[serde(rename = "share.session_days")]
    pub share_session_days: u32,
    #[serde(rename = "share.max_devices")]
    pub share_max_devices: u32,
    #[serde(rename = "share.notify_pairing")]
    pub share_notify_pairing: bool,
    /// Knowledge libraries (M17): inject retrieved passages into ordinary chats. Off = prompts are unchanged.
    #[serde(rename = "knowledge.chat_context")]
    pub knowledge_chat_context: bool,
    /// Token budget of the injected passages (and of `kb_search` results).
    #[serde(rename = "knowledge.max_tokens")]
    pub knowledge_max_tokens: u32,
    /// How ordinary chats use library passages: `strict` (answer only from them) or `guide` (use them
    /// as the main reference but explain, adapt, and write new code, labelling what is not from them).
    #[serde(rename = "knowledge.answer_style")]
    pub knowledge_answer_style: String,
    /// The welcome tour was finished or skipped. Settings can show it again.
    #[serde(rename = "ui.onboarded")]
    pub ui_onboarded: bool,
    /// Show a tray / menu-bar icon with "Open", the sharing status and "Quit". Off by default.
    #[serde(rename = "app.show_tray_icon")]
    pub app_show_tray_icon: bool,
    /// Start NanoNexus when the user logs in. Off by default.
    #[serde(rename = "app.launch_at_login")]
    pub app_launch_at_login: bool,
    /// Add the catalog's short, model-specific hint (for example for MiniCPM5) to the system prompt.
    #[serde(rename = "models.use_prompt_hints")]
    pub models_use_prompt_hints: bool,
    /// Duo assistant: model id used for easy questions. Empty = the active model.
    #[serde(rename = "duo.light_model_id")]
    pub duo_light_model_id: String,
    /// Duo assistant: model id used for hard questions. Empty = the active model.
    #[serde(rename = "duo.strong_model_id")]
    pub duo_strong_model_id: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_theme: Theme::System,
            ui_accent: "violet".into(),
            ui_show_perf_stats: true,
            plugins_disabled: String::new(),
            runtime_ctx_size: 8192,
            runtime_gpu_layers: "all".into(),
            runtime_flash_attn: "auto".into(),
            runtime_threads: None,
            // The app stays in the Dock on macOS until Cmd-Q.
            app_quit_on_window_close: !cfg!(target_os = "macos"),
            runtime_idle_unload_minutes: 0,
            models_active_model_id: "builtin".into(),
            privacy_allow_network_downloads: true,
            notifications_os_when_unfocused: true,
            notifications_kinds: BTreeMap::new(),
            notifications_quiet_start: String::new(),
            notifications_quiet_end: String::new(),
            journal_retention_days: 30,
            journal_max_bytes: 2_147_483_648,
            models_extra_dirs: Vec::new(),
            share_enabled: false,
            share_autostart: false,
            share_port: 9889,
            share_bind: "0.0.0.0".into(),
            share_mdns: true,
            share_mdns_name: "nanonexus".into(),
            share_https: false,
            share_default_policy: SharePolicy::ChatRead,
            share_allowed_assistants: vec!["*".into()],
            share_session_days: 30,
            share_max_devices: 5,
            share_notify_pairing: true,
            knowledge_chat_context: false,
            knowledge_max_tokens: 1200,
            knowledge_answer_style: "strict".into(),
            ui_onboarded: false,
            app_show_tray_icon: false,
            app_launch_at_login: false,
            models_use_prompt_hints: true,
            duo_light_model_id: String::new(),
            duo_strong_model_id: String::new(),
        }
    }
}

/// `"HH:MM"` → minutes since midnight.
fn parse_hhmm(s: &str) -> Option<u32> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m): (u32, u32) = (h.parse().ok()?, m.parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

/// A set of `dotted.key -> JSON value` changes.
pub type SettingsPatch = BTreeMap<String, Value>;

impl Settings {
    /// Whether notifications of `kind` are enabled (absent = enabled).
    pub fn notification_kind_enabled(&self, kind: &str) -> bool {
        self.notifications_kinds.get(kind).copied().unwrap_or(true)
    }

    /// Is `minute_of_day` (local time, 0..1440) inside the quiet hours?
    pub fn in_quiet_hours(&self, minute_of_day: u32) -> bool {
        let (Some(start), Some(end)) = (
            parse_hhmm(&self.notifications_quiet_start),
            parse_hhmm(&self.notifications_quiet_end),
        ) else {
            return false;
        };
        match start.cmp(&end) {
            std::cmp::Ordering::Equal => false,
            std::cmp::Ordering::Less => (start..end).contains(&minute_of_day),
            std::cmp::Ordering::Greater => minute_of_day >= start || minute_of_day < end,
        }
    }

    /// Should a notification of `kind` also be shown by the OS? Needs the master switch, the kind
    /// enabled, an unfocused window (an in-app bell is enough otherwise), and no quiet hours.
    pub fn wants_os_notification(&self, kind: &str, window_focused: bool, minute_of_day: u32) -> bool {
        self.notifications_os_when_unfocused
            && self.notification_kind_enabled(kind)
            && !window_focused
            && !self.in_quiet_hours(minute_of_day)
    }

    /// The value of one key as JSON, or `None` for an unknown key.
    pub fn get_key(&self, key: &str) -> Option<Value> {
        match serde_json::to_value(self) {
            Ok(Value::Object(mut m)) => m.remove(key),
            _ => None,
        }
    }

    /// Set one key. Unknown keys and values of the wrong type are rejected and leave `self` untouched.
    pub fn set_key(&mut self, key: &str, value: Value) -> Result<(), NexusError> {
        self.apply(&SettingsPatch::from([(key.to_owned(), value)]))
    }

    /// Apply a patch atomically: either every entry is valid or nothing changes.
    pub fn apply(&mut self, patch: &SettingsPatch) -> Result<(), NexusError> {
        let Value::Object(mut map) =
            serde_json::to_value(&*self).map_err(|e| NexusError::Internal(e.to_string()))?
        else {
            return Err(NexusError::Internal(
                "settings did not serialize to an object".into(),
            ));
        };
        for (k, v) in patch {
            if !map.contains_key(k) {
                return Err(NexusError::InvalidInput(format!("unknown setting: {k}")));
            }
            map.insert(k.clone(), v.clone());
        }
        let next: Settings = serde_json::from_value(Value::Object(map))
            .map_err(|e| NexusError::InvalidInput(format!("invalid setting value: {e}")))?;
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_match_spec() {
        let s = Settings::default();
        assert_eq!(s.runtime_ctx_size, 8192);
        assert_eq!(s.share_port, 9889);
        assert_eq!(s.share_default_policy, SharePolicy::ChatRead);
        assert_eq!(s.journal_max_bytes, 2_147_483_648);
        assert!(s.notification_kind_enabled("anything"));
    }

    #[test]
    fn os_notification_routing() {
        let mut s = Settings::default();
        assert!(s.wants_os_notification("download", false, 600));
        assert!(
            !s.wants_os_notification("download", true, 600),
            "focused: bell only"
        );
        s.notifications_kinds.insert("download".into(), false);
        assert!(!s.wants_os_notification("download", false, 600), "kind off");
        assert!(s.wants_os_notification("job", false, 600));
        s.notifications_os_when_unfocused = false;
        assert!(!s.wants_os_notification("job", false, 600), "master switch");
    }

    #[test]
    fn quiet_hours_overnight_and_same_day() {
        let mut s = Settings::default();
        assert!(!s.in_quiet_hours(0), "off by default");
        s.notifications_quiet_start = "22:00".into();
        s.notifications_quiet_end = "07:30".into();
        assert!(s.in_quiet_hours(23 * 60));
        assert!(s.in_quiet_hours(3 * 60));
        assert!(!s.in_quiet_hours(7 * 60 + 30));
        assert!(!s.in_quiet_hours(12 * 60));
        assert!(!s.wants_os_notification("job", false, 23 * 60));
        s.notifications_quiet_start = "13:00".into();
        s.notifications_quiet_end = "14:00".into();
        assert!(s.in_quiet_hours(13 * 60 + 30));
        assert!(!s.in_quiet_hours(14 * 60));
        s.notifications_quiet_end = "garbage".into();
        assert!(!s.in_quiet_hours(13 * 60 + 30), "invalid = off");
    }

    #[test]
    fn defaults_round_trip_through_json() {
        let s = Settings::default();
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<Settings>(&json).unwrap(), s);
        assert_eq!(s.get_key("ui.theme"), Some(json!("system")));
        assert_eq!(s.get_key("runtime.threads"), Some(Value::Null));
        assert_eq!(s.get_key("nope"), None);
    }

    #[test]
    fn empty_object_yields_defaults() {
        assert_eq!(
            serde_json::from_str::<Settings>("{}").unwrap(),
            Settings::default()
        );
    }

    #[test]
    fn set_key_validates() {
        let mut s = Settings::default();
        s.set_key("runtime.ctx_size", json!(4096)).unwrap();
        s.set_key("runtime.threads", json!(6)).unwrap();
        s.set_key("ui.theme", json!("dark")).unwrap();
        assert_eq!(
            (s.runtime_ctx_size, s.runtime_threads, s.ui_theme),
            (4096, Some(6), Theme::Dark)
        );

        let before = s.clone();
        assert!(s.set_key("runtime.ctx_size", json!("big")).is_err());
        assert!(s.set_key("ui.theme", json!("neon")).is_err());
        assert!(s.set_key("no.such.key", json!(1)).is_err());
        assert_eq!(s, before);
    }

    #[test]
    fn patch_is_atomic() {
        let mut s = Settings::default();
        let patch = SettingsPatch::from([
            ("share.port".to_owned(), json!(1234)),
            ("share.https".to_owned(), json!("yes")),
        ]);
        assert!(s.apply(&patch).is_err());
        assert_eq!(s.share_port, 9889);
    }
}
