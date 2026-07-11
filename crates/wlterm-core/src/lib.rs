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
    pub wayland_proxy_command: Vec<String>,
    pub refresh_interval_seconds: u64,
    pub ui: UiConfig,
    pub waybar: WaybarConfig,
    pub quickshell: QuickshellConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            public_socket_path: default_public_socket_path(),
            wezterm_command: vec!["weezterm".into(), "start".into(), "--".into()],
            wayland_proxy_command: vec!["d2b-wayland-proxy".into()],
            refresh_interval_seconds: 5,
            ui: UiConfig::default(),
            waybar: WaybarConfig::default(),
            quickshell: QuickshellConfig::default(),
        }
    }
}

fn default_public_socket_path() -> String {
    std::env::var("D2B_PUBLIC_SOCKET").unwrap_or_else(|_| "/run/d2b/public.sock".to_string())
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
pub struct TargetId(String);

impl TargetId {
    pub fn new(value: impl Into<String>) -> Result<Self, ModelError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ModelError::EmptyTargetId);
        }
        d2b_toolkit_core::workload::validate_shell_target(&value)
            .map_err(|_| ModelError::InvalidTargetId)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_canonical(&self) -> bool {
        d2b_toolkit_core::WorkloadTarget::parse(&self.0).is_ok()
    }
}

impl fmt::Debug for TargetId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TargetId").field(&"<redacted>").finish()
    }
}

/// Source-compatible alias for callers that still construct local VM ids.
///
/// New code should use `TargetId`; discovered workloads always use their
/// canonical target, including first-class local VMs and unsafe-local providers.
pub type VmId = TargetId;

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetPowerState {
    Online,
    Offline,
    #[default]
    Unknown,
}

impl TargetPowerState {
    pub const fn is_online(self) -> bool {
        matches!(self, Self::Online)
    }
}

pub type VmPowerState = TargetPowerState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    #[default]
    LocalVm,
    QemuMedia,
    ProviderManaged,
    UnsafeLocal,
}

impl ProviderKind {
    pub const fn metrics_label_value(self) -> &'static str {
        match self {
            Self::LocalVm => "local-vm",
            Self::QemuMedia => "qemu-media",
            Self::ProviderManaged => "provider-managed",
            Self::UnsafeLocal => "unsafe-local",
        }
    }

    pub const fn is_unsafe_local(self) -> bool {
        matches!(self, Self::UnsafeLocal)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum IsolationPosture {
    #[default]
    VirtualMachine,
    ProviderManaged,
    UnsafeLocal,
}

impl IsolationPosture {
    pub const fn metrics_label_value(self) -> &'static str {
        match self {
            Self::VirtualMachine => "virtual-machine",
            Self::ProviderManaged => "provider-managed",
            Self::UnsafeLocal => "unsafe-local",
        }
    }

    pub const fn warning(self) -> Option<&'static str> {
        match self {
            Self::UnsafeLocal => Some("No isolation: runs in the host user session"),
            Self::VirtualMachine | Self::ProviderManaged => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionPersistence {
    #[default]
    RuntimeManaged,
    UserManagerLifetime,
}

impl SessionPersistence {
    pub const fn metrics_label_value(self) -> &'static str {
        match self {
            Self::RuntimeManaged => "runtime-managed",
            Self::UserManagerLifetime => "user-manager-lifetime",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetAvailability {
    #[default]
    Ready,
    HelperUnavailable,
    HelperStale,
    UserManagerUnavailable,
    GraphicalSessionInactive,
    WaylandUnavailable,
    ProxyUnavailable,
    Degraded,
}

impl TargetAvailability {
    pub const fn metrics_label_value(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::HelperUnavailable => "helper-unavailable",
            Self::HelperStale => "helper-stale",
            Self::UserManagerUnavailable => "user-manager-unavailable",
            Self::GraphicalSessionInactive => "graphical-session-inactive",
            Self::WaylandUnavailable => "wayland-unavailable",
            Self::ProxyUnavailable => "proxy-unavailable",
            Self::Degraded => "degraded",
        }
    }

    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub const fn remediation(self) -> Option<TargetRemediation> {
        let (kind, message) = match self {
            Self::Ready => return None,
            Self::HelperUnavailable | Self::HelperStale => (
                RemediationKind::RestartHelper,
                "Restart d2b-unsafe-local-helper.service in the user session",
            ),
            Self::UserManagerUnavailable => (
                RemediationKind::StartUserManager,
                "Sign in through a PAM-backed graphical session",
            ),
            Self::GraphicalSessionInactive => (
                RemediationKind::StartGraphicalSession,
                "Start an active graphical user session",
            ),
            Self::WaylandUnavailable => (
                RemediationKind::RestoreWayland,
                "Restore the Wayland user session",
            ),
            Self::ProxyUnavailable => (
                RemediationKind::RepairProxy,
                "Repair d2b-wayland-proxy; direct compositor fallback is disabled",
            ),
            Self::Degraded => (
                RemediationKind::Retry,
                "Review d2b provider status and retry",
            ),
        };
        Some(TargetRemediation { kind, message })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RemediationKind {
    UpdateD2b,
    RestartHelper,
    StartUserManager,
    StartGraphicalSession,
    RestoreWayland,
    RepairProxy,
    Retry,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetRemediation {
    pub kind: RemediationKind,
    pub message: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ShellLauncherItem {
    pub id: String,
    pub name: String,
}

fn default_shell_feature_available() -> bool {
    true
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadSummary {
    pub target: TargetId,
    /// Legacy flat-card identifier retained for 0.1 JSON consumers.
    pub id: VmId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canonical_target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_vm_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workload_name: Option<String>,
    #[serde(default)]
    pub provider_kind: ProviderKind,
    #[serde(default)]
    pub isolation_posture: IsolationPosture,
    #[serde(default)]
    pub session_persistence: SessionPersistence,
    #[serde(default)]
    pub availability: TargetAvailability,
    #[serde(default = "default_shell_feature_available")]
    pub shell_feature_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell_launcher_item: Option<ShellLauncherItem>,
    pub power_state: VmPowerState,
    #[serde(default)]
    pub sessions: Vec<ShellSession>,
}

impl<'de> Deserialize<'de> for WorkloadSummary {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            #[serde(default)]
            target: Option<TargetId>,
            #[serde(default)]
            id: Option<VmId>,
            #[serde(default)]
            canonical_target: Option<String>,
            #[serde(default)]
            legacy_vm_name: Option<String>,
            #[serde(default)]
            workload_name: Option<String>,
            #[serde(default)]
            provider_kind: ProviderKind,
            #[serde(default)]
            isolation_posture: IsolationPosture,
            #[serde(default)]
            session_persistence: SessionPersistence,
            #[serde(default)]
            availability: TargetAvailability,
            #[serde(default = "default_shell_feature_available")]
            shell_feature_available: bool,
            #[serde(default)]
            shell_launcher_item: Option<ShellLauncherItem>,
            #[serde(default)]
            power_state: VmPowerState,
            #[serde(default)]
            sessions: Vec<ShellSession>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let canonical = wire
            .canonical_target
            .as_deref()
            .map(TargetId::new)
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let target = wire
            .target
            .or(canonical)
            .or_else(|| wire.id.clone())
            .ok_or_else(|| serde::de::Error::custom("workload target is required"))?;
        let id = wire.id.unwrap_or_else(|| {
            wire.legacy_vm_name
                .as_deref()
                .and_then(|legacy| TargetId::new(legacy).ok())
                .unwrap_or_else(|| target.clone())
        });
        let legacy_vm_name = wire
            .legacy_vm_name
            .or_else(|| (id != target && !id.is_canonical()).then(|| id.as_str().to_string()));
        let canonical_target = target.is_canonical().then(|| target.as_str().to_string());

        Ok(Self {
            target,
            id,
            canonical_target,
            legacy_vm_name,
            workload_name: wire.workload_name,
            provider_kind: wire.provider_kind,
            isolation_posture: wire.isolation_posture,
            session_persistence: wire.session_persistence,
            availability: wire.availability,
            shell_feature_available: wire.shell_feature_available,
            shell_launcher_item: wire.shell_launcher_item,
            power_state: wire.power_state,
            sessions: wire.sessions,
        })
    }
}

impl WorkloadSummary {
    pub fn new(target: TargetId, power_state: VmPowerState) -> Self {
        let canonical_target = target.is_canonical().then(|| target.as_str().to_string());
        Self {
            id: target.clone(),
            target,
            canonical_target,
            legacy_vm_name: None,
            workload_name: None,
            provider_kind: ProviderKind::LocalVm,
            isolation_posture: IsolationPosture::VirtualMachine,
            session_persistence: SessionPersistence::RuntimeManaged,
            availability: TargetAvailability::Ready,
            shell_feature_available: true,
            shell_launcher_item: None,
            power_state,
            sessions: Vec::new(),
        }
    }

    pub fn discovered(
        target: TargetId,
        compatibility_id: TargetId,
        power_state: VmPowerState,
    ) -> Self {
        let mut summary = Self::new(target.clone(), power_state);
        summary.id = compatibility_id;
        summary.canonical_target = Some(target.as_str().to_string());
        summary
    }

    pub fn visual_state(&self) -> TargetVisualState {
        if self.actions_available() {
            TargetVisualState {
                power_state: self.power_state,
                can_list_sessions: true,
                can_create_session: true,
                can_open_session: true,
            }
        } else {
            TargetVisualState {
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

    pub const fn actions_available(&self) -> bool {
        self.power_state.is_online() && self.availability.is_ready() && self.shell_feature_available
    }

    pub const fn requires_unsafe_local_shell(&self) -> bool {
        self.provider_kind.is_unsafe_local()
    }

    pub const fn remediation(&self) -> Option<TargetRemediation> {
        if !self.shell_feature_available {
            return Some(TargetRemediation {
                kind: RemediationKind::UpdateD2b,
                message: "Update d2b, d2bd, and d2b-unsafe-local-helper together",
            });
        }
        self.availability.remediation()
    }
}

pub type VmSummary = WorkloadSummary;

/// Extract the realm segment from a canonical workload target of the form
/// `<workload>.<realm>[...].d2b`.
///
/// Returns `None` when the target does not end with `.d2b` or has fewer than
/// two dot-separated segments before the suffix.
///
/// ```
/// use wlterm_core::realm_from_canonical_target;
/// assert_eq!(realm_from_canonical_target("dev-general.dev.d2b"), Some("dev"));
/// assert_eq!(realm_from_canonical_target("work-aad.local.d2b"), Some("local"));
/// assert_eq!(realm_from_canonical_target("dev-general.dev.local.d2b"), Some("dev"));
/// assert_eq!(realm_from_canonical_target("no-realm.d2b"), None);
/// assert_eq!(realm_from_canonical_target("not-a-target"), None);
/// ```
pub fn realm_from_canonical_target(target: &str) -> Option<&str> {
    let without_suffix = target.strip_suffix(".d2b")?;
    let mut parts = without_suffix.splitn(3, '.');
    parts.next()?; // workload segment
    parts.next() // realm segment
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetVisualState {
    pub power_state: VmPowerState,
    pub can_list_sessions: bool,
    pub can_create_session: bool,
    pub can_open_session: bool,
}

pub type VmVisualState = TargetVisualState;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    config: Config,
    workloads: BTreeMap<TargetId, WorkloadSummary>,
    async_errors: Vec<AsyncErrorEvent>,
    global_remediation: Option<TargetRemediation>,
}

impl Model {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            workloads: BTreeMap::new(),
            async_errors: Vec::new(),
            global_remediation: None,
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn workloads(&self) -> impl Iterator<Item = &WorkloadSummary> {
        self.workloads.values()
    }

    pub fn target(&self, target: &TargetId) -> Option<&WorkloadSummary> {
        self.workloads.get(target).or_else(|| {
            self.workloads.values().find(|summary| {
                summary.id == *target
                    || summary
                        .legacy_vm_name
                        .as_deref()
                        .is_some_and(|legacy| legacy == target.as_str())
            })
        })
    }

    pub fn vms(&self) -> impl Iterator<Item = &VmSummary> {
        self.workloads()
    }

    pub fn vm(&self, vm: &VmId) -> Option<&VmSummary> {
        self.target(vm)
    }

    pub fn async_errors(&self) -> &[AsyncErrorEvent] {
        &self.async_errors
    }

    pub const fn global_remediation(&self) -> Option<TargetRemediation> {
        self.global_remediation
    }

    pub fn apply(&mut self, event: ModelEvent) {
        match event {
            ModelEvent::WorkloadSnapshot { workloads } => {
                self.workloads = workloads
                    .into_iter()
                    .map(|workload| (workload.target.clone(), workload))
                    .collect();
            }
            ModelEvent::WorkloadChanged { workload } => {
                self.workloads.insert(workload.target.clone(), workload);
            }
            ModelEvent::VmSnapshot { vms } => {
                self.apply(ModelEvent::WorkloadSnapshot { workloads: vms });
            }
            ModelEvent::VmChanged { vm } => {
                self.apply(ModelEvent::WorkloadChanged { workload: vm });
            }
            ModelEvent::AsyncError { message } => {
                self.async_errors.push(AsyncErrorEvent::new(
                    message,
                    self.config.ui.async_error_display,
                ));
            }
            ModelEvent::GlobalRemediation { remediation } => {
                self.global_remediation = Some(remediation);
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
            UserIntent::RefreshTargets => PlannedAction::RefreshTargets,
            UserIntent::ListSessions { target } => {
                self.plan_available(target, |target| PlannedAction::ListSessions { target })
            }
            UserIntent::CreateSession { target, name } => {
                self.plan_available(target, |target| PlannedAction::AttachShell {
                    target,
                    name: Some(name),
                    force: false,
                })
            }
            UserIntent::OpenSession { target, name } => self.plan_open(target, name),
            UserIntent::StopShell {
                target,
                name,
                confirmed,
            } => {
                let Some(summary) = self.target(&target) else {
                    return PlannedAction::Disabled {
                        reason: DisabledReason::TargetUnknown,
                    };
                };
                let Some(endpoint) = self.available_shell_target(summary) else {
                    return PlannedAction::Disabled {
                        reason: disabled_reason(summary),
                    };
                };
                if self.config.ui.stop_confirmation && !confirmed {
                    PlannedAction::PromptStop {
                        target: endpoint,
                        name,
                    }
                } else {
                    PlannedAction::KillShell {
                        target: endpoint,
                        name,
                    }
                }
            }
            UserIntent::DetachShell { target, name } => {
                self.plan_available(target, |target| PlannedAction::DetachShell { target, name })
            }
            UserIntent::ForceOpenSession { target, name } => {
                self.plan_available(target, |target| PlannedAction::AttachShell {
                    target,
                    name: Some(name),
                    force: true,
                })
            }
        }
    }

    fn plan_open(&self, target: TargetId, name: FriendlyName) -> PlannedAction {
        let Some(summary) = self.target(&target) else {
            return PlannedAction::Disabled {
                reason: DisabledReason::TargetUnknown,
            };
        };
        let Some(endpoint) = self.available_shell_target(summary) else {
            return PlannedAction::Disabled {
                reason: disabled_reason(summary),
            };
        };

        if summary
            .session(&name)
            .is_some_and(ShellSession::is_attached)
        {
            match self.config.ui.default_open_behavior {
                OpenBehavior::FocusExisting => PlannedAction::FocusExistingShell {
                    target: endpoint,
                    name,
                },
                OpenBehavior::Prompt => PlannedAction::PromptAlreadyAttached {
                    target: endpoint,
                    name,
                },
                OpenBehavior::ForceOpen => PlannedAction::AttachShell {
                    target: endpoint,
                    name: Some(name),
                    force: true,
                },
            }
        } else {
            PlannedAction::AttachShell {
                target: endpoint,
                name: Some(name),
                force: false,
            }
        }
    }

    fn plan_available(
        &self,
        target: TargetId,
        action: impl FnOnce(ShellTarget) -> PlannedAction,
    ) -> PlannedAction {
        let Some(summary) = self.target(&target) else {
            return PlannedAction::Disabled {
                reason: DisabledReason::TargetUnknown,
            };
        };
        match self.available_shell_target(summary) {
            Some(target) => action(target),
            None => PlannedAction::Disabled {
                reason: disabled_reason(summary),
            },
        }
    }

    fn available_shell_target(&self, summary: &WorkloadSummary) -> Option<ShellTarget> {
        summary.actions_available().then(|| summary.shell_target())
    }
}

fn disabled_reason(summary: &WorkloadSummary) -> DisabledReason {
    if !summary.shell_feature_available {
        DisabledReason::UpdateRequired
    } else if !summary.availability.is_ready() {
        DisabledReason::ProviderUnavailable
    } else {
        DisabledReason::TargetOffline
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellTarget {
    pub id: TargetId,
    pub provider_kind: ProviderKind,
}

impl WorkloadSummary {
    pub fn shell_target(&self) -> ShellTarget {
        ShellTarget {
            id: self.target.clone(),
            provider_kind: self.provider_kind,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelEvent {
    WorkloadSnapshot {
        workloads: Vec<WorkloadSummary>,
    },
    WorkloadChanged {
        workload: WorkloadSummary,
    },
    /// Compatibility event accepted from 0.1 frontends.
    VmSnapshot {
        vms: Vec<VmSummary>,
    },
    /// Compatibility event accepted from 0.1 frontends.
    VmChanged {
        vm: VmSummary,
    },
    AsyncError {
        message: String,
    },
    GlobalRemediation {
        remediation: TargetRemediation,
    },
    DismissAsyncError {
        index: usize,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub enum UserIntent {
    RefreshTargets,
    ListSessions {
        target: TargetId,
    },
    CreateSession {
        target: TargetId,
        name: FriendlyName,
    },
    OpenSession {
        target: TargetId,
        name: FriendlyName,
    },
    ForceOpenSession {
        target: TargetId,
        name: FriendlyName,
    },
    StopShell {
        target: TargetId,
        name: FriendlyName,
        confirmed: bool,
    },
    DetachShell {
        target: TargetId,
        name: FriendlyName,
    },
}

impl fmt::Debug for UserIntent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefreshTargets => f.write_str("RefreshTargets"),
            Self::ListSessions { target } => f
                .debug_struct("ListSessions")
                .field("target", target)
                .finish(),
            Self::CreateSession { target, .. } => f
                .debug_struct("CreateSession")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::OpenSession { target, .. } => f
                .debug_struct("OpenSession")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::ForceOpenSession { target, .. } => f
                .debug_struct("ForceOpenSession")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::StopShell {
                target, confirmed, ..
            } => f
                .debug_struct("StopShell")
                .field("target", target)
                .field("name", &"<redacted>")
                .field("confirmed", confirmed)
                .finish(),
            Self::DetachShell { target, .. } => f
                .debug_struct("DetachShell")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub enum PlannedAction {
    RefreshTargets,
    ListSessions {
        target: ShellTarget,
    },
    AttachShell {
        target: ShellTarget,
        name: Option<FriendlyName>,
        force: bool,
    },
    FocusExistingShell {
        target: ShellTarget,
        name: FriendlyName,
    },
    PromptAlreadyAttached {
        target: ShellTarget,
        name: FriendlyName,
    },
    PromptStop {
        target: ShellTarget,
        name: FriendlyName,
    },
    KillShell {
        target: ShellTarget,
        name: FriendlyName,
    },
    DetachShell {
        target: ShellTarget,
        name: FriendlyName,
    },
    Disabled {
        reason: DisabledReason,
    },
}

impl PlannedAction {
    pub const fn metrics_label_value(&self) -> &'static str {
        match self {
            Self::RefreshTargets => "refresh-targets",
            Self::ListSessions { .. } => "list-sessions",
            Self::AttachShell { force, .. } if *force => "attach-shell-force",
            Self::AttachShell { .. } => "attach-shell",
            Self::FocusExistingShell { .. } => "focus-existing-shell",
            Self::PromptAlreadyAttached { .. } => "prompt-already-attached",
            Self::PromptStop { .. } => "prompt-stop",
            Self::KillShell { .. } => "kill-shell",
            Self::DetachShell { .. } => "detach-shell",
            Self::Disabled { reason } => reason.metrics_label_value(),
        }
    }
}

impl fmt::Debug for PlannedAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RefreshTargets => f.write_str("RefreshTargets"),
            Self::ListSessions { target } => f
                .debug_struct("ListSessions")
                .field("target", target)
                .finish(),
            Self::AttachShell {
                target,
                name,
                force,
            } => f
                .debug_struct("AttachShell")
                .field("target", target)
                .field("has_name", &name.is_some())
                .field("force", force)
                .finish(),
            Self::FocusExistingShell { target, .. } => f
                .debug_struct("FocusExistingShell")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptAlreadyAttached { target, .. } => f
                .debug_struct("PromptAlreadyAttached")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::PromptStop { target, .. } => f
                .debug_struct("PromptStop")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::KillShell { target, .. } => f
                .debug_struct("KillShell")
                .field("target", target)
                .field("name", &"<redacted>")
                .finish(),
            Self::DetachShell { target, .. } => f
                .debug_struct("DetachShell")
                .field("target", target)
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
    TargetOffline,
    TargetUnknown,
    ProviderUnavailable,
    UpdateRequired,
    /// Compatibility reason retained for 0.1 consumers.
    VmOffline,
    /// Compatibility reason retained for 0.1 consumers.
    VmUnknown,
}

impl DisabledReason {
    pub const fn metrics_label_value(self) -> &'static str {
        match self {
            Self::TargetOffline => "disabled-target-offline",
            Self::TargetUnknown => "disabled-target-unknown",
            Self::ProviderUnavailable => "disabled-provider-unavailable",
            Self::UpdateRequired => "disabled-update-required",
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
    EmptyTargetId,
    InvalidTargetId,
    EmptySessionId,
    InvalidCorrelation,
}

impl fmt::Display for ModelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyTargetId => "target id must not be empty",
            Self::InvalidTargetId => "target id has an invalid shape",
            Self::EmptySessionId => "session id must not be empty",
            Self::InvalidCorrelation => "correlation id has an invalid shape",
        })
    }
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

    fn endpoint(name: &str) -> ShellTarget {
        ShellTarget {
            id: vm(name),
            provider_kind: ProviderKind::LocalVm,
        }
    }

    fn model_with_vm(summary: VmSummary, config: Config) -> Model {
        let mut model = Model::new(config);
        model.apply(ModelEvent::VmSnapshot { vms: vec![summary] });
        model
    }

    #[test]
    fn default_config_exposes_safe_behavior_and_serializes_shape() {
        let cfg = Config::default();
        assert_eq!(cfg.public_socket_path, "/run/d2b/public.sock");
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
    fn target_id_debug_is_redacted() {
        let target = TargetId::new("tools.host.d2b").expect("target");
        let rendered = format!("{target:?}");
        assert!(!rendered.contains("tools.host.d2b"));
        assert!(rendered.contains("redacted"));
    }

    #[test]
    fn canonical_target_model_preserves_legacy_vm_serde_defaults() {
        let legacy: WorkloadSummary = serde_json::from_value(serde_json::json!({
            "id": "work",
            "powerState": "online",
            "sessions": []
        }))
        .expect("legacy 0.1 summary");
        assert_eq!(legacy.target.as_str(), "work");
        assert_eq!(legacy.id.as_str(), "work");
        assert_eq!(legacy.provider_kind, ProviderKind::LocalVm);
        assert!(legacy.shell_feature_available);

        let summary = WorkloadSummary::discovered(
            TargetId::new("builder.dev.d2b").unwrap(),
            TargetId::new("builder").unwrap(),
            TargetPowerState::Online,
        );
        let json = serde_json::to_value(&summary).unwrap();
        assert_eq!(json["target"], "builder.dev.d2b");
        assert_eq!(json["id"], "builder");
        assert_eq!(json["canonicalTarget"], "builder.dev.d2b");
    }

    #[test]
    fn unsafe_local_posture_is_explicit_and_update_skew_disables_actions() {
        let mut summary = WorkloadSummary::discovered(
            TargetId::new("tools.host.d2b").unwrap(),
            TargetId::new("tools").unwrap(),
            TargetPowerState::Online,
        );
        summary.provider_kind = ProviderKind::UnsafeLocal;
        summary.isolation_posture = IsolationPosture::UnsafeLocal;
        summary.session_persistence = SessionPersistence::UserManagerLifetime;
        summary.shell_feature_available = false;

        assert!(!summary.actions_available());
        assert_eq!(
            summary.remediation().unwrap().kind,
            RemediationKind::UpdateD2b
        );
        assert_eq!(
            summary.isolation_posture.warning(),
            Some("No isolation: runs in the host user session")
        );
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
            model.plan(UserIntent::ListSessions {
                target: work.clone()
            }),
            PlannedAction::Disabled {
                reason: DisabledReason::TargetOffline
            }
        );
        assert_eq!(
            model.plan(UserIntent::CreateSession {
                target: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::Disabled {
                reason: DisabledReason::TargetOffline
            }
        );
        assert_eq!(
            model.plan(UserIntent::OpenSession {
                target: work,
                name: shell("quiet-otter")
            }),
            PlannedAction::Disabled {
                reason: DisabledReason::TargetOffline
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
                target: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::FocusExistingShell {
                target: endpoint("work"),
                name: shell("quiet-otter")
            }
        );

        let mut prompt_cfg = Config::default();
        prompt_cfg.ui.default_open_behavior = OpenBehavior::Prompt;
        let prompt_model = model_with_vm(summary.clone(), prompt_cfg);
        assert_eq!(
            prompt_model.plan(UserIntent::OpenSession {
                target: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::PromptAlreadyAttached {
                target: endpoint("work"),
                name: shell("quiet-otter")
            }
        );

        let mut force_cfg = Config::default();
        force_cfg.ui.default_open_behavior = OpenBehavior::ForceOpen;
        let force_model = model_with_vm(summary, force_cfg);
        assert_eq!(
            force_model.plan(UserIntent::OpenSession {
                target: work.clone(),
                name: shell("quiet-otter")
            }),
            PlannedAction::AttachShell {
                target: endpoint("work"),
                name: Some(shell("quiet-otter")),
                force: true
            }
        );
    }

    #[test]
    fn stop_requires_confirmation_until_confirmed() {
        let work = vm("work");
        let model = model_with_vm(
            VmSummary::new(work.clone(), VmPowerState::Online),
            Config::default(),
        );
        let name = shell("quiet-otter");

        assert_eq!(
            model.plan(UserIntent::StopShell {
                target: work.clone(),
                name: name.clone(),
                confirmed: false
            }),
            PlannedAction::PromptStop {
                target: endpoint("work"),
                name: name.clone()
            }
        );
        assert_eq!(
            model.plan(UserIntent::StopShell {
                target: work,
                name: name.clone(),
                confirmed: true
            }),
            PlannedAction::KillShell {
                target: endpoint("work"),
                name
            }
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
            target: endpoint("work"),
            name: Some(shell("customer-project-shell")),
            force: false,
        };

        assert_eq!(action.metrics_label_value(), "attach-shell");
        assert!(!action.metrics_label_value().contains("customer"));
        assert_eq!(ShellVisualState::Attached.metrics_label_value(), "attached");
    }

    #[test]
    fn realm_from_canonical_target_extracts_realm_segment() {
        assert_eq!(
            realm_from_canonical_target("dev-general.dev.d2b"),
            Some("dev")
        );
        assert_eq!(
            realm_from_canonical_target("work-aad.local.d2b"),
            Some("local")
        );
        // multi-level targets: realm is the segment immediately after the workload
        assert_eq!(
            realm_from_canonical_target("dev-general.dev.local.d2b"),
            Some("dev")
        );
        assert_eq!(
            realm_from_canonical_target("home-media.home.corp.d2b"),
            Some("home")
        );
    }

    #[test]
    fn realm_from_canonical_target_rejects_non_targets() {
        // no .d2b suffix
        assert_eq!(realm_from_canonical_target("work-aad.local"), None);
        // only one segment before .d2b (no realm)
        assert_eq!(realm_from_canonical_target("no-realm.d2b"), None);
        // empty
        assert_eq!(realm_from_canonical_target(""), None);
        // bare suffix only
        assert_eq!(realm_from_canonical_target(".d2b"), None);
    }
}
