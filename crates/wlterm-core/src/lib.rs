//! Core model types for d2b-wlterm.

pub mod friendly_name;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub public_socket_path: String,
    pub wezterm_command: Vec<String>,
    pub refresh_interval_seconds: u64,
    pub ui: UiConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            public_socket_path: default_public_socket_path(),
            wezterm_command: vec!["weezterm".into(), "start".into(), "--".into()],
            refresh_interval_seconds: 5,
            ui: UiConfig::default(),
        }
    }
}

fn default_public_socket_path() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .map(|dir| format!("{dir}/d2b/public.sock"))
        .unwrap_or_else(|_| "/run/d2b/public.sock".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    pub default_open_behavior: OpenBehavior,
    pub stop_confirmation: bool,
    pub async_error_display: AsyncErrorDisplay,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            default_open_behavior: OpenBehavior::FocusExisting,
            stop_confirmation: true,
            async_error_display: AsyncErrorDisplay::Notification,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenBehavior {
    FocusExisting,
    OpenNew,
    Prompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AsyncErrorDisplay {
    Notification,
    Waybar,
    Silent,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionId(String);

impl std::fmt::Debug for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("SessionId").field(&"<redacted>").finish()
    }
}

impl SessionId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::EmptySessionId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelError {
    EmptySessionId,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_exposes_planned_safe_behavior() {
        let cfg = Config::default();
        assert_eq!(cfg.refresh_interval_seconds, 5);
        assert_eq!(cfg.ui.default_open_behavior, OpenBehavior::FocusExisting);
        assert!(cfg.ui.stop_confirmation);
        assert_eq!(cfg.ui.async_error_display, AsyncErrorDisplay::Notification);
    }

    #[test]
    fn session_id_rejects_empty_values() {
        assert_eq!(SessionId::new(" "), Err(ModelError::EmptySessionId));
        assert_eq!(SessionId::new("work").unwrap().as_str(), "work");
    }

    #[test]
    fn session_id_debug_is_redacted() {
        let session = SessionId::new("quiet-otter").expect("session");
        let rendered = format!("{session:?}");
        assert!(!rendered.contains("quiet-otter"));
        assert!(rendered.contains("redacted"));
    }
}
