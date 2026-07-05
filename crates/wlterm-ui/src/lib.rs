//! UI state concepts for d2b-wlterm frontends.

use wlterm_core::{AsyncErrorDisplay, OpenBehavior, SessionId};

#[derive(Clone, PartialEq, Eq)]
pub enum OpenDecision {
    OpenNew { session: String },
    FocusExisting { session: String },
    Prompt { session: String },
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
        OpenBehavior::OpenNew => OpenDecision::OpenNew {
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
}

impl AsyncErrorEvent {
    pub fn new(message: impl Into<String>, display: AsyncErrorDisplay) -> Self {
        Self {
            message: message.into(),
            display,
        }
    }

    pub fn should_render(&self) -> bool {
        self.display != AsyncErrorDisplay::Silent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn stop_request_keeps_confirmation_explicit() {
        let session = SessionId::new("work").unwrap();
        assert!(StopRequest::new(&session, true).requires_confirmation);
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

    #[test]
    fn silent_async_errors_do_not_render() {
        let event = AsyncErrorEvent::new("late failure", AsyncErrorDisplay::Silent);
        assert!(!event.should_render());
    }
}
