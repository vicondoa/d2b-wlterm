//! UI state concepts for d2b-wlterm frontends.

use serde::Serialize;
use sha2::{Digest, Sha256};
use wlterm_core::friendly_name::FriendlyName;
use wlterm_core::{
    AsyncErrorDisplay, AsyncErrorEvent as CoreAsyncErrorEvent, Model, OpenBehavior,
    SafeCorrelation, SessionId, ShellVisualState, VmPowerState,
};

pub const DISPLAY_LABEL_MAX_CHARS: usize = 40;
pub const EMPTY_LABEL_PLACEHOLDER: &str = "unnamed-shell";

#[derive(Clone, PartialEq, Eq)]
pub enum OpenDecision {
    OpenNew { session: String },
    FocusExisting { session: String },
    Prompt { session: String },
    ForceOpen { session: String },
}

impl std::fmt::Debug for OpenDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenNew { .. } => f
                .debug_struct("OpenNew")
                .field("session", &"<redacted>")
                .finish(),
            Self::FocusExisting { .. } => f
                .debug_struct("FocusExisting")
                .field("session", &"<redacted>")
                .finish(),
            Self::Prompt { .. } => f
                .debug_struct("Prompt")
                .field("session", &"<redacted>")
                .finish(),
            Self::ForceOpen { .. } => f
                .debug_struct("ForceOpen")
                .field("session", &"<redacted>")
                .finish(),
        }
    }
}

pub fn decide_open(
    session: &SessionId,
    already_attached: bool,
    behavior: OpenBehavior,
) -> OpenDecision {
    if !already_attached {
        return OpenDecision::OpenNew {
            session: session.as_str().to_string(),
        };
    }

    match behavior {
        OpenBehavior::FocusExisting => OpenDecision::FocusExisting {
            session: session.as_str().to_string(),
        },
        OpenBehavior::ForceOpen => OpenDecision::ForceOpen {
            session: session.as_str().to_string(),
        },
        OpenBehavior::Prompt => OpenDecision::Prompt {
            session: session.as_str().to_string(),
        },
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct StopRequest {
    pub session: String,
    pub requires_confirmation: bool,
}

impl std::fmt::Debug for StopRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StopRequest")
            .field("session", &"<redacted>")
            .field("requires_confirmation", &self.requires_confirmation)
            .finish()
    }
}

impl StopRequest {
    pub fn new(session: &SessionId, requires_confirmation: bool) -> Self {
        Self {
            session: session.as_str().to_string(),
            requires_confirmation,
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
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlCenterState {
    pub vms: Vec<VmControlCard>,
    pub active_shells: usize,
    pub has_error: bool,
    pub errors: Vec<RenderedAsyncError>,
}

impl ControlCenterState {
    pub fn from_model(model: &Model) -> Self {
        let errors: Vec<_> = model
            .async_errors()
            .iter()
            .filter_map(RenderedAsyncError::from_core)
            .collect();
        let vms: Vec<_> = model.vms().map(VmControlCard::from_summary).collect();
        let active_shells = vms.iter().map(|vm| vm.active_shells).sum();

        Self {
            vms,
            active_shells,
            has_error: !errors.is_empty(),
            errors,
        }
    }

    pub fn empty() -> Self {
        Self {
            vms: Vec::new(),
            active_shells: 0,
            has_error: false,
            errors: Vec::new(),
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("control center state serializes")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VmControlCard {
    pub id: String,
    pub label: String,
    pub power_state: VmPowerState,
    pub disabled: bool,
    pub disabled_reason: Option<String>,
    pub active_shells: usize,
    pub shells: Vec<ShellControlRow>,
}

impl VmControlCard {
    fn from_summary(summary: &wlterm_core::VmSummary) -> Self {
        let disabled = !summary.power_state.is_online();
        let shells: Vec<_> = summary
            .sessions
            .iter()
            .map(ShellControlRow::from_session)
            .collect();
        let active_shells = shells
            .iter()
            .filter(|shell| shell.visual_state != ShellVisualState::Unavailable)
            .count();

        Self {
            id: summary.id.as_str().to_string(),
            label: sanitize_display_label(summary.id.as_str()),
            power_state: summary.power_state,
            disabled,
            disabled_reason: disabled.then(|| match summary.power_state {
                VmPowerState::Offline => "vm-offline".to_string(),
                VmPowerState::Unknown => "vm-state-unknown".to_string(),
                VmPowerState::Online => "disabled".to_string(),
            }),
            active_shells,
            shells,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellControlRow {
    pub name: String,
    pub visual_state: ShellVisualState,
    pub is_default: bool,
    pub attached: bool,
    pub actions: Vec<&'static str>,
}

impl ShellControlRow {
    fn from_session(session: &wlterm_core::ShellSession) -> Self {
        let actions = match session.visual_state {
            ShellVisualState::Attached => vec!["focus-existing", "prompt-force-open", "stop"],
            ShellVisualState::Detached => vec!["open", "stop"],
            ShellVisualState::Unavailable => Vec::new(),
        };

        Self {
            name: sanitize_display_label(session.name.as_str()),
            visual_state: session.visual_state.clone(),
            is_default: session.is_default,
            attached: session.is_attached(),
            actions,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellNamePrompt {
    pub default_name: String,
    pub typed_text: String,
    pub resolved_name: Option<String>,
    pub error: Option<String>,
}

impl ShellNamePrompt {
    pub fn new(typed_text: &str) -> Self {
        let default_name = FriendlyName::generate()
            .map(|name| name.as_str().to_string())
            .unwrap_or_else(|_| "fresh-shell".to_string());
        Self::with_default(&default_name, typed_text)
    }

    pub fn with_default(default_name: &str, typed_text: &str) -> Self {
        let candidate = if typed_text.trim().is_empty() {
            default_name
        } else {
            typed_text.trim()
        };
        match FriendlyName::from_candidate(candidate) {
            Ok(name) => Self {
                default_name: sanitize_display_label(default_name),
                typed_text: sanitize_display_label(typed_text),
                resolved_name: Some(name.as_str().to_string()),
                error: None,
            },
            Err(_) => Self {
                default_name: sanitize_display_label(default_name),
                typed_text: sanitize_display_label(typed_text),
                resolved_name: None,
                error: Some("shell-name-invalid".to_string()),
            },
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("shell name prompt serializes")
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlreadyAttachedNotice {
    pub mode: &'static str,
    pub shell: String,
    pub message: String,
    pub allow_force_open: bool,
}

impl AlreadyAttachedNotice {
    pub fn for_behavior(shell_name: &str, behavior: OpenBehavior) -> Self {
        let shell = sanitize_display_label(shell_name);
        match behavior {
            OpenBehavior::FocusExisting => Self {
                mode: "toast",
                shell: shell.clone(),
                message: format!("Focusing {shell}; use force-open if focus is unavailable."),
                allow_force_open: true,
            },
            OpenBehavior::Prompt => Self {
                mode: "prompt",
                shell: shell.clone(),
                message: format!("{shell} is already attached. Open another view?"),
                allow_force_open: true,
            },
            OpenBehavior::ForceOpen => Self {
                mode: "force-open",
                shell: shell.clone(),
                message: format!("Opening another view for {shell}."),
                allow_force_open: false,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalCloseDecision {
    pub action: &'static str,
    pub shell: String,
}

pub fn disconnect_terminal_view(session: &SessionId) -> TerminalCloseDecision {
    TerminalCloseDecision {
        action: "disconnect",
        shell: sanitize_display_label(session.as_str()),
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderedAsyncError {
    pub title: String,
    pub detail: String,
    pub display: AsyncErrorDisplay,
    pub correlation: Option<String>,
    pub digest: String,
}

impl RenderedAsyncError {
    pub fn from_event(event: &AsyncErrorEvent) -> Option<Self> {
        if !event.should_render() {
            return None;
        }
        Some(Self::from_parts(
            &event.message,
            event.display,
            event.correlation.as_ref(),
        ))
    }

    pub fn from_core(event: &CoreAsyncErrorEvent) -> Option<Self> {
        if !event.should_render() {
            return None;
        }
        Some(Self::from_parts(
            &event.message,
            event.display,
            event.correlation.as_ref(),
        ))
    }

    fn from_parts(
        message: &str,
        display: AsyncErrorDisplay,
        correlation: Option<&SafeCorrelation>,
    ) -> Self {
        let digest = digest_message(message, correlation);
        let correlation = correlation.map(|value| value.as_str().to_string());
        let detail = match &correlation {
            Some(correlation) => format!("correlation {correlation}; digest {digest}"),
            None => format!("digest {digest}"),
        };
        Self {
            title: "d2b-wlterm action failed".to_string(),
            detail,
            display,
            correlation,
            digest,
        }
    }
}

pub fn sanitize_display_label(value: &str) -> String {
    let mut sanitized = String::new();
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' {
            if matches!(chars.peek(), Some('[')) {
                chars.next();
                for next in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&next) {
                        break;
                    }
                }
            }
            continue;
        }
        if ch == '\u{9b}' {
            for next in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&next) {
                    break;
                }
            }
            continue;
        }
        if ch.is_control() {
            continue;
        }
        sanitized.push(ch);
    }

    let sanitized = sanitized.trim();
    if sanitized.is_empty() {
        return EMPTY_LABEL_PLACEHOLDER.to_string();
    }

    let mut truncated = String::new();
    for ch in sanitized.chars().take(DISPLAY_LABEL_MAX_CHARS) {
        truncated.push(ch);
    }
    if truncated.is_empty() {
        EMPTY_LABEL_PLACEHOLDER.to_string()
    } else {
        truncated
    }
}

fn digest_message(message: &str, correlation: Option<&SafeCorrelation>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"d2b-wlterm-ui-error");
    hasher.update((message.len() as u64).to_le_bytes());
    hasher.update(message.as_bytes());
    if let Some(correlation) = correlation {
        hasher.update(correlation.as_str().as_bytes());
    }
    let digest = hasher.finalize();
    let mut rendered = String::with_capacity(12);
    for byte in &digest[..6] {
        rendered.push_str(&format!("{byte:02x}"));
    }
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;
    use wlterm_core::friendly_name::FriendlyName;
    use wlterm_core::{
        Config, ModelEvent, PlannedAction, ShellSession, UserIntent, VmId, VmSummary,
    };

    fn vm(name: &str) -> VmId {
        VmId::new(name).unwrap()
    }

    fn shell(name: &str) -> FriendlyName {
        FriendlyName::from_candidate(name).unwrap()
    }

    #[test]
    fn already_attached_open_focuses_by_default() {
        let session = SessionId::new("work").unwrap();
        assert_eq!(
            decide_open(&session, true, OpenBehavior::FocusExisting),
            OpenDecision::FocusExisting {
                session: "work".into()
            }
        );
    }

    #[test]
    fn already_attached_open_can_force_new_attachment() {
        let session = SessionId::new("work").unwrap();
        assert_eq!(
            decide_open(&session, true, OpenBehavior::ForceOpen),
            OpenDecision::ForceOpen {
                session: "work".into()
            }
        );
    }

    #[test]
    fn stop_request_keeps_confirmation_explicit() {
        let session = SessionId::new("work").unwrap();
        assert!(StopRequest::new(&session, true).requires_confirmation);
    }

    #[test]
    fn disconnect_view_is_not_stop() {
        let session = SessionId::new("quiet-otter").unwrap();
        let decision = disconnect_terminal_view(&session);
        assert_eq!(decision.action, "disconnect");
        assert_ne!(decision.action, "stop");
    }

    #[test]
    fn manual_shell_name_prompt_defaults_or_overrides() {
        let defaulted = ShellNamePrompt::with_default("quiet-otter", "");
        assert_eq!(defaulted.resolved_name.as_deref(), Some("quiet-otter"));
        assert_eq!(defaulted.error, None);

        let override_name = ShellNamePrompt::with_default("quiet-otter", "brave-panda");
        assert_eq!(override_name.resolved_name.as_deref(), Some("brave-panda"));

        let invalid = ShellNamePrompt::with_default("quiet-otter", "bad/name");
        assert_eq!(invalid.resolved_name, None);
        assert_eq!(invalid.error.as_deref(), Some("shell-name-invalid"));
    }

    #[test]
    fn already_attached_notice_covers_focus_prompt_and_force_open() {
        let focus = AlreadyAttachedNotice::for_behavior("quiet-otter", OpenBehavior::FocusExisting);
        assert_eq!(focus.mode, "toast");
        assert!(focus.allow_force_open);

        let prompt = AlreadyAttachedNotice::for_behavior("quiet-otter", OpenBehavior::Prompt);
        assert_eq!(prompt.mode, "prompt");
        assert!(prompt.allow_force_open);

        let force = AlreadyAttachedNotice::for_behavior("quiet-otter", OpenBehavior::ForceOpen);
        assert_eq!(force.mode, "force-open");
        assert!(!force.allow_force_open);
    }

    #[test]
    fn control_center_state_marks_offline_vm_disabled() {
        let work = vm("work");
        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::VmSnapshot {
            vms: vec![VmSummary::new(work.clone(), VmPowerState::Offline)],
        });

        assert_eq!(
            model.plan(UserIntent::ListSessions { vm: work }),
            PlannedAction::Disabled {
                reason: wlterm_core::DisabledReason::VmOffline
            }
        );

        let state = ControlCenterState::from_model(&model);
        assert!(state.vms[0].disabled);
        assert_eq!(state.vms[0].disabled_reason.as_deref(), Some("vm-offline"));
    }

    #[test]
    fn control_center_counts_active_shells_and_renders_errors() {
        let mut summary = VmSummary::new(vm("work"), VmPowerState::Online);
        summary
            .sessions
            .push(ShellSession::attached(shell("quiet-otter")));
        summary
            .sessions
            .push(ShellSession::detached(shell("brave-panda")));

        let mut model = Model::new(Config::default());
        model.apply(ModelEvent::VmSnapshot { vms: vec![summary] });
        model.apply(ModelEvent::AsyncError {
            message: "contains \u{1b}[31mquiet-otter\u{1b}[0m and opaque handle".into(),
        });

        let state = ControlCenterState::from_model(&model);
        assert_eq!(state.active_shells, 2);
        assert!(state.has_error);
        assert_eq!(state.errors[0].title, "d2b-wlterm action failed");
        assert!(!state.to_json().contains("quiet-otter and opaque"));
    }

    #[test]
    fn async_errors_render_safe_correlation_and_digest() {
        let correlation = SafeCorrelation::new("wlterm-deadbeef").unwrap();
        let event = AsyncErrorEvent::with_correlation(
            "contains quiet-otter and opaque-session-handle",
            AsyncErrorDisplay::Inline,
            correlation,
        );

        let rendered = RenderedAsyncError::from_event(&event).unwrap();
        let json = serde_json::to_string(&rendered).unwrap();
        assert!(json.contains("wlterm-deadbeef"));
        assert!(json.contains("digest"));
        assert!(!json.contains("quiet-otter"));
        assert!(!json.contains("opaque-session-handle"));
    }

    #[test]
    fn silent_async_errors_do_not_render() {
        let event = AsyncErrorEvent::new("late failure", AsyncErrorDisplay::Silent);
        assert!(!event.should_render());
        assert!(RenderedAsyncError::from_event(&event).is_none());
    }

    #[test]
    fn labels_strip_ansi_controls_and_truncate() {
        let raw = "\u{1b}[31mquiet\u{1b}[0m\n-otter";
        assert_eq!(sanitize_display_label(raw), "quiet-otter");
        assert_eq!(
            sanitize_display_label("\u{1b}[31m\n\t"),
            EMPTY_LABEL_PLACEHOLDER
        );

        let long = "a".repeat(DISPLAY_LABEL_MAX_CHARS + 10);
        assert_eq!(
            sanitize_display_label(&long).chars().count(),
            DISPLAY_LABEL_MAX_CHARS
        );
    }

    #[test]
    fn debug_redacts_session_names() {
        let session = SessionId::new("quiet-otter").unwrap();
        let open = format!(
            "{:?}",
            decide_open(&session, true, OpenBehavior::FocusExisting)
        );
        let stop = format!("{:?}", StopRequest::new(&session, true));
        assert!(!open.contains("quiet-otter"));
        assert!(!stop.contains("quiet-otter"));
        assert!(open.contains("redacted"));
        assert!(stop.contains("redacted"));
    }
}
