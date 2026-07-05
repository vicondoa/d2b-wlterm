//! d2b adapter boundary.
//!
//! TODO(d2b-toolkit): replace this stub with the local d2b-toolkit client once
//! the sibling toolkit repository is available.

use wlterm_core::SessionId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bClientConfig {
    pub public_socket_path: String,
}

impl Default for D2bClientConfig {
    fn default() -> Self {
        Self {
            public_socket_path: default_public_socket_path(),
        }
    }
}

fn default_public_socket_path() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .map(|dir| format!("{dir}/d2b/public.sock"))
        .unwrap_or_else(|_| "/run/d2b/public.sock".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct D2bClient {
    config: D2bClientConfig,
}

impl D2bClient {
    pub fn new(config: D2bClientConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &D2bClientConfig {
        &self.config
    }

    pub fn describe_session(&self, session: &SessionId) -> D2bSessionStatus {
        D2bSessionStatus {
            session: session.as_str().to_string(),
            state: D2bVmState::Unknown,
            attached: false,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct D2bSessionStatus {
    pub session: String,
    pub state: D2bVmState,
    pub attached: bool,
}

impl std::fmt::Debug for D2bSessionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("D2bSessionStatus")
            .field("session", &"<redacted>")
            .field("state", &self.state)
            .field("attached", &self.attached)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum D2bVmState {
    Running,
    Stopped,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_status_is_explicitly_unknown() {
        let client = D2bClient::new(D2bClientConfig::default());
        let session = SessionId::new("work").unwrap();
        let status = client.describe_session(&session);
        assert_eq!(status.state, D2bVmState::Unknown);
        assert!(!status.attached);
    }

    #[test]
    fn default_socket_path_is_not_unexpanded_shell_syntax() {
        let config = D2bClientConfig::default();
        assert!(!config.public_socket_path.contains("$XDG_RUNTIME_DIR"));
        assert!(config.public_socket_path.ends_with("/d2b/public.sock"));
    }

    #[test]
    fn status_debug_redacts_session_name() {
        let status = D2bSessionStatus {
            session: "quiet-otter".to_string(),
            state: D2bVmState::Unknown,
            attached: false,
        };
        let rendered = format!("{status:?}");
        assert!(!rendered.contains("quiet-otter"));
        assert!(rendered.contains("redacted"));
    }
}
