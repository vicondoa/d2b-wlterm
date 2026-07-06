//! Waybar output helpers.

use serde::Serialize;
use wlterm_core::{Model, ShellVisualState, VmPowerState};
use wlterm_ui::RenderedAsyncError;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WaybarStatus {
    pub text: String,
    pub tooltip: String,
    pub class: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WaybarCounts {
    pub active_shells: usize,
    pub attached_shells: usize,
    pub detached_shells: usize,
    pub unavailable_shells: usize,
    pub online_vms: usize,
    pub offline_vms: usize,
    pub unknown_vms: usize,
    pub renderable_errors: usize,
}

impl WaybarStatus {
    pub fn idle() -> Self {
        Self::from_counts(WaybarCounts::default())
    }

    pub fn from_model(model: &Model) -> Self {
        let mut counts = WaybarCounts::default();
        for vm in model.vms() {
            match vm.power_state {
                VmPowerState::Online => counts.online_vms += 1,
                VmPowerState::Offline => counts.offline_vms += 1,
                VmPowerState::Unknown => counts.unknown_vms += 1,
            }
            for shell in &vm.sessions {
                match shell.visual_state {
                    ShellVisualState::Attached => {
                        counts.active_shells += 1;
                        counts.attached_shells += 1;
                    }
                    ShellVisualState::Detached => {
                        counts.active_shells += 1;
                        counts.detached_shells += 1;
                    }
                    ShellVisualState::Unavailable => counts.unavailable_shells += 1,
                }
            }
        }
        counts.renderable_errors = model
            .async_errors()
            .iter()
            .filter(|error| RenderedAsyncError::from_core(error).is_some())
            .count();
        Self::from_counts(counts)
    }

    pub fn from_counts(counts: WaybarCounts) -> Self {
        let class = if counts.renderable_errors > 0 {
            "error"
        } else if counts.online_vms == 0 && (counts.offline_vms + counts.unknown_vms) > 0 {
            "disabled"
        } else if counts.active_shells > 0 {
            "active"
        } else {
            "idle"
        };

        let mut tooltip = format!(
            "d2b-wlterm: {} active shell(s), {} attached, {} detached",
            counts.active_shells, counts.attached_shells, counts.detached_shells
        );
        if counts.offline_vms > 0 || counts.unknown_vms > 0 {
            tooltip.push_str(&format!(
                "; {} offline VM(s), {} unknown VM(s)",
                counts.offline_vms, counts.unknown_vms
            ));
        }
        if counts.renderable_errors > 0 {
            tooltip.push_str(&format!("; {} error(s)", counts.renderable_errors));
        }
        if counts == WaybarCounts::default() {
            tooltip = "d2b-wlterm ready".to_string();
        }

        Self {
            text: format!("${}", counts.active_shells),
            tooltip,
            class: class.to_string(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("WaybarStatus serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wlterm_core::friendly_name::FriendlyName;
    use wlterm_core::{AsyncErrorDisplay, Config, ModelEvent, VmId, VmSummary};

    fn vm(name: &str) -> VmId {
        VmId::new(name).unwrap()
    }

    fn shell(name: &str) -> FriendlyName {
        FriendlyName::from_candidate(name).unwrap()
    }

    #[test]
    fn idle_status_renders_waybar_json() {
        assert_eq!(
            WaybarStatus::idle().to_json(),
            r#"{"text":"$0","tooltip":"d2b-wlterm ready","class":"idle"}"#
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

    #[test]
    fn waybar_counts_active_shells() {
        let status = WaybarStatus::from_counts(WaybarCounts {
            active_shells: 2,
            attached_shells: 1,
            detached_shells: 1,
            online_vms: 1,
            ..WaybarCounts::default()
        });

        assert_eq!(status.text, "$2");
        assert_eq!(status.class, "active");
        assert!(status.tooltip.contains("1 attached"));
    }

    #[test]
    fn waybar_prefers_global_error_state() {
        let status = WaybarStatus::from_counts(WaybarCounts {
            active_shells: 2,
            attached_shells: 2,
            online_vms: 1,
            renderable_errors: 1,
            ..WaybarCounts::default()
        });

        assert_eq!(status.class, "error");
        assert!(status.tooltip.contains("1 error"));
    }

    #[test]
    fn waybar_marks_all_offline_state_disabled() {
        let status = WaybarStatus::from_counts(WaybarCounts {
            offline_vms: 1,
            ..WaybarCounts::default()
        });

        assert_eq!(status.class, "disabled");
        assert!(status.tooltip.contains("1 offline"));
    }

    #[test]
    fn waybar_state_from_model_counts_shells_and_errors() {
        let mut summary = VmSummary::new(vm("work"), VmPowerState::Online);
        summary
            .sessions
            .push(wlterm_core::ShellSession::attached(shell("quiet-otter")));
        summary
            .sessions
            .push(wlterm_core::ShellSession::detached(shell("brave-panda")));
        let mut offline = VmSummary::new(vm("personal"), VmPowerState::Offline);
        offline.sessions.push(wlterm_core::ShellSession {
            name: shell("old-panda"),
            visual_state: ShellVisualState::Unavailable,
            is_default: false,
        });

        let mut cfg = Config::default();
        cfg.ui.async_error_display = AsyncErrorDisplay::Waybar;
        let mut model = Model::new(cfg);
        model.apply(ModelEvent::VmSnapshot {
            vms: vec![summary, offline],
        });
        model.apply(ModelEvent::AsyncError {
            message: "late failure".into(),
        });

        let status = WaybarStatus::from_model(&model);
        assert_eq!(status.text, "$2");
        assert_eq!(status.class, "error");
        assert!(status.tooltip.contains("2 active"));
        assert!(status.tooltip.contains("1 offline"));
        assert!(status.tooltip.contains("1 error"));
    }
}
