//! Waybar output helpers.

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WaybarStatus {
    pub text: String,
    pub tooltip: String,
    pub class: String,
}

impl WaybarStatus {
    pub fn idle() -> Self {
        Self {
            text: "d2b".to_string(),
            tooltip: "d2b-wlterm ready".to_string(),
            class: "idle".to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("WaybarStatus serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_status_renders_waybar_json() {
        assert_eq!(
            WaybarStatus::idle().to_json(),
            r#"{"text":"d2b","tooltip":"d2b-wlterm ready","class":"idle"}"#
        );
    }

    #[test]
    fn json_output_escapes_quotes() {
        let status = WaybarStatus {
            text: "a\"b".into(),
            tooltip: "line\nnext".into(),
            class: "warn".into(),
        };
        assert_eq!(
            status.to_json(),
            r#"{"text":"a\"b","tooltip":"line\nnext","class":"warn"}"#
        );
    }
}
