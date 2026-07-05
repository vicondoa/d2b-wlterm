//! Core model, reducer, and action planning for d2b-wlterm.

pub mod friendly_name;

use friendly_name::FriendlyName;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub public_socket_path: String,
    pub wezterm_command: Vec<String>,
    pub refresh_interval_seconds: u64,
    pub ui: UiConfig,
    pub waybar: WaybarConfig,
    pub quickshell: QuickshellConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            public_socket_path: default_public_socket_path(),
            wezterm_command: vec!["wezterm".into(), "start".into(), "--".into()],
            refresh_interval_seconds: 5,
            ui: UiConfig::default(),
            waybar: WaybarConfig::default(),
            quickshell: QuickshellConfig::default(),
        }
    }
}

fn default_public_socket_path() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .map(|dir| format!("{dir}/d2b/public.sock"))
        .unwrap_or_else(|_| "/run/d2b/public.sock".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WaybarConfig {
    pub enable: bool,
    pub module_name: String,
}

impl Default for WaybarConfig {
    fn default() -> Self {
        Self {
            enable: false,
            module_name: "custom/d2b-wlterm".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QuickshellConfig {
    pub enable: bool,
    pub control_center_state_path: String,
}

impl Default for QuickshellConfig {
    fn default() -> Self {
        Self {
            enable: false,
            control_center_state_path: "$XDG_RUNTIME_DIR/d2b-wlterm/control-center.json"
                .to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenBehavior {
    FocusExisting,
    #[serde(alias = "open-new")]
    ForceOpen,
    Prompt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AsyncErrorDisplay {
    Inline,
    Notification,
    Waybar,
    Silent,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SafeCorrelation(String);

impl SafeCorrelation {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 80
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
        if valid {
            Ok(Self(value))
        } else {
            Err(ModelError::InvalidCorrelation)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub const fn metrics_label_value(&self) -> &'static str {
        "correlation"
    }
}

impl fmt::Debug for SafeCorrelation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SafeCorrelation").field(&self.0).finish()
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VmId(String);

impl VmId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::EmptyVmId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for VmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("VmId").field(&self.0).finish()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl fmt::Debug for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VmPowerState {
    Online,
    Offline,
    Unknown,
}

impl VmPowerState {
    pub const fn is_online(self) -> bool {
        matches!(self, Self::Online)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellVisualState {
    Detached,
    Attached,
    Unavailable,
}

impl ShellVisualState {
    pub const fn metrics_label_value(&self) -> &'static str {
        match self {
            Self::Detached => "detached",
            Self::Attached => "attached",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShellSession {
    pub name: FriendlyName,
    pub visual_state: ShellVisualState,
    #[serde(default)]
    pub is_default: bool,
}

impl fmt::Debug for ShellSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShellSession")
            .field("name", &"<redacted>")
            .field("visual_state", &self.visual_state)
            .field("is_default", &self.is_default)
            .finish()
    }
}

impl ShellSession {
    pub fn detached(name: FriendlyName) -> Self {
        Self {
            name,
            visual_state: ShellVisualState::Detached,
            is_default: false,
        }
    }

    pub fn attached(name: FriendlyName) -> Self {
        Self {
            name,
            visual_state: ShellVisualState::Attached,
            is_default: false,
        }
    }

    pub const fn is_attached(&self) -> bool {
        matches!(self.visual_state, ShellVisualState::Attached)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VmSummary {
    pub id: VmId,
    pub power_state: VmPowerState,
    #[serde(default)]
    pub sessions: Vec<ShellSession>,
}

impl VmSummary {
    pub fn new(id: VmId, power_state: VmPowerState) -> Self {
        Self {
            id,
            power_state,
            sessions: Vec::new(),
        }
    }

    pub fn visual_state(&self) -> VmVisualState {
        if self.power_state.is_online() {
            VmVisualState {
                power_state: self.power_state,
                can_list_sessions: true,
                can_create_session: true,
                can_open_session: true,
            }
        } else {
            VmVisualState {
                power_state: self.power_state,
                can_list_sessions: false,
                can_create_session: false,
                can_open_session: false,
            }
        }
    }

    pub fn session(&self, name: &FriendlyName) -> Option<&ShellSession> {
        self.sessions
            .iter()
            .find(|session| session.name.as_str() == name.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmVisualState {
    pub power_state: VmPowerState,
    pub can_list_sessions: bool,
    pub can_create_session: bool,
    pub can_open_session: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    config: Config,
    vms: BTreeMap<VmId, VmSummary>,
    async_errors: Vec<AsyncErrorEvent>,
}

impl Model {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            vms: BTreeMap::new(),
            async_errors: Vec::new(),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn vms(&self) -> impl Iterator<Item = &VmSummary> {
        self.vms.values()
    }

    pub fn vm(&self, vm: &VmId) -> Option<&VmSummary> {
        self.vms.get(vm)
    }

    pub fn async_errors(&self) -> &[AsyncErrorEvent] {
        &self.async_errors
    }

    pub fn apply(&mut self, event: ModelEvent) {
        match event {
            ModelEvent::VmSnapshot { vms } => {
                self.vms = vms.into_iter().map(|vm| (vm.id.clone(), vm)).collect();
            }
            ModelEvent::VmChanged { vm } => {
                self.vms.insert(vm.id.clone(), vm);
            }
            ModelEvent::AsyncError { message } => {
                self.async_errors.push(AsyncErrorEvent::new(
                    message,
                    self.config.ui.async_error_display,
                ));
            }
            ModelEvent::DismissAsyncError { index } => {
                if index < self.async_errors.len() {
                    self.async_errors.remove(index);
                }
            }
        }
    }

    pub fn plan(&self, intent: UserIntent) -> PlannedAction {
        match intent {
            UserIntent::RefreshVms => PlannedAction::RefreshVms,
            UserIntent::ListSessions { vm } => {
                if self.vm_is_online(&vm) {
                    PlannedAction::ListSessions { vm }
                } else {
                    PlannedAction::Disabled {
                        reason: DisabledReason::VmOffline,
                    }
                }
            }
            UserIntent::CreateSession { vm, name } => {
                if self.vm_is_online(&vm) {
                    PlannedAction::AttachShell {
                        vm,
                        name: Some(name),
                        force: false,
                    }
                } else {
                    PlannedAction::Disabled {
                        reason: DisabledReason::VmOffline,
                    }
                }
            }
            UserIntent::OpenSession { vm, name } => self.plan_open(vm, name),
            UserIntent::StopShell {
                vm,
                name,
                confirmed,
            } => {
                if self.config.ui.stop_confirmation && !confirmed {
                    PlannedAction::PromptStop { vm, name }
                } else {
                    PlannedAction::KillShell { vm, name }
                }
            }
            UserIntent::ForceOpenSession { vm, name } => {
                if self.vm_is_online(&vm) {
                    PlannedAction::AttachShell {
                        vm,
                        name: Some(name),
                        force: true,
                    }
                } else {
                    PlannedAction::Disabled {
                        reason: DisabledReason::VmOffline,
                    }
                }
            }
        }
    }

    fn plan_open(&self, vm: VmId, name: FriendlyName) -> PlannedAction {
        let Some(summary) = self.vm(&vm) else {
            return PlannedAction::Disabled {
                reason: DisabledReason::VmUnknown,
            };
        };
        if !summary.power_state.is_online() {
            return PlannedAction::Disabled {
                reason: DisabledReason::VmOffline,
            };
        }

        if summary
            .session(&name)
            .is_some_and(ShellSession::is_attached)
        {
            match self.config.ui.default_open_behavior {
                OpenBehavior::FocusExisting => PlannedAction::FocusExistingShell { vm, name },
                OpenBehavior::Prompt => PlannedAction::PromptAlreadyAttached { vm, name },
                OpenBehavior::ForceOpen => PlannedAction::AttachShell {
                    vm,
                    name: Some(name),
                    force: true,
                },
            }
        } else {
            PlannedAction::AttachShell {
                vm,
                name: Some(name),
                force: false,
            }
        }
    }

    fn vm_is_online(&self, vm: &VmId) -> bool {
        self.vm(vm)
            .is_some_and(|summary| summary.power_state.is_online())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelEvent {
    VmSnapshot { vms: Vec<VmSummary> },
    VmChanged { vm: VmSummary },
    AsyncError { message: String },
    DismissAsyncError { index: usize },
}

#[derive(Clone, PartialEq, Eq)]
pub enum UserIntent {
    RefreshVms,
    ListSessions {
        vm: VmId,
    },
    CreateSession {
        vm: VmId,
        name: FriendlyName,
    },
    OpenSession {
        vm: VmId,
        name: FriendlyName,
    },
    ForceOpenSession {
        vm: VmId,
        name: FriendlyName,
    },
    StopShell {
        vm: VmId,
        name: FriendlyName,
        confirmed: bool,
    },
}

impl fmt::Debug for UserIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefreshVms => f.write_str("RefreshVms"),
            Self::ListSessions { vm } => f.debug_struct("ListSessions").field("vm", vm).finish(),
            Self::CreateSession { vm, .. } => f
                .debug_struct("CreateSession")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::OpenSession { vm, .. } => f
                .debug_struct("OpenSession")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::ForceOpenSession { vm, .. } => f
                .debug_struct("ForceOpenSession")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::StopShell { vm, confirmed, .. } => f
                .debug_struct("StopShell")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .field("confirmed", confirmed)
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum PlannedAction {
    RefreshVms,
    ListSessions {
        vm: VmId,
    },
    AttachShell {
        vm: VmId,
        name: Option<FriendlyName>,
        force: bool,
    },
    FocusExistingShell {
        vm: VmId,
        name: FriendlyName,
    },
    PromptAlreadyAttached {
        vm: VmId,
        name: FriendlyName,
    },
    PromptStop {
        vm: VmId,
        name: FriendlyName,
    },
    KillShell {
        vm: VmId,
        name: FriendlyName,
    },
    Disabled {
        reason: DisabledReason,
    },
}

impl PlannedAction {
    pub const fn metrics_label_value(&self) -> &'static str {
        match self {
            Self::RefreshVms => "refresh-vms",
            Self::ListSessions { .. } => "list-sessions",
            Self::AttachShell { force, .. } if *force => "attach-shell-force",
            Self::AttachShell { .. } => "attach-shell",
            Self::FocusExistingShell { .. } => "focus-existing-shell",
            Self::PromptAlreadyAttached { .. } => "prompt-already-attached",
            Self::PromptStop { .. } => "prompt-stop",
            Self::KillShell { .. } => "kill-shell",
            Self::Disabled { reason } => reason.metrics_label_value(),
        }
    }
}

impl fmt::Debug for PlannedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefreshVms => f.write_str("RefreshVms"),
            Self::ListSessions { vm } => f.debug_struct("ListSessions").field("vm", vm).finish(),
            Self::AttachShell { vm, name, force } => f
                .debug_struct("AttachShell")
                .field("vm", vm)
                .field("has_name", &name.is_some())
                .field("force", force)
                .finish(),
            Self::FocusExistingShell { vm, .. } => f
                .debug_struct("FocusExistingShell")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptAlreadyAttached { vm, .. } => f
                .debug_struct("PromptAlreadyAttached")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptStop { vm, .. } => f
                .debug_struct("PromptStop")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::KillShell { vm, .. } => f
                .debug_struct("KillShell")
                .field("vm", vm)
                .field("name", &"<redacted>")
                .finish(),
            Self::Disabled { reason } => {
                f.debug_struct("Disabled").field("reason", reason).finish()
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisabledReason {
    VmOffline,
    VmUnknown,
}

impl DisabledReason {
    pub const fn metrics_label_value(self) -> &'static str {
        match self {
            Self::VmOffline => "disabled-vm-offline",
            Self::VmUnknown => "disabled-vm-unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsyncErrorEvent {
    pub message: String,
    pub display: AsyncErrorDisplay,
    pub correlation: Option<SafeCorrelation>,
}

impl AsyncErrorEvent {
    pub fn new(message: impl Into<String>, display: AsyncErrorDisplay) -> Self {
        Self {
            message: message.into(),
            display,
            correlation: None,
        }
    }

    pub fn with_correlation(
        message: impl Into<String>,
        display: AsyncErrorDisplay,
        correlation: SafeCorrelation,
    ) -> Self {
        Self {
            message: message.into(),
            display,
            correlation: Some(correlation),
        }
    }

    pub fn should_render(&self) -> bool {
        self.display != AsyncErrorDisplay::Silent
    }

    pub const fn metrics_label_value(&self) -> &'static str {
        match self.display {
            AsyncErrorDisplay::Inline => "inline",
            AsyncErrorDisplay::Notification => "notification",
            AsyncErrorDisplay::Waybar => "waybar",
            AsyncErrorDisplay::Silent => "silent",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelError {
    EmptyVmId,
    EmptySessionId,
    InvalidCorrelation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vm(name: &str) -> VmId {
        VmId::new(name).unwrap()
    }

    fn shell(name: &str) -> FriendlyName {
        FriendlyName::from_candidate(name).unwrap()
    }

    fn model_with_vm(summary: VmSummary, config: Config) -> Model {
        let mut model = Model::new(config);
        model.apply(ModelEvent::VmSnapshot { vms: vec![summary] });
        model
    }

    #[test]
    fn default_config_exposes_safe_behavior_and_serializes_shape() {
        let cfg = Config::default();
        assert_eq!(cfg.refresh_interval_seconds, 5);
        assert_eq!(cfg.ui.default_open_behavior, OpenBehavior::FocusExisting);
        assert!(cfg.ui.stop_confirmation);
        assert_eq!(cfg.ui.async_error_display, AsyncErrorDisplay::Notification);
        assert!(!cfg.waybar.enable);
        assert!(!cfg.quickshell.enable);

        let rendered = serde_json::to_string(&cfg).expect("config serializes");
        let roundtrip: Config = serde_json::from_str(&rendered).expect("config deserializes");
        assert_eq!(
            roundtrip.ui.default_open_behavior,
            OpenBehavior::FocusExisting
        );
        assert_eq!(roundtrip.waybar.module_name, "custom/d2b-wlterm");
        assert_eq!(
            roundtrip.quickshell.control_center_state_path,
            "$XDG_RUNTIME_DIR/d2b-wlterm/control-center.json"
        );
    }

    #[test]
    fn config_accepts_legacy_open_new_as_force_open() {
        let cfg: UiConfig = serde_json::from_value(serde_json::json!({
            "default_open_behavior": "open-new",
            "stop_confirmation": true,
            "async_error_display": "notification"
        }))
        .expect("legacy config");

        assert_eq!(cfg.default_open_behavior, OpenBehavior::ForceOpen);
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

    #[test]
    fn offline_vm_visual_state_disables_shell_actions() {
        let summary = VmSummary::new(vm("work"), VmPowerState::Offline);
        let visual = summary.visual_state();

        assert!(!visual.can_list_sessions);
        assert!(!visual.can_create_session);
        assert!(!visual.can_open_session);
    }

    #[test]
    fn offline_vm_cannot_list_create_or_open_shells() {
        let work = vm("work");
        let model = model_with_vm(
            VmSummary::new(work.clone(), VmPowerState::Offline),
            Config::default(),
        );

        assert_eq!(
            model.plan(UserIntent::ListSessions { vm: work.clone() }),
            PlannedAction::Disabled {
                reason: DisabledReason::VmOffline
            }
        );
        assert_eq!(
            model.plan(UserIntent::CreateSession {
                vm: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::Disabled {
                reason: DisabledReason::VmOffline
            }
        );
        assert_eq!(
            model.plan(UserIntent::OpenSession {
                vm: work,
                name: shell("quiet-otter")
            }),
            PlannedAction::Disabled {
                reason: DisabledReason::VmOffline
            }
        );
    }

    #[test]
    fn open_attached_shell_focuses_prompts_or_force_opens() {
        let work = vm("work");
        let mut summary = VmSummary::new(work.clone(), VmPowerState::Online);
        summary
            .sessions
            .push(ShellSession::attached(shell("quiet-otter")));

        let focus_model = model_with_vm(summary.clone(), Config::default());
        assert_eq!(
            focus_model.plan(UserIntent::OpenSession {
                vm: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::FocusExistingShell {
                vm: work.clone(),
                name: shell("quiet-otter")
            }
        );

        let mut prompt_cfg = Config::default();
        prompt_cfg.ui.default_open_behavior = OpenBehavior::Prompt;
        let prompt_model = model_with_vm(summary.clone(), prompt_cfg);
        assert_eq!(
            prompt_model.plan(UserIntent::OpenSession {
                vm: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::PromptAlreadyAttached {
                vm: work.clone(),
                name: shell("quiet-otter")
            }
        );

        let mut force_cfg = Config::default();
        force_cfg.ui.default_open_behavior = OpenBehavior::ForceOpen;
        let force_model = model_with_vm(summary, force_cfg);
        assert_eq!(
            force_model.plan(UserIntent::OpenSession {
                vm: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::AttachShell {
                vm: work,
                name: Some(shell("quiet-otter")),
                force: true
            }
        );
    }

    #[test]
    fn stop_requires_confirmation_until_confirmed() {
        let model = Model::new(Config::default());
        let work = vm("work");
        let name = shell("quiet-otter");

        assert_eq!(
            model.plan(UserIntent::StopShell {
                vm: work.clone(),
                name: name.clone(),
                confirmed: false
            }),
            PlannedAction::PromptStop {
                vm: work.clone(),
                name: name.clone()
            }
        );
        assert_eq!(
            model.plan(UserIntent::StopShell {
                vm: work.clone(),
                name: name.clone(),
                confirmed: true
            }),
            PlannedAction::KillShell { vm: work, name }
        );
    }

    #[test]
    fn async_errors_use_configured_display_model() {
        let mut cfg = Config::default();
        cfg.ui.async_error_display = AsyncErrorDisplay::Waybar;
        let mut model = Model::new(cfg);

        model.apply(ModelEvent::AsyncError {
            message: "late failure".to_string(),
        });

        assert_eq!(model.async_errors()[0].display, AsyncErrorDisplay::Waybar);
        assert!(model.async_errors()[0].should_render());
        assert_eq!(model.async_errors()[0].metrics_label_value(), "waybar");
    }

    #[test]
    fn async_errors_can_carry_safe_correlation() {
        let correlation = SafeCorrelation::new("wlterm-deadbeef").expect("safe");
        let event = AsyncErrorEvent::with_correlation(
            "daemon request failed",
            AsyncErrorDisplay::Inline,
            correlation,
        );

        assert_eq!(event.metrics_label_value(), "inline");
        assert_eq!(
            event.correlation.as_ref().map(SafeCorrelation::as_str),
            Some("wlterm-deadbeef")
        );
        assert_eq!(
            SafeCorrelation::new("quiet-otter/opaque-session-handle"),
            Err(ModelError::InvalidCorrelation)
        );
    }

    #[test]
    fn shell_names_do_not_expand_metric_cardinality() {
        let action = PlannedAction::AttachShell {
            vm: vm("work"),
            name: Some(shell("customer-project-shell")),
            force: false,
        };

        assert_eq!(action.metrics_label_value(), "attach-shell");
        assert!(!action.metrics_label_value().contains("customer"));
        assert_eq!(ShellVisualState::Attached.metrics_label_value(), "attached");
    }
}
